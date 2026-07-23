use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard, Once},
};

use actix_web::{
    http::StatusCode,
    test::{call_service, init_service, read_body, TestRequest},
};
use lettre::Message;

use crate::{
    create_app,
    error::Error,
    mailer::MailSender,
    metrics::{register_metrics, PROCESSED_MAIL_REQUESTS, PROCESSING_TIME, UP_METRIC},
    models, State,
};

const API_KEY: &str = "test-api-key";
const ALLOWED_FROM: &str = "allowed@example.com";

struct RecordingMailer {
    sent: Mutex<Vec<Message>>,
}

impl RecordingMailer {
    fn new() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
        }
    }

    fn sent(&self) -> Vec<Message> {
        self.sent.lock().unwrap().clone()
    }
}

impl MailSender for RecordingMailer {
    fn send(&self, message: &Message) -> Result<(), Error> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }

    fn test_connection(&self) -> Result<bool, Error> {
        Ok(true)
    }
}

struct FailingMailer;

impl MailSender for FailingMailer {
    fn send(&self, _message: &Message) -> Result<(), Error> {
        Err(Error::String("smtp send failed".to_string()))
    }

    fn test_connection(&self) -> Result<bool, Error> {
        Ok(true)
    }
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_metrics_registered() {
    static INIT: Once = Once::new();
    INIT.call_once(register_metrics);
    // Prometheus omits unused vec/histogram samples from gather until observed.
    let _ = PROCESSED_MAIL_REQUESTS.with_label_values(&["success"]).get();
    let _ = PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get();
    let _ = UP_METRIC.with_label_values(&["mailserver"]).get();
    PROCESSING_TIME.observe(0.0);
}

fn test_state(mailer: Arc<dyn MailSender>) -> State {
    State {
        mailer,
        allowlist: HashSet::from([ALLOWED_FROM.to_string()]),
        api_key: API_KEY.to_string(),
    }
}

fn valid_mail_json() -> serde_json::Value {
    serde_json::json!({
        "from": ALLOWED_FROM,
        "to": "to@example.com",
        "subject": "hello",
        "body": "world"
    })
}

async fn json_body<B>(response: actix_web::dev::ServiceResponse<B>) -> models::Error
where
    B: actix_web::body::MessageBody,
{
    let body = read_body(response).await;
    serde_json::from_slice(&body).unwrap()
}

#[actix_web::test]
async fn livez_returns_ok() {
    let _guard = test_lock();
    let app = init_service(create_app(test_state(Arc::new(RecordingMailer::new())))).await;

    let response = call_service(
        &app,
        TestRequest::get().uri("/livez").to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(read_body(response).await, "ok");
}

#[actix_web::test]
async fn readyz_returns_ok() {
    let _guard = test_lock();
    let app = init_service(create_app(test_state(Arc::new(RecordingMailer::new())))).await;

    let response = call_service(
        &app,
        TestRequest::get().uri("/readyz").to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(read_body(response).await, "ok");
}

#[actix_web::test]
async fn metrics_contains_registered_names() {
    let _guard = test_lock();
    ensure_metrics_registered();
    let app = init_service(create_app(test_state(Arc::new(RecordingMailer::new())))).await;

    let response = call_service(
        &app,
        TestRequest::get().uri("/metrics").to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
    assert!(body.contains("processed_mail_requests"));
    assert!(body.contains("processing_time"));
    // Gauge families only appear in gather output after a label child exists.
    assert!(body.contains("up"));
}

#[actix_web::test]
async fn sendmail_happy_path() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let success_before = PROCESSED_MAIL_REQUESTS
        .with_label_values(&["success"])
        .get();

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .set_json(valid_mail_json())
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    let formatted = String::from_utf8(sent[0].formatted()).unwrap();
    assert!(formatted.contains(ALLOWED_FROM));
    assert!(formatted.contains("to@example.com"));
    assert!(formatted.contains("hello"));
    assert!(formatted.contains("world"));

    assert_eq!(
        PROCESSED_MAIL_REQUESTS
            .with_label_values(&["success"])
            .get(),
        success_before + 1
    );
}

#[actix_web::test]
async fn sendmail_with_cc_and_bcc() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let mut body = valid_mail_json();
    body["cc"] = serde_json::json!("cc@example.com");
    body["bcc"] = serde_json::json!("bcc@example.com");

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .set_json(body)
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let message = &mailer.sent()[0];
    let formatted = String::from_utf8(message.formatted()).unwrap();
    assert!(formatted.contains("cc@example.com"));
    // lettre strips Bcc from the rendered headers; recipients remain on the envelope.
    let envelope_recipients: Vec<String> = message
        .envelope()
        .to()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(envelope_recipients.iter().any(|a| a == "bcc@example.com"));
    assert!(envelope_recipients.iter().any(|a| a == "cc@example.com"));
}

#[actix_web::test]
async fn sendmail_missing_api_key_returns_401() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let error_before = PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get();

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .set_json(valid_mail_json())
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let json = json_body(response).await;
    assert!(json.message.is_some());
    assert!(json.error.is_none());
    assert!(mailer.sent().is_empty());
    // Auth failures return before send_mail metrics are updated.
    assert_eq!(
        PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get(),
        error_before
    );
}

#[actix_web::test]
async fn sendmail_wrong_api_key_returns_401() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", "wrong"))
            .set_json(valid_mail_json())
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(mailer.sent().is_empty());
}

#[actix_web::test]
async fn sendmail_non_allowlisted_from_returns_401() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let error_before = PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get();

    let mut body = valid_mail_json();
    body["from"] = serde_json::json!("other@example.com");

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .set_json(body)
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let json = json_body(response).await;
    assert!(json.message.unwrap().contains("Not allowed"));
    assert!(mailer.sent().is_empty());
    assert_eq!(
        PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get(),
        error_before + 1
    );
}

#[actix_web::test]
async fn sendmail_malformed_json_returns_500() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .insert_header(("Content-Type", "application/json"))
            .set_payload("{not-json")
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = json_body(response).await;
    assert!(json.error.is_some());
    assert!(json.message.is_none());
    assert!(mailer.sent().is_empty());
}

#[actix_web::test]
async fn sendmail_invalid_address_returns_500() {
    let _guard = test_lock();
    let mailer = Arc::new(RecordingMailer::new());
    let app = init_service(create_app(test_state(mailer.clone()))).await;

    let mut body = valid_mail_json();
    body["to"] = serde_json::json!("not-an-email");

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .set_json(body)
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = json_body(response).await;
    assert!(json.error.is_some());
    assert!(mailer.sent().is_empty());
}

#[actix_web::test]
async fn sendmail_transport_failure_returns_500() {
    let _guard = test_lock();
    let app = init_service(create_app(test_state(Arc::new(FailingMailer)))).await;

    let error_before = PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get();

    let response = call_service(
        &app,
        TestRequest::post()
            .uri("/api/sendmail")
            .insert_header(("X-API-KEY", API_KEY))
            .set_json(valid_mail_json())
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = json_body(response).await;
    assert!(json.error.unwrap().contains("smtp send failed"));
    assert_eq!(
        PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).get(),
        error_before + 1
    );
}

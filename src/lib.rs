#[macro_use]
extern crate serde;

use std::{collections::HashSet, str::from_utf8, sync::Arc, time::Instant};

use actix_web::{
    body::MessageBody,
    dev::{ServiceFactory, ServiceRequest, ServiceResponse},
    get, middleware::Logger, post, web, App, HttpRequest, Responder,
};
use lettre::{message::Mailbox, Message};

use crate::{
    error::Error,
    metrics::{get_metrics, PROCESSED_MAIL_REQUESTS, PROCESSING_TIME},
    models::Mail,
};

pub mod config;
pub mod error;
pub mod mailer;
pub mod mail_health;
pub mod metrics;
#[allow(dead_code, non_snake_case)]
pub mod models;

pub use config::AppConfig;
pub use mailer::MailSender;

#[cfg(test)]
mod app_tests;

/// Parse `ALLOWED_SENDERS` CSV. Trims each entry and drops empties.
/// An empty result (missing/blank env) means deny-all.
pub fn parse_allowlist(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

#[derive(Clone)]
pub struct State {
    pub mailer: Arc<dyn MailSender>,
    pub allowlist: HashSet<String>,
    pub api_key: String,
}

pub fn create_app(
    state: State,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<impl MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .wrap(
            Logger::default()
                .exclude("/livez".to_string())
                .exclude("/readyz".to_string())
                .log_target("http"),
        )
        .app_data(web::Data::new(state))
        .service(livez)
        .service(readyz)
        .service(metrics_endpoint)
        .service(sendmail)
}

#[get("/livez")]
async fn livez() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/readyz")]
async fn readyz() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/metrics")]
async fn metrics_endpoint() -> impl Responder {
    match get_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            tracing::error!(error = e.to_string(), "unable to gather metrics");
            "".to_string()
        }
    }
}

fn send_mail(body: web::Bytes, state: web::Data<State>) -> Result<&'static str, Error> {
    let mail = serde_json::from_str::<Mail>(from_utf8(&body)?)?;
    let from = mail.from.parse::<Mailbox>()?;
    if !state.allowlist.contains(&format!("{}", from.email)) {
        return Err(Error::Unauthorized(format!(
            "Not allowed to send from '{}'",
            from.email
        )));
    }
    let to = mail.to.parse::<Mailbox>()?;

    let mut message_builder = Message::builder();

    if let Some(cc) = mail.cc {
        let cc = cc.parse::<Mailbox>()?;
        message_builder = message_builder.cc(cc);
    }
    if let Some(bcc) = mail.bcc {
        let bcc = bcc.parse::<Mailbox>()?;
        message_builder = message_builder.bcc(bcc);
    }

    let message = message_builder
        .from(from)
        .to(to)
        .subject(mail.subject)
        .body(mail.body)?;

    state.mailer.send(&message)?;
    Ok("")
}

#[post("/api/sendmail")]
async fn sendmail(
    request: HttpRequest,
    body: web::Bytes,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    validate_api_key(&request, &state.api_key)?;

    let start_time = Instant::now();
    let result = send_mail(body, state);
    let elapsed_millis = start_time.elapsed().as_millis();
    PROCESSING_TIME.observe(elapsed_millis as f64 / 1000.0);

    match result {
        Ok(ok) => {
            PROCESSED_MAIL_REQUESTS
                .with_label_values(&["success"])
                .inc();
            Ok(ok)
        }
        Err(e) => {
            PROCESSED_MAIL_REQUESTS.with_label_values(&["error"]).inc();
            Err(e)
        }
    }
}

pub fn validate_api_key(request: &HttpRequest, api_key: &str) -> Result<(), Error> {
    let token = request
        .headers()
        .get("X-API-KEY")
        .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
        .to_str()
        .map_err(|_| Error::Unauthorized("Invalid api key".to_string()))?;

    if token == api_key {
        Ok(())
    } else {
        Err(Error::Unauthorized("Incorrect api key".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use actix_web::{http::header::HeaderValue, test::TestRequest};

    use super::*;
    use crate::error::Error;

    #[test]
    fn parse_allowlist_empty_string_is_deny_all() {
        let allowlist = parse_allowlist("");
        assert!(allowlist.is_empty());
    }

    #[test]
    fn parse_allowlist_single_sender() {
        let allowlist = parse_allowlist("a@example.com");
        assert_eq!(allowlist, HashSet::from(["a@example.com".to_string()]));
    }

    #[test]
    fn parse_allowlist_comma_separated() {
        let allowlist = parse_allowlist("a@example.com,b@example.com");
        assert_eq!(
            allowlist,
            HashSet::from(["a@example.com".to_string(), "b@example.com".to_string()])
        );
    }

    #[test]
    fn parse_allowlist_trims_whitespace_and_drops_empty_segments() {
        let allowlist = parse_allowlist(" a@example.com , b@example.com , ,");
        assert_eq!(
            allowlist,
            HashSet::from(["a@example.com".to_string(), "b@example.com".to_string()])
        );
    }

    #[test]
    fn validate_api_key_missing_header() {
        let request = TestRequest::default().to_http_request();
        let err = validate_api_key(&request, "secret").unwrap_err();
        assert!(matches!(err, Error::Unauthorized(ref m) if m.contains("missing")));
    }

    #[test]
    fn validate_api_key_invalid_utf8() {
        let request = TestRequest::default()
            .insert_header(("X-API-KEY", HeaderValue::from_bytes(&[0xff]).unwrap()))
            .to_http_request();
        let err = validate_api_key(&request, "secret").unwrap_err();
        assert!(matches!(err, Error::Unauthorized(ref m) if m.contains("Invalid")));
    }

    #[test]
    fn validate_api_key_incorrect() {
        let request = TestRequest::default()
            .insert_header(("X-API-KEY", "wrong"))
            .to_http_request();
        let err = validate_api_key(&request, "secret").unwrap_err();
        assert!(matches!(err, Error::Unauthorized(ref m) if m.contains("Incorrect")));
    }

    #[test]
    fn validate_api_key_matching() {
        let request = TestRequest::default()
            .insert_header(("X-API-KEY", "secret"))
            .to_http_request();
        assert!(validate_api_key(&request, "secret").is_ok());
    }
}

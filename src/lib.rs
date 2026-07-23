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
pub mod mailtest;
pub mod metrics;
#[allow(dead_code, non_snake_case)]
pub mod models;

pub use config::AppConfig;
pub use mailer::MailSender;

/// Parse `ALLOWED_SENDERS` CSV. Preserves surrounding whitespace on each part.
pub fn parse_allowlist(raw: &str) -> HashSet<String> {
    raw.split(',').map(|part| part.to_string()).collect()
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

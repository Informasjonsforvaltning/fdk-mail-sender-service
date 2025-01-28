#[macro_use]
extern crate serde;

use std::{collections::HashSet, env, str::from_utf8, time::Instant};

use actix_web::{get, middleware::Logger, post, web, App, HttpRequest, HttpServer, Responder};
use lazy_static::lazy_static;
use lettre::{
    message::Mailbox,
    transport::smtp::{
        authentication::Credentials,
        extension::ClientId,
    },
    Message, SmtpTransport, Transport,
};
use crate::{
    error::Error,
    mailtest::init_mailtest,
    metrics::{get_metrics, register_metrics},
    metrics::{PROCESSED_MAIL_REQUESTS, PROCESSING_TIME},
    models::Mail,
};

mod error;
mod mailtest;
mod metrics;
#[allow(dead_code, non_snake_case)]
mod models;

lazy_static! {
    static ref API_KEY: String = env::var("API_KEY").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "API_KEY not found");
        std::process::exit(1)
    });
    static ref SMTP_HOST: String = env::var("SMTP_HOST").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "SMTP_HOST not found");
        std::process::exit(1)
    });
    static ref SMTP_PORT: u16 = env::var("SMTP_PORT")
        .map(|s| s.parse::<u16>().unwrap_or_else(|e| {
            tracing::error!(error = e.to_string().as_str(), "SMTP_PORT invalid");
            std::process::exit(1)
        }))
        .unwrap_or(587);
    static ref SMTP_USER: Option<String> = env::var("SMTP_USER").ok();
    static ref SMTP_PASSWORD: Option<String> = env::var("SMTP_PASSWORD").ok();
    static ref SMTP_EHLO_DOMAIN: Option<String> = env::var("SMTP_EHLO_DOMAIN").ok();
    static ref TLS_DOMAIN: Option<String> = env::var("TLS_DOMAIN").ok();
    static ref TLS_CERT: Option<String> = env::var("TLS_CERT").ok();
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
    validate_api_key(request)?;

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

fn validate_api_key(request: HttpRequest) -> Result<(), Error> {
    let token = request
        .headers()
        .get("X-API-KEY")
        .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
        .to_str()
        .map_err(|_| Error::Unauthorized("Invalid api key".to_string()))?;

    if token == API_KEY.clone() {
        Ok(())
    } else {
        Err(Error::Unauthorized("Incorrect api key".to_string()))
    }
}

#[derive(Clone)]
struct State {
    mailer: SmtpTransport,
    allowlist: HashSet<String>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    register_metrics();

    // Fail if API_KEY missing
    let _ = API_KEY.clone();

    let mut mail_builder = match (TLS_CERT.clone(), TLS_DOMAIN.clone()) {
        (None, None) => SmtpTransport::builder_dangerous(SMTP_HOST.as_str()),
        (Some(_), Some(_)) => SmtpTransport::builder_dangerous(SMTP_HOST.as_str()),
        _ => {
            tracing::error!("Either both or none of TLS_CERT and TLS_DOMAIN must be configured");
            std::process::exit(1);
        }
    }
    .port(SMTP_PORT.clone());

    if let Some(ehlo_domain) = SMTP_EHLO_DOMAIN.clone() {
        mail_builder = mail_builder.hello_name(ClientId::Domain(ehlo_domain));
    }
    match (SMTP_USER.clone(), SMTP_PASSWORD.clone()) {
        (Some(user), Some(password)) => {
            let credentials = Credentials::new(user, password);
            mail_builder = mail_builder.credentials(credentials);
        }
        (None, None) => {
            tracing::info!("No credentials defined, skipping auth")
        }
        _ => {
            tracing::error!(
                "Either both or none of SMTP_USER and SMTP_PASSWORD must be configured"
            );
            std::process::exit(1);
        }
    }
    let mailer = mail_builder.build();

    let allowlist = env::var("ALLOWED_SENDERS")
        .map(|s| {
            s.split(",")
                .map(|part| part.to_string())
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();
    tracing::info!("Using allowlist {:?}", allowlist);

    init_mailtest(mailer.clone());

    let state = State { mailer, allowlist };

    HttpServer::new(move || {
        App::new()
            .wrap(
                Logger::default()
                    .exclude("/livez".to_string())
                    .exclude("/readyz".to_string())
                    .log_target("http"),
            )
            .app_data(web::Data::new(state.clone()))
            .service(livez)
            .service(readyz)
            .service(metrics_endpoint)
            .service(sendmail)
    })
    .bind(("0.0.0.0", 8080))?
    .workers(4)
    .run()
    .await
}

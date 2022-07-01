#[macro_use]
extern crate serde;

use std::{collections::HashSet, env, str::from_utf8};

use actix_web::{get, middleware::Logger, post, web, App, HttpRequest, HttpServer, Responder};
use lazy_static::lazy_static;
use lettre::{
    message::Mailbox,
    transport::smtp::{
        authentication::Credentials,
        client::{Certificate, Tls, TlsParameters},
        extension::ClientId,
    },
    Message, SmtpTransport, Transport,
};

use crate::{error::Error, models::Mail};

mod error;
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

#[post("/api/sendmail")]
async fn mail(
    request: HttpRequest,
    body: web::Bytes,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

    let mail = serde_json::from_str::<Mail>(from_utf8(&body)?)?;
    let from = mail.from.parse::<Mailbox>().map_err(|e| Error::from(e))?;
    if !state.allowlist.contains(&format!("{}", from.email)) {
        return Err(Error::Unauthorized(format!(
            "Not allowed to send from '{}'",
            from.email
        )));
    }
    let to = mail.to.parse::<Mailbox>().map_err(|e| Error::from(e))?;

    let email = Message::builder()
        .from(from)
        .to(to)
        .subject(mail.subject)
        .body(mail.body)
        .map_err(|e| Error::from(e))?;

    state.mailer.send(&email).map_err(|e| Error::from(e))?;
    Ok("")
}

fn validate_api_key(request: HttpRequest) -> Result<(), Error> {
    let token = request
        .headers()
        .get("X-API-KEY")
        .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid api key".to_string()))?;

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

    // Fail if API_KEY missing
    let _ = API_KEY.clone();

    let mut mail_builder = match (TLS_CERT.clone(), TLS_DOMAIN.clone()) {
        (None, None) => SmtpTransport::starttls_relay(SMTP_HOST.as_str()).unwrap_or_else(|e| {
            tracing::error!(error = e.to_string().as_str(), "TLS error");
            std::process::exit(1);
        }),
        (Some(tls_cert), Some(tls_domain)) => {
            tracing::info!("Using specific cert with domain '{}'", tls_domain);

            let certificate = Certificate::from_pem(&tls_cert.as_bytes()).unwrap_or_else(|e| {
                tracing::error!(error = e.to_string().as_str(), "Certificate error");
                std::process::exit(1);
            });
            let tls = TlsParameters::builder(tls_domain)
                .add_root_certificate(certificate)
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!(error = e.to_string().as_str(), "TLS error");
                    std::process::exit(1);
                });

            // builder_dangerous - as we need to customize TLS certificate
            SmtpTransport::builder_dangerous(SMTP_HOST.clone()).tls(Tls::Required(tls))
        }
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
    let mailer = mail_builder.port(SMTP_PORT.clone()).build();

    let allowlist = env::var("ALLOWED_SENDERS")
        .map(|s| {
            s.split(",")
                .map(|part| part.to_string())
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();
    tracing::info!("Using allowlist {:?}", allowlist);
    let state = State { mailer, allowlist };

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(state.clone()))
            .service(livez)
            .service(readyz)
            .service(mail)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

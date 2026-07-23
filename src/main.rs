use std::sync::Arc;

use actix_web::HttpServer;

use fdk_mail_sender_service::{
    create_app, mail_health::init_mail_health, mailer::MailSender, metrics::register_metrics,
    AppConfig, State,
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    register_metrics();

    let config = AppConfig::from_env().unwrap_or_else(|e| {
        tracing::error!(error = e.as_str(), "configuration error");
        std::process::exit(1);
    });

    let mailer = config.build_smtp_transport().unwrap_or_else(|e| {
        tracing::error!(error = e.as_str(), "SMTP configuration error");
        std::process::exit(1);
    });

    tracing::info!("Using allowlist {:?}", config.allowlist);

    let mailer: Arc<dyn MailSender> = Arc::new(mailer);
    init_mail_health(mailer.clone());

    let state = State {
        mailer,
        allowlist: config.allowlist,
        api_key: config.api_key,
    };

    HttpServer::new(move || create_app(state.clone()))
        .bind(("0.0.0.0", 8080))?
        .workers(4)
        .run()
        .await
}

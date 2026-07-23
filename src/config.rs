use std::{collections::HashSet, env};

use lettre::{
    transport::smtp::{authentication::Credentials, extension::ClientId},
    SmtpTransport,
};

use crate::parse_allowlist;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub api_key: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_ehlo_domain: Option<String>,
    pub tls_domain: Option<String>,
    pub tls_cert: Option<String>,
    pub allowlist: HashSet<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_key = env::var("API_KEY").map_err(|e| format!("API_KEY not found: {e}"))?;
        let smtp_host = env::var("SMTP_HOST").map_err(|e| format!("SMTP_HOST not found: {e}"))?;
        let smtp_port = match env::var("SMTP_PORT") {
            Ok(s) => s
                .parse::<u16>()
                .map_err(|e| format!("SMTP_PORT invalid: {e}"))?,
            Err(_) => 587,
        };

        let allowlist = env::var("ALLOWED_SENDERS")
            .map(|s| parse_allowlist(&s))
            .unwrap_or_default();

        Ok(Self {
            api_key,
            smtp_host,
            smtp_port,
            smtp_user: env::var("SMTP_USER").ok(),
            smtp_password: env::var("SMTP_PASSWORD").ok(),
            smtp_ehlo_domain: env::var("SMTP_EHLO_DOMAIN").ok(),
            tls_domain: env::var("TLS_DOMAIN").ok(),
            tls_cert: env::var("TLS_CERT").ok(),
            allowlist,
        })
    }

    pub fn build_smtp_transport(&self) -> Result<SmtpTransport, String> {
        let mut mail_builder = match (&self.tls_cert, &self.tls_domain) {
            (None, None) | (Some(_), Some(_)) => {
                SmtpTransport::builder_dangerous(self.smtp_host.as_str())
            }
            _ => {
                return Err(
                    "Either both or none of TLS_CERT and TLS_DOMAIN must be configured".into(),
                );
            }
        }
        .port(self.smtp_port);

        if let Some(ehlo_domain) = &self.smtp_ehlo_domain {
            mail_builder = mail_builder.hello_name(ClientId::Domain(ehlo_domain.clone()));
        }

        match (&self.smtp_user, &self.smtp_password) {
            (Some(user), Some(password)) => {
                let credentials = Credentials::new(user.clone(), password.clone());
                mail_builder = mail_builder.credentials(credentials);
            }
            (None, None) => {
                tracing::info!("No credentials defined, skipping auth");
            }
            _ => {
                return Err(
                    "Either both or none of SMTP_USER and SMTP_PASSWORD must be configured".into(),
                );
            }
        }

        Ok(mail_builder.build())
    }
}

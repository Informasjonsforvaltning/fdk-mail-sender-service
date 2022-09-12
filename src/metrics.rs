use lazy_static::lazy_static;
use prometheus::{Encoder, Histogram, HistogramOpts, IntCounterVec, Opts, Registry};

use crate::error::Error;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref PROCESSED_MAIL_REQUESTS: IntCounterVec = IntCounterVec::new(
        Opts::new("processed_mail_requests", "Processed Mail Requests"),
        &["status"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(
            error = e.to_string(),
            "processed_mail_requests metric error"
        );
        std::process::exit(1);
    });
    pub static ref MAIL_TEST_COUNT: IntCounterVec =
        IntCounterVec::new(Opts::new("mail_test_counts", "Mail Tests"), &["status"])
            .unwrap_or_else(|e| {
                tracing::error!(error = e.to_string(), "mail_test_counts metric error");
                std::process::exit(1);
            });
    pub static ref PROCESSING_TIME: Histogram = Histogram::with_opts(HistogramOpts {
        common_opts: Opts::new("processing_time", "Mail Processing Times"),
        buckets: vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 100.0],
    })
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "processing_time");
        std::process::exit(1);
    });
}

pub fn register_metrics() {
    REGISTRY
        .register(Box::new(PROCESSED_MAIL_REQUESTS.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(
                error = e.to_string(),
                "processed_mail_requests collector error"
            );
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(MAIL_TEST_COUNT.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "mail_test_counts collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(PROCESSING_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "response_time collector error");
            std::process::exit(1);
        });
}

pub fn get_metrics() -> Result<String, Error> {
    let mut buffer = Vec::new();

    prometheus::TextEncoder::new()
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| e.to_string())?;

    let metrics = String::from_utf8(buffer).map_err(|e| e.to_string())?;
    Ok(metrics)
}

extern crate cronjob;

use std::sync::Arc;

use cronjob::CronJob;

use crate::{mailer::MailSender, metrics::UP_METRIC};

/// Probe SMTP connectivity and update the `up{service="mailserver"}` gauge.
pub fn check_mailserver(mailer: &dyn MailSender) {
    match mailer.test_connection() {
        Ok(_) => {
            UP_METRIC.with_label_values(&["mailserver"]).set(1);
            tracing::debug!("mailserver responding successfully")
        }
        Err(e) => {
            UP_METRIC.with_label_values(&["mailserver"]).set(0);
            tracing::error!(error = e.to_string(), "unable to connect to mailserver")
        }
    }
}

pub fn init_mail_health(mailer: Arc<dyn MailSender>) {
    let mut cron = CronJob::new("mail_health", move |_name: &str| {
        check_mailserver(mailer.as_ref());
    });
    cron.minutes("*");
    cron.seconds("0");
    tracing::info!("starting mail health probe");
    CronJob::start_job_threaded(cron);
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use lettre::Message;

    use super::*;
    use crate::error::Error;

    fn gauge_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct OkMailer;

    impl MailSender for OkMailer {
        fn send(&self, _message: &Message) -> Result<(), Error> {
            Ok(())
        }

        fn test_connection(&self) -> Result<bool, Error> {
            Ok(true)
        }
    }

    struct ErrMailer;

    impl MailSender for ErrMailer {
        fn send(&self, _message: &Message) -> Result<(), Error> {
            Ok(())
        }

        fn test_connection(&self) -> Result<bool, Error> {
            Err(Error::String("connection failed".to_string()))
        }
    }

    #[test]
    fn check_mailserver_sets_gauge_to_1_on_success() {
        let _guard = gauge_lock();
        check_mailserver(&OkMailer);
        assert_eq!(UP_METRIC.with_label_values(&["mailserver"]).get(), 1);
    }

    #[test]
    fn check_mailserver_sets_gauge_to_0_on_failure() {
        let _guard = gauge_lock();
        check_mailserver(&ErrMailer);
        assert_eq!(UP_METRIC.with_label_values(&["mailserver"]).get(), 0);
    }
}

extern crate cronjob;

use std::sync::Arc;

use cronjob::CronJob;

use crate::{mailer::MailSender, metrics::UP_METRIC};

pub fn init_mailtest(mailer: Arc<dyn MailSender>) {
    let mut cron = CronJob::new("mailtest", test_mail(mailer));
    cron.minutes("*");
    cron.seconds("0");
    tracing::info!("starting mailtester");
    CronJob::start_job_threaded(cron);
}

fn test_mail(mailer: Arc<dyn MailSender>) -> impl Fn(&str) {
    move |_name: &str| match mailer.test_connection() {
        Ok(_) => {
            UP_METRIC.with_label_values(&["mailserver"]).set(1);
            tracing::info!("mailserver responding successfully")
        }
        Err(e) => {
            UP_METRIC.with_label_values(&["mailserver"]).set(0);
            tracing::error!(error = e.to_string(), "unable to connect to mailserver")
        }
    }
}

extern crate cronjob;

use cronjob::CronJob;
use lettre::SmtpTransport;

use crate::metrics::MAIL_TESTS;

pub fn init_mailtest(mailer: SmtpTransport) {
    let mut cron = CronJob::new("mailtest", test_mail(mailer));
    cron.minutes("*");
    cron.seconds("0");
    tracing::info!("starting mailtester");
    CronJob::start_job_threaded(cron);
}

fn test_mail(mailer: SmtpTransport) -> impl Fn(&str) -> () {
    return move |_name: &str| match mailer.clone().test_connection() {
        Ok(_) => {
            MAIL_TESTS.with_label_values(&["success"]).inc();
            tracing::info!("mailserver responding successfully")
        }
        Err(e) => {
            MAIL_TESTS.with_label_values(&["error"]).inc();
            tracing::error!(error = e.to_string(), "unable to connect to mailserver")
        }
    };
}

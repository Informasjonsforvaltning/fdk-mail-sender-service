extern crate cronjob;

use cronjob::CronJob;
use lettre::SmtpTransport;

pub fn init_mailtest(mailer: SmtpTransport) {
    let mut cron = CronJob::new("mailtest", test_mail(mailer));
    cron.minutes("1");
    cron.start_job();
}

fn test_mail(mailer: SmtpTransport) -> impl Fn(&str) -> () {
    return move |_name: &str| match mailer.clone().test_connection() {
        Ok(_) => tracing::info!("mailserver responding successfully"),
        Err(e) => tracing::error!(error = e.to_string(), "unable to connect to mailserver"),
    };
}

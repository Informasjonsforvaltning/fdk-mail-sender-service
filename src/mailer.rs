use lettre::{Message, SmtpTransport, Transport};

/// Abstraction over SMTP send/connectivity for production and tests.
pub trait MailSender: Send + Sync {
    fn send(&self, message: &Message) -> Result<(), lettre::transport::smtp::Error>;
    fn test_connection(&self) -> Result<bool, lettre::transport::smtp::Error>;
}

impl MailSender for SmtpTransport {
    fn send(&self, message: &Message) -> Result<(), lettre::transport::smtp::Error> {
        Transport::send(self, message).map(|_| ())
    }

    fn test_connection(&self) -> Result<bool, lettre::transport::smtp::Error> {
        SmtpTransport::test_connection(self)
    }
}

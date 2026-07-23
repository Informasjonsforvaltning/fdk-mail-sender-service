use lettre::{Message, SmtpTransport, Transport};

use crate::error::Error;

/// Abstraction over SMTP send/connectivity for production and tests.
pub trait MailSender: Send + Sync {
    fn send(&self, message: &Message) -> Result<(), Error>;
    fn test_connection(&self) -> Result<bool, Error>;
}

impl MailSender for SmtpTransport {
    fn send(&self, message: &Message) -> Result<(), Error> {
        Transport::send(self, message).map(|_| ()).map_err(Error::from)
    }

    fn test_connection(&self) -> Result<bool, Error> {
        SmtpTransport::test_connection(self).map_err(Error::from)
    }
}

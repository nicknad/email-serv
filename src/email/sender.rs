use crate::config::Config;
use crate::http::error::EmailServError;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};

#[derive(Clone)]
pub struct Sender {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from_email: String,
}

impl Sender {
    pub fn new(config: &Config) -> Result<Self, EmailServError> {
        let creds = Credentials::new(config.smtp_user.clone(), config.smtp_pass.clone());

        let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
            .map_err(|e| EmailServError::EmailDeliveryFailed(format!("Invalid SMTP host: {}", e)))?
            .port(config.smtp_port)
            .credentials(creds)
            .build();

        Ok(Sender {
            mailer,
            from_email: config.email_from.clone(),
        })
    }

    pub async fn send_email(
        &self,
        to_email: &str,
        subject: &str,
        html_content: &str,
    ) -> Result<(), EmailServError> {
        let email = Message::builder()
            .from(self.from_email.parse().map_err(|e| {
                EmailServError::EmailDeliveryFailed(format!("Invalid FROM email: {}", e))
            })?)
            .to(to_email.parse().map_err(|e| {
                EmailServError::EmailDeliveryFailed(format!("Invalid TO email: {}", e))
            })?)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(String::from(html_content))
            .map_err(|e| {
                EmailServError::EmailDeliveryFailed(format!("Failed to build email: {}", e))
            })?;

        self.mailer.send(email).await.map_err(|e| {
            EmailServError::EmailDeliveryFailed(format!("Failed to send email: {}", e))
        })?;

        Ok(())
    }
}

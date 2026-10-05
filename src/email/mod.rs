use crate::config::Config;
use crate::http::error::EmailServError;
use std::sync::Arc;
use tera::{Context, Tera};

pub mod sender;

#[derive(Clone)]
pub struct EmailService {
    tera: Arc<Tera>,
    sender: Arc<sender::Sender>,
}

impl EmailService {
    pub fn new(config: &Config) -> Result<Self, EmailServError> {
        let mut tera = Tera::new("templates/**/*.html").map_err(|e| {
            EmailServError::TemplateError(format!("Tera initialization failed: {}", e))
        })?;

        // Add a global function for the current year
        tera.register_function(
            "now",
            |args: &std::collections::HashMap<String, serde_json::Value>| {
                let format = args.get("format").and_then(|f| f.as_str()).unwrap_or("%Y");
                Ok(serde_json::Value::String(
                    chrono::Utc::now().format(format).to_string(),
                ))
            },
        );

        let sender = sender::Sender::new(config)?;

        Ok(EmailService {
            tera: Arc::new(tera),
            sender: Arc::new(sender),
        })
    }

    pub fn render_template<T: serde::Serialize>(
        &self,
        template_path: &str,
        context_data: &T,
    ) -> Result<String, EmailServError> {
        let context = Context::from_serialize(context_data).map_err(|e| {
            EmailServError::TemplateError(format!("Context serialization failed: {}", e))
        })?;

        self.tera.render(template_path, &context).map_err(|e| {
            tracing::error!("Template rendering error for '{}': {:?}", template_path, e);
            EmailServError::TemplateError(format!(
                "Template rendering failed for '{}': {}",
                template_path, e
            ))
        })
    }

    /// Whether a template with this path is loaded (used to reject admin
    /// requests for templates that do not exist before touching the queue).
    pub fn template_exists(&self, template_path: &str) -> bool {
        self.tera
            .get_template_names()
            .any(|name| name == template_path)
    }

    /// Loads the subject line that belongs to a body template.
    ///
    /// Convention (used by welcome/ and newsletters/):
    /// - `foo/body.html`            -> `foo/subject.txt`
    /// - `foo/verification_body.html` -> `foo/verification_subject.txt`
    pub fn load_subject(&self, template_path: &str) -> Result<String, EmailServError> {
        let subject_path = if let Some(prefix) = template_path.strip_suffix("_body.html") {
            format!("templates/{}_subject.txt", prefix)
        } else if let Some(prefix) = template_path.strip_suffix("body.html") {
            format!("templates/{}subject.txt", prefix)
        } else {
            format!("templates/{}", template_path.replace(".html", ".txt"))
        };

        std::fs::read_to_string(&subject_path)
            .map(|subject| subject.trim().to_string())
            .map_err(|e| {
                EmailServError::TemplateError(format!(
                    "Failed to read subject file '{}': {}",
                    subject_path, e
                ))
            })
    }

    pub async fn send_email(
        &self,
        to_email: &str,
        subject: &str,
        html_content: &str,
    ) -> Result<(), EmailServError> {
        self.sender
            .send_email(to_email, subject, html_content)
            .await
    }
}

//! Shared SMTP email helper used by all CLI binaries.
//!
//! Configuration is read entirely from environment variables so that
//! credentials never appear as command-line arguments (which are visible
//! in the process table and shell history).
//!
//! Variables:
//!   SMTP_HOST         mail server hostname          (default: localhost)
//!   SMTP_PORT         port number                   (default: 587)
//!   SMTP_TLS          "starttls" | "tls" | "none"   (default: starttls)
//!   SMTP_USERNAME     login username                 (optional)
//!   SMTP_PASSWORD     login password                 (optional)
//!   SMTP_FROM         envelope / From address        (default: noreply@wasd.dk)

use std::env;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// "starttls" | "tls" | "none"
    pub tls: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
}

impl SmtpConfig {
    /// Build from the standard SMTP_* environment variables.
    pub fn from_env() -> Self {
        SmtpConfig {
            host: env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("SMTP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(587),
            tls: env::var("SMTP_TLS").unwrap_or_else(|_| "starttls".to_string()),
            username: env::var("SMTP_USERNAME").ok(),
            password: env::var("SMTP_PASSWORD").ok(),
            from: env::var("SMTP_FROM").unwrap_or_else(|_| "noreply@wasd.dk".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Delivery
// ---------------------------------------------------------------------------

/// Send a multipart (plain-text + HTML) email.
///
/// # Errors
/// Returns an error if the message cannot be built or if delivery fails.
pub fn send_email(
    cfg: &SmtpConfig,
    to_addr: &str,
    subject: &str,
    plain: &str,
    html_body: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    use lettre::message::{MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let message = Message::builder()
        .from(cfg.from.parse()?)
        .to(to_addr.parse()?)
        .subject(subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(plain.to_string()))
                .singlepart(SinglePart::html(html_body.to_string())),
        )?;

    // "tls"      → implicit TLS / SMTPS  (typically port 465)
    // "none"     → plain SMTP, no TLS    (e.g. local relay on port 25)
    // "starttls" → STARTTLS upgrade       (default, typically port 587)
    let mut builder = match cfg.tls.as_str() {
        "tls" => SmtpTransport::relay(&cfg.host)?.port(cfg.port),
        "none" => SmtpTransport::builder_dangerous(&cfg.host).port(cfg.port),
        _ => SmtpTransport::starttls_relay(&cfg.host)?.port(cfg.port),
    };

    if let (Some(u), Some(p)) = (cfg.username.as_deref(), cfg.password.as_deref()) {
        builder = builder.credentials(Credentials::new(u.to_string(), p.to_string()));
    }

    builder.build().send(&message)?;
    Ok(())
}

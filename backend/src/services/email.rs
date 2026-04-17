//! SMTP email sender (lettre). Used for account verification (double opt-in).
use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::env;

pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub public_base_url: String,
}

impl EmailConfig {
    pub fn from_env() -> Option<Self> {
        let host = env::var("SMTP_HOST").ok()?;
        let port = env::var("SMTP_PORT").ok()?.parse().ok()?;
        let username = env::var("SMTP_USER").ok()?;
        let password = env::var("SMTP_PASS").ok()?;
        let from = env::var("SMTP_FROM").unwrap_or_else(|_| username.clone());
        let public_base_url = env::var("PUBLIC_BASE_URL").unwrap_or_else(|_| "https://justyeet.it".into());
        Some(Self { host, port, username, password, from, public_base_url })
    }
}

fn transport(cfg: &EmailConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, lettre::transport::smtp::Error> {
    let creds = Credentials::new(cfg.username.clone(), cfg.password.clone());
    AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)
        .map(|b| b.port(cfg.port).credentials(creds).build())
}

pub async fn send_verification_email(
    cfg: &EmailConfig,
    to_email: &str,
    token: &str,
) -> anyhow::Result<()> {
    let verify_url = format!("{}/?verify={}", cfg.public_base_url.trim_end_matches('/'), token);
    let from: Mailbox = format!("YEET Social <{}>", cfg.from).parse()?;
    let to: Mailbox = to_email.parse()?;

    let html = format!(
        r#"<!DOCTYPE html><html><body style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#0a0a0a;color:#fff;margin:0;padding:24px">
<div style="max-width:520px;margin:0 auto;background:#16181c;border:1px solid #2a2a2a;border-radius:16px;padding:32px">
<h1 style="color:#c6f135;margin:0 0 16px;font-size:22px">Welcome to YEET Social</h1>
<p style="color:#e0e0e0;line-height:1.6;font-size:15px">Please confirm your email address to activate your account.</p>
<p style="margin:24px 0"><a href="{url}" style="display:inline-block;background:#c6f135;color:#000;padding:12px 28px;border-radius:24px;text-decoration:none;font-weight:700">Confirm Email</a></p>
<p style="color:#888;font-size:13px">Or copy this link: <span style="word-break:break-all;color:#c6f135">{url}</span></p>
<p style="color:#666;font-size:12px;margin-top:24px">This link expires in 24 hours. If you didn't sign up for YEET, ignore this email.</p>
</div></body></html>"#,
        url = verify_url
    );

    let email = Message::builder()
        .from(from)
        .to(to)
        .subject("Confirm your YEET Social account")
        .header(ContentType::TEXT_HTML)
        .body(html)?;

    let mailer = transport(cfg)?;
    mailer.send(email).await?;
    Ok(())
}

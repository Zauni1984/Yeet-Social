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
    // Pick the right TLS mode for the port. `relay()` is *implicit* TLS
    // (SMTPS, port 465); `starttls_relay()` upgrades a plaintext 587
    // connection to TLS. Using relay() on 587 (the most common submission
    // port) fails the handshake, so no mail is ever sent — which looks like
    // "email verification is broken". SMTP_STARTTLS can force either mode.
    let use_starttls = match env::var("SMTP_STARTTLS").ok().as_deref() {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        None => cfg.port == 587, // sensible default: 587 → STARTTLS, else implicit TLS
    };
    let builder = if use_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?
    };
    Ok(builder.port(cfg.port).credentials(creds).build())
}

pub async fn send_verification_email(
    cfg: &EmailConfig,
    to_email: &str,
    token: &str,
) -> anyhow::Result<()> {
    // Server-side GET verification: the link hits the backend directly and
    // verifies without needing the SPA to load/run JS (the old /?verify=
    // client-only flow was fragile in some mail clients/in-app browsers).
    let verify_url = format!("{}/api/v1/auth/email-verify?token={}", cfg.public_base_url.trim_end_matches('/'), token);
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

/// F7 — confirmation of a pay-per-view purchase on a durable medium
/// (§ 312f BGB): restates the consent the buyer gave (immediate performance,
/// loss of the right of withdrawal), the price and the time.
pub async fn send_ppv_confirmation(
    cfg: &EmailConfig,
    to_email: &str,
    lang: &str,
    post_id: uuid::Uuid,
    price_yeet: f64,
    consent_at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<()> {
    let from: Mailbox = format!("YEET Social <{}>", cfg.from).parse()?;
    let to: Mailbox = to_email.parse()?;
    let when = consent_at.format("%Y-%m-%d %H:%M UTC").to_string();
    let base = cfg.public_base_url.trim_end_matches('/');
    let (subject, title, intro, consent, details, footer) = if lang == "de" {
        ("Bestätigung Ihrer Pay-per-View-Freischaltung – YEET Social",
         "Bestätigung Ihrer Pay-per-View-Freischaltung",
         "Sie haben soeben einen Pay-per-View-Inhalt auf YEET Social freigeschaltet. Diese E-Mail bestätigt Ihren Kauf und die von Ihnen erteilte Zustimmung auf einem dauerhaften Datenträger.",
         "Sie haben ausdrücklich verlangt, dass wir sofort – vor Ablauf der 14-tägigen Widerrufsfrist – mit der Bereitstellung des digitalen Inhalts beginnen, und zur Kenntnis genommen, dass Ihr Widerrufsrecht damit erlischt, sobald der Inhalt bereitgestellt wurde (§ 356 Abs. 5 BGB).",
         "Preis: {price} YEET (davon 90 % an den Creator, 10 % Plattformgebühr), abgebucht von Ihrem Punkteguthaben. Zeitpunkt der Zustimmung: {when}. Post-ID: {post}.",
         "Diesen Beleg finden Sie auch in Ihrem Konto unter „Token Tips → Pay-per-View-Käufe“. Fragen: info@blocksocial.eu. Nutzungsbedingungen: {base}/legal/terms")
    } else {
        ("Confirmation of your pay-per-view unlock – YEET Social",
         "Confirmation of your pay-per-view unlock",
         "You have just unlocked pay-per-view content on YEET Social. This email confirms your purchase and the consent you gave, on a durable medium.",
         "You expressly requested that we begin supplying the digital content immediately, before the 14-day withdrawal period has expired, and acknowledged that you thereby lose your right of withdrawal once the content has been made available (§ 356 (5) BGB / Art. 16 (m) Directive 2011/83/EU).",
         "Price: {price} YEET (90 % to the creator, 10 % platform fee), debited from your points balance. Time of consent: {when}. Post ID: {post}.",
         "You can also find this receipt in your account under “Token Tips → Pay-per-View purchases”. Questions: info@blocksocial.eu. Terms of Service: {base}/legal/terms")
    };
    let details = details.replace("{price}", &format!("{price_yeet}")).replace("{when}", &when).replace("{post}", &post_id.to_string());
    let footer = footer.replace("{base}", base);
    let html = format!(
        r#"<!DOCTYPE html><html><body style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#0a0a0a;color:#fff;margin:0;padding:24px">
<div style="max-width:560px;margin:0 auto;background:#16181c;border:1px solid #2a2a2a;border-radius:16px;padding:32px">
<h1 style="color:#c6f135;margin:0 0 16px;font-size:20px">{title}</h1>
<p style="color:#e0e0e0;line-height:1.6;font-size:15px">{intro}</p>
<p style="color:#e0e0e0;line-height:1.6;font-size:14px;border-left:3px solid #c6f135;padding-left:12px">{consent}</p>
<p style="color:#e0e0e0;line-height:1.6;font-size:14px">{details}</p>
<p style="color:#888;font-size:12px;margin-top:24px">{footer}</p>
</div></body></html>"#
    );
    let msg = Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html)?;
    transport(cfg)?.send(msg).await?;
    Ok(())
}

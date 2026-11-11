//! POST /api/v1/csp-report — sink for `Content-Security-Policy` violation
//! reports (audit FE-H1; enforced since 11 Nov, report-only before). Reports are logged at
//! warn level (target `yeet_server::api::csp`) so the policy can be tuned
//! before it is enforced; nothing is stored. Rate-limited per client IP and
//! capped at 16 KB by the route layer. Accepts both the legacy `report-uri`
//! shape (`{"csp-report": {…}}`) and the Reporting API array shape.
use axum::{body::Bytes, extract::State, http::{HeaderMap, StatusCode}};
use crate::AppState;

fn field(r: &serde_json::Value, a: &str, b: &str) -> String {
    let v = r.get(a).or_else(|| r.get(b));
    let s = match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    s.chars().take(300).collect()
}

pub async fn report(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let ip = crate::api::middleware::client_ip(&headers);
    if crate::api::middleware::limit(&state, "csp_report", &ip, 60, 20, 3600, 200).await.is_err() {
        return StatusCode::NO_CONTENT;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::NO_CONTENT;
    };
    let reports: Vec<&serde_json::Value> = if let Some(r) = v.get("csp-report") {
        vec![r]
    } else if let Some(arr) = v.as_array() {
        arr.iter().filter_map(|e| e.get("body")).collect()
    } else {
        vec![&v]
    };
    for r in reports.into_iter().take(5) {
        tracing::warn!(
            directive = %field(r, "violated-directive", "effectiveDirective"),
            blocked   = %field(r, "blocked-uri", "blockedURL"),
            document  = %field(r, "document-uri", "documentURL"),
            source    = %field(r, "source-file", "sourceFile"),
            line      = %field(r, "line-number", "lineNumber"),
            "CSP violation"
        );
    }
    StatusCode::NO_CONTENT
}

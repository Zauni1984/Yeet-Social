//! Blockfrost authentication helpers.
//!
//! Every request to Blockfrost requires a `project_id` header. This module
//! produces the header tuple slice consumed by
//! [`HttpClient::get_with_headers`] / [`post_json_with_headers`].

/// Build the auth-header slice for a Blockfrost request.
///
/// Returns an empty `Vec` if no `project_id` is configured. Callers that
/// pass the result to `*_with_headers` will then make an unauthenticated
/// request and receive a 403 from Blockfrost — the failure mode is loud
/// and observable rather than silent.
#[must_use]
pub(crate) fn project_id_headers(project_id: Option<&str>) -> Vec<(&str, &str)> {
    match project_id {
        Some(id) if !id.is_empty() => vec![("project_id", id)],
        _ => Vec::new(),
    }
}

// Rust guideline compliant 2026-05-02

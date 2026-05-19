//! URL path-segment encoding helpers.
//!
//! Chain crates interpolate user-derived addresses directly into REST
//! paths (`/addresses/{addr}/transactions`, etc.). All current address
//! formats are alphanumeric or alphanumeric-plus-colon, so the
//! interpolation works today, but a future address scheme that includes
//! `/`, `?`, `#`, or other URL-significant characters would silently
//! corrupt the request URL.
//!
//! [`encode_segment`] runs the input through a conservative ASCII-set
//! filter so the output is always safe to drop into a URL path
//! component without breaking the URL grammar.

use std::borrow::Cow;

use percent_encoding::AsciiSet;

/// ASCII set used for URL path-segment encoding.
///
/// We use [`percent_encoding::NON_ALPHANUMERIC`] — a strict superset of
/// the RFC 3986 path-segment reserved set. This over-encodes a handful
/// of safe characters (`-`, `_`, `.`, `~`) but the cost is one or two
/// extra bytes per address; the benefit is that nothing the chain
/// returns can ever break out of its path component. Receiving servers
/// always decode percent-escapes back, so over-encoding is functionally
/// transparent.
pub const PATH_SEGMENT: &AsciiSet = percent_encoding::NON_ALPHANUMERIC;

/// Percent-encode `s` as a single URL path segment.
///
/// Returns a borrowed `Cow` when no encoding was needed (the common case
/// for alphanumeric addresses), so callers don't pay an allocation when
/// the input is already safe.
///
/// # Examples
/// ```
/// use dontyeet_network::path::encode_segment;
/// assert_eq!(encode_segment("addr1qx2fxv2"), "addr1qx2fxv2");
/// assert_eq!(encode_segment("kaspa:abc"), "kaspa%3Aabc");
/// assert_eq!(encode_segment("../etc/passwd"), "%2E%2E%2Fetc%2Fpasswd");
/// ```
#[must_use]
pub fn encode_segment(s: &str) -> Cow<'_, str> {
    percent_encoding::utf8_percent_encode(s, PATH_SEGMENT).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphanumeric_is_unchanged() {
        let addr = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zp30";
        let out = encode_segment(addr);
        assert_eq!(out, addr);
    }

    #[test]
    fn colon_is_encoded() {
        // Kaspa-style colon-prefixed addresses.
        let out = encode_segment("kaspa:qpzry9x8gf2tvdw0s3jn54khce6mua7l");
        assert!(out.contains("%3A"), "got: {out}");
    }

    #[test]
    fn slash_and_question_mark_are_encoded() {
        // Hypothetical malicious or buggy address — must never break out
        // of the path component into query or hierarchy.
        let out = encode_segment("evil/../../?injected#frag");
        assert!(!out.contains('/'));
        assert!(!out.contains('?'));
        assert!(!out.contains('#'));
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(encode_segment(""), "");
    }

    #[test]
    fn unicode_is_encoded_as_utf8_percent() {
        let out = encode_segment("café");
        // The 'é' is U+00E9 = 0xC3 0xA9 in UTF-8.
        assert!(out.contains("%C3%A9"), "got: {out}");
    }
}

// Rust guideline compliant 2026-05-02

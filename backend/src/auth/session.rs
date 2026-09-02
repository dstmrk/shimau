//! Session tokens and the cookie that carries them (spec §7.1).
//!
//! The database stores the SHA-256 of the token, never the token itself: a
//! leaked `shimau.db` then does not hand out live sessions.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest, Sha256};

pub const COOKIE_NAME: &str = "shimau_session";
const TOKEN_BYTES: usize = 32;

/// Generates a fresh session token from the operating system CSPRNG.
pub fn generate_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The value stored in the `sessions` table.
pub fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// `Set-Cookie` value that establishes a session.
///
/// `SameSite=Lax` is the CSRF gate: a cross-site form POST does not carry the
/// cookie, and every mutating endpoint takes a JSON body, which an HTML form
/// cannot produce. `HttpOnly` keeps the token out of reach of any script on
/// the page.
pub fn set_cookie(token: &str, max_age_secs: i64, secure: bool) -> String {
    let mut cookie =
        format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// `Set-Cookie` value that clears the session.
pub fn clear_cookie(secure: bool) -> String {
    let mut cookie = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Extracts the session token from a `Cookie` header value.
pub fn token_from_cookie_header(header: &str) -> Option<&str> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == COOKIE_NAME).then(|| value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_url_safe() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(
            a.len() >= 43,
            "32 random bytes should not encode this short"
        );
    }

    #[test]
    fn the_stored_hash_is_not_the_token() {
        let token = generate_token().unwrap();
        let hash = token_hash(&token);
        assert_ne!(hash, token);
        assert_eq!(hash, token_hash(&token), "hashing must be deterministic");
    }

    #[test]
    fn the_session_cookie_is_httponly_and_lax() {
        let cookie = set_cookie("abc", 3600, true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=3600"));
        assert!(cookie.starts_with("shimau_session=abc;"));
    }

    #[test]
    fn secure_is_omitted_for_plain_http_installs() {
        assert!(!set_cookie("abc", 3600, false).contains("Secure"));
        assert!(!clear_cookie(false).contains("Secure"));
    }

    #[test]
    fn clear_cookie_expires_immediately() {
        assert!(clear_cookie(true).contains("Max-Age=0"));
    }

    #[test]
    fn parses_the_token_out_of_a_cookie_header() {
        assert_eq!(
            token_from_cookie_header("theme=dark; shimau_session=tok123; other=1"),
            Some("tok123")
        );
        assert_eq!(token_from_cookie_header("shimau_session=tok"), Some("tok"));
        assert_eq!(token_from_cookie_header("theme=dark"), None);
        assert_eq!(token_from_cookie_header(""), None);
    }

    #[test]
    fn a_cookie_named_like_a_prefix_is_not_matched() {
        assert_eq!(token_from_cookie_header("shimau_session_x=tok"), None);
    }
}

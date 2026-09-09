//! API tokens: the credential a machine client presents.
//!
//! A browser gets a session cookie; anything that is not a browser gets one of
//! these, in an `Authorization: Bearer` header. They are deliberately separate
//! credentials. A cookie is sent by the browser automatically, which is what
//! `SameSite=Lax` exists to contain, and handing the same value to a script
//! would mean the administrator's own login is what leaks when the script
//! does. A token is presented explicitly, carries less authority than the
//! session, and can be revoked on its own.
//!
//! The database stores the SHA-256 of the token, exactly as it does for a
//! session, and for the same reason: a leaked `shimau.db` must not hand out
//! live credentials. Argon2id would be the wrong tool here even though it is
//! the right one for the password. A password is low-entropy and chosen by a
//! human, so the cost of hashing is what stands between a stolen hash and a
//! dictionary. A token is 256 bits from the operating system CSPRNG, where
//! there is nothing to guess and the tens of milliseconds would instead be
//! paid on every single request a machine client makes.

use super::session;

/// Prefix on every token. It costs seven characters and buys two things: a
/// value found in a log or a config file is recognisably shimau's, and secret
/// scanners have something to match on.
pub const TOKEN_PREFIX: &str = "shimau_";

const TOKEN_BYTES: usize = 32;

/// Longest accepted label. A label is a note to the administrator about which
/// client holds the token, not a description.
pub const MAX_LABEL_LEN: usize = 64;

/// Generates a fresh token from the operating system CSPRNG.
pub fn generate() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)?;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    Ok(format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

/// The value stored in the `api_tokens` table.
///
/// Deliberately the same construction as a session token's hash: both are
/// high-entropy random strings, so both want a fast digest rather than a
/// password hash.
pub fn hash(token: &str) -> String {
    session::token_hash(token)
}

/// Extracts the token from an `Authorization` header value.
///
/// The scheme is matched case-insensitively because RFC 9110 says it is
/// case-insensitive, and clients disagree in practice: `Bearer` and `bearer`
/// both appear in the wild.
pub fn from_authorization_header(header: &str) -> Option<&str> {
    let (scheme, value) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = value.trim();
    (!token.is_empty()).then_some(token)
}

/// Trims and checks a label supplied when creating a token.
pub fn normalise_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_LABEL_LEN {
        return None;
    }
    // A label is echoed back into the token list. Control characters have no
    // business there, and a newline in a label is a line in the audit log that
    // did not come from the log.
    if trimmed.chars().any(char::is_control) {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_prefixed_and_url_safe() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(TOKEN_PREFIX));
        let body = a.strip_prefix(TOKEN_PREFIX).unwrap();
        assert!(body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(
            body.len() >= 43,
            "32 random bytes should not encode this short"
        );
    }

    #[test]
    fn the_stored_hash_is_not_the_token() {
        let token = generate().unwrap();
        let stored = hash(&token);
        assert_ne!(stored, token);
        assert!(!stored.contains(token.strip_prefix(TOKEN_PREFIX).unwrap()));
        assert_eq!(stored, hash(&token), "hashing must be deterministic");
    }

    #[test]
    fn a_bearer_header_yields_its_token() {
        assert_eq!(
            from_authorization_header("Bearer shimau_abc"),
            Some("shimau_abc")
        );
        assert_eq!(
            from_authorization_header("bearer shimau_abc"),
            Some("shimau_abc")
        );
        assert_eq!(
            from_authorization_header("BEARER  shimau_abc  "),
            Some("shimau_abc")
        );
    }

    #[test]
    fn other_schemes_and_malformed_headers_yield_nothing() {
        assert_eq!(from_authorization_header("Basic abc"), None);
        assert_eq!(from_authorization_header("shimau_abc"), None);
        assert_eq!(from_authorization_header("Bearer"), None);
        assert_eq!(from_authorization_header("Bearer "), None);
        assert_eq!(from_authorization_header(""), None);
    }

    #[test]
    fn labels_are_trimmed_and_bounded() {
        assert_eq!(normalise_label("  laptop "), Some("laptop".to_string()));
        assert_eq!(normalise_label(""), None);
        assert_eq!(normalise_label("   "), None);
        assert_eq!(
            normalise_label(&"x".repeat(MAX_LABEL_LEN)).unwrap().len(),
            MAX_LABEL_LEN
        );
        assert_eq!(normalise_label(&"x".repeat(MAX_LABEL_LEN + 1)), None);
    }

    #[test]
    fn a_label_cannot_carry_a_control_character() {
        assert_eq!(normalise_label("ci\nrunner"), None);
        assert_eq!(normalise_label("ci\treal"), None);
        assert_eq!(normalise_label("ci\u{0}"), None);
    }
}

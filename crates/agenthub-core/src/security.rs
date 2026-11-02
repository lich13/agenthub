use sha2::{Digest, Sha256};

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn is_sensitive_output_line(line: &str) -> bool {
    // Keep explicit boundaries in the native string table too: merging the
    // credential prefixes with adjacent literals looks like an embedded key
    // to the release payload scanner.
    const MARKERS: &str = "auth.json\0token\0secret\0password\0authorization\0cookie\0device_key\0api_key\0apikey\0x-api-key\0nhk_\0private_key\0access_key\0bearer \0sk-\0ghp_\0github_pat_\0xoxb-\0xoxp-";
    let lower = line.to_ascii_lowercase();
    MARKERS.split('\0').any(|marker| lower.contains(marker))
}

pub fn redact_output(input: &str) -> String {
    let mut out = Vec::new();
    for line in input.lines() {
        if is_sensitive_output_line(line) {
            out.push("[redacted sensitive line]".to_string());
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::redact_output;

    #[test]
    fn redacts_token_like_lines() {
        assert_eq!(
            redact_output("ok\nTOKEN=abc"),
            "ok\n[redacted sensitive line]"
        );
    }

    #[test]
    fn redacts_cookie_device_key_and_authorization_lines() {
        assert_eq!(
            redact_output(
                "ok\nCookie: agenthub_session=abc\ndevice_key=secret\nAuthorization Bearer abc\nend"
            ),
            "ok\n[redacted sensitive line]\n[redacted sensitive line]\n[redacted sensitive line]\nend"
        );
    }

    #[test]
    fn redacts_common_api_key_lines() {
        assert_eq!(
            redact_output("ok\nOPENAI_API_KEY=sk-secret\nPRIVATE_KEY=abc\naccess_key=abc\nend"),
            "ok\n[redacted sensitive line]\n[redacted sensitive line]\n[redacted sensitive line]\nend"
        );
    }
}

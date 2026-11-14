use sha2::{Digest, Sha256};

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn is_sensitive_output_line(line: &str) -> bool {
    fn word(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
    }
    fn credential_key(key: &str) -> bool {
        const KEYS: &str = "token\0secret\0password\0authorization\0cookie\0set-cookie\0device_key\0api_key\0api-key\0apikey\0x-api-key\0private_key\0access_key";
        KEYS.split('\0').any(|name| {
            key == name
                || key
                    .strip_suffix(name)
                    .is_some_and(|prefix| prefix.ends_with('_'))
        })
    }
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    if lower.contains("-----begin ") && lower.contains("private key-----") {
        return true;
    }
    let mut start = 0;
    while start < bytes.len() {
        if !word(bytes[start]) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < bytes.len() && word(bytes[end]) {
            end += 1;
        }
        let token = &lower[start..end];
        // Prefixes must start a token and carry a plausible key payload.
        const PREFIXES: &str = "nhk_\0sk-\0ghp_\0github_pat_\0xoxb-\0xoxp-";
        if PREFIXES.split('\0').any(|prefix| {
            token
                .strip_prefix(prefix)
                .is_some_and(|value| value.len() >= 16)
        }) {
            return true;
        }
        let rest = lower[end..].trim_start_matches([' ', '\t', '\'', '"']);
        if credential_key(token.trim_start_matches("--"))
            && (rest
                .strip_prefix(['=', ':'])
                .is_some_and(|value| !value.trim().is_empty())
                || token.starts_with("--") && !rest.is_empty())
        {
            return true;
        }
        if token == "bearer"
            && end < bytes.len()
            && bytes[end].is_ascii_whitespace()
            && !lower[end..].trim().is_empty()
        {
            return true;
        }
        start = end;
    }
    false
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
    fn preserves_technical_words_and_redacts_actual_values() {
        let text = "task-1 disk-space risk-level ask-user mask-value flask-app\nmax_tokens=100 token_count=2 tokenizer secret handling auth.json";
        assert_eq!(redact_output(text), text);
        for line in [
            format!("credential: sk-{}", "a".repeat(24)),
            format!("credential: ghp_{}", "b".repeat(36)),
            r#"{"access_token":"fixture-value"}"#.into(),
            "--api-key=fixture".into(),
            "OPENAI_API_KEY = fixture".into(),
            "-----BEGIN PRIVATE KEY-----".into(),
        ] {
            assert_eq!(redact_output(&line), "[redacted sensitive line]");
        }
    }

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

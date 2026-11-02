//! Shell text is retained for hook/configuration display, never for job execution.
#[derive(Clone, Copy)]
pub enum QuoteStyle {
    Always,
    Minimal,
    MinimalWithEquals,
}

pub fn shell_quote(value: &str, style: QuoteStyle) -> String {
    let safe = !matches!(style, QuoteStyle::Always)
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '/' | '.' | '_' | '-' | ':')
                || (ch == '=' && matches!(style, QuoteStyle::MinimalWithEquals))
        });
    if safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_all_existing_quoting_policies() {
        for style in [
            QuoteStyle::Always,
            QuoteStyle::Minimal,
            QuoteStyle::MinimalWithEquals,
        ] {
            assert_eq!(shell_quote("a b'c", style), "'a b'\\''c'");
            assert_eq!(shell_quote("$(fixture);\n", style), "'$(fixture);\n'");
        }
        assert_eq!(shell_quote("", QuoteStyle::Always), "''");
        assert_eq!(shell_quote("", QuoteStyle::Minimal), "");
        assert_eq!(shell_quote("a=b", QuoteStyle::Minimal), "'a=b'");
        assert_eq!(shell_quote("a=b", QuoteStyle::MinimalWithEquals), "a=b");
        assert_eq!(
            shell_quote("/a-b_c.d:1", QuoteStyle::Always),
            "'/a-b_c.d:1'"
        );
        assert_eq!(shell_quote("/a-b_c.d:1", QuoteStyle::Minimal), "/a-b_c.d:1");
    }
}

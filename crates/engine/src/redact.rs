use regex::Regex;
use std::sync::LazyLock;

static ACCESS_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)accessToken=[^\s]+").expect("accessToken regex"));
static JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").expect("jwt regex")
});

pub fn redact_line(line: &str) -> String {
    redact_line_with_tokens(line, &[])
}

pub fn redact_line_with_tokens(line: &str, tokens: &[String]) -> String {
    let mut out = line.to_string();
    let mut exact: Vec<&str> = tokens
        .iter()
        .map(String::as_str)
        .filter(|t| !t.is_empty())
        .collect();
    exact.sort_by_key(|t| std::cmp::Reverse(t.len()));
    for token in exact {
        out = out.replace(token, "[redacted]");
    }
    let out = ACCESS_TOKEN.replace_all(&out, "accessToken=[redacted]");
    JWT.replace_all(&out, "[redacted]").into_owned()
}

#[cfg(test)]
mod tests {
    use super::redact_line;

    #[test]
    fn redact_access_token_query() {
        assert_eq!(
            redact_line("foo accessToken=sekrit bar"),
            "foo accessToken=[redacted] bar"
        );
    }

    #[test]
    fn redact_jwt() {
        let line = "token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.aaa.bbb";
        assert!(!redact_line(line).contains("eyJ"));
    }
}

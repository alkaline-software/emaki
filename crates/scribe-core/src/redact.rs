//! Secret scrubbing. A safety net, not a guarantee: it catches the shapes that
//! leak most often (provider key prefixes, `Authorization` headers, `KEY=value`
//! assignments) and says so plainly rather than implying completeness.

use std::sync::LazyLock;

use regex::Regex;

pub const MASK: &str = "[redacted]";

enum Rule {
    Plain(Regex),
    /// Keep the first capture group, mask the rest.
    KeepPrefix(Regex),
    /// The generic assignment shape; skips obviously innocuous values.
    Assignment(Regex),
}

static BUILTIN: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let plain = |p: &str| Rule::Plain(Regex::new(p).unwrap());
    vec![
        plain(r"sk-ant-[A-Za-z0-9_\-]{20,}"),
        plain(r"sk-[A-Za-z0-9]{32,}"),
        plain(r"gh[pousr]_[A-Za-z0-9]{16,}"),
        plain(r"github_pat_[A-Za-z0-9_]{20,}"),
        plain(r"xox[abposr]-[A-Za-z0-9-]{10,}"),
        plain(r"AKIA[0-9A-Z]{16}"),
        plain(r"AIza[0-9A-Za-z_\-]{35}"),
        plain(r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}"),
        plain(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"),
        Rule::KeepPrefix(Regex::new(r#"(?i)(authorization\s*:\s*(?:bearer|basic|token)\s+)[^\s"']{8,}"#).unwrap()),
        Rule::KeepPrefix(Regex::new(r#"(?i)(x-api-key\s*:\s*)[^\s"']{8,}"#).unwrap()),
        Rule::Assignment(
            Regex::new(
                r#"(?i)([A-Za-z0-9_.\-]{0,32}?(?:api[_\-]?key|secret[_\-]?key|access[_\-]?token|auth[_\-]?token|client[_\-]?secret|passwd|password|secret|token)[A-Za-z0-9_.\-]{0,32}?\s*[=:]\s*["']?)([^\s"',;}]{6,})"#,
            )
            .unwrap(),
        ),
    ]
});

static TRIGGER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)sk-|gh[pousr]_|github_pat_|xox|AKIA|AIza|eyJ|BEGIN [A-Z ]*PRIVATE|authoriz|x-api-key|key|secret|token|passw").unwrap()
});

static INNOCUOUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:true|false|null|none|undefined|yes|no|\$\{?[A-Za-z_][A-Za-z0-9_]*\}?|<[^>]*>|\[[^\]]*\]|\.\.\.|x{3,}|\*{3,}|\.{3,}|your[_\-].*|my[_\-].*|some[_\-].*|example.*|placeholder.*|changeme.*|\[redacted\])$",
    )
    .unwrap()
});

pub struct Redactor {
    enabled: bool,
    custom: Vec<Regex>,
}

impl Redactor {
    pub fn new(enabled: bool, extra_patterns: &[String]) -> Self {
        let custom = extra_patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();
        Self { enabled, custom }
    }

    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self::new(cfg.redact.enabled, &cfg.redact.extra_patterns)
    }

    pub fn off() -> Self {
        Self { enabled: false, custom: Vec::new() }
    }

    pub fn scrub(&self, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        if self.custom.is_empty() && !TRIGGER.is_match(text) {
            return text.to_string();
        }
        let mut out = text.to_string();
        for rule in BUILTIN.iter() {
            out = match rule {
                Rule::Plain(re) => re.replace_all(&out, MASK).into_owned(),
                Rule::KeepPrefix(re) => re.replace_all(&out, |c: &regex::Captures| format!("{}{MASK}", &c[1])).into_owned(),
                Rule::Assignment(re) => re
                    .replace_all(&out, |c: &regex::Captures| {
                        let (prefix, value) = (&c[1], &c[2]);
                        // The keyword must not sit inside a longer identifier's tail
                        // that the regex could not anchor; mimic the Python lookbehind.
                        if INNOCUOUS.is_match(value) {
                            c[0].to_string()
                        } else {
                            format!("{prefix}{MASK}")
                        }
                    })
                    .into_owned(),
            };
        }
        for re in &self.custom {
            out = re.replace_all(&out, MASK).into_owned();
        }
        out
    }
}

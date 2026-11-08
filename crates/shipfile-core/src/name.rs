//! Names of the app, environments and resources.

use std::fmt;

use serde::Serialize;

/// Longest allowed name. AWS names are built from several of these, so they stay short.
pub const MAX_LEN: usize = 32;

/// A DNS-safe name: lowercase letters, digits and single hyphens, starting with a letter.
///
/// Because underscores are not allowed, turning a name into an environment-variable prefix
/// (`orders-queue` → `ORDERS_QUEUE`) can never make two different names collide.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Name(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("is empty")]
    Empty,
    #[error("is longer than {MAX_LEN} characters")]
    TooLong,
    #[error("must start with a lowercase letter")]
    BadStart,
    #[error("contains `{0}`; only lowercase letters, digits and `-` are allowed")]
    BadChar(char),
    #[error("must not end with `-`")]
    TrailingHyphen,
    #[error("must not contain `--`")]
    DoubleHyphen,
}

impl Name {
    pub fn new(name: &str) -> Result<Self, NameError> {
        let first = name.chars().next().ok_or(NameError::Empty)?;
        if name.chars().count() > MAX_LEN {
            return Err(NameError::TooLong);
        }
        if let Some(bad) = name.chars().find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')) {
            return Err(NameError::BadChar(bad));
        }
        if !first.is_ascii_lowercase() {
            return Err(NameError::BadStart);
        }
        if name.ends_with('-') {
            return Err(NameError::TrailingHyphen);
        }
        if name.contains("--") {
            return Err(NameError::DoubleHyphen);
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `orders-queue` → `ORDERS_QUEUE`.
    pub fn env_prefix(&self) -> String {
        self.0.to_ascii_uppercase().replace('-', "_")
    }

    /// `main-db` → `main_db`, for identifiers that cannot hold hyphens (such as database names).
    pub fn snake(&self) -> String {
        self.0.replace('-', "_")
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A conventional, lowercase suggestion for an invalid name (`Main_DB` → `main-db`).
pub fn suggest_fix(name: &str) -> Option<String> {
    let mut fixed = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            fixed.push(c);
        } else if !fixed.ends_with('-') && !fixed.is_empty() {
            fixed.push('-');
        }
    }
    let fixed: String = fixed.trim_end_matches('-').chars().take(MAX_LEN).collect();
    let fixed = fixed.trim_end_matches('-').to_owned();
    (Name::new(&fixed).is_ok() && fixed != name).then_some(fixed)
}

/// Whether `name` is a valid environment-variable name.
pub fn is_env_var(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_dns_safe_names() {
        for ok in ["api", "orders-queue", "a1", "main-db-2", &"a".repeat(MAX_LEN)] {
            assert!(Name::new(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_bad_names_with_a_reason() {
        assert_eq!(Name::new(""), Err(NameError::Empty));
        assert_eq!(Name::new(&"a".repeat(MAX_LEN + 1)), Err(NameError::TooLong));
        assert_eq!(Name::new("1api"), Err(NameError::BadStart));
        assert_eq!(Name::new("-api"), Err(NameError::BadStart));
        assert_eq!(Name::new("Api"), Err(NameError::BadChar('A')));
        assert_eq!(Name::new("main_db"), Err(NameError::BadChar('_')));
        assert_eq!(Name::new("api-"), Err(NameError::TrailingHyphen));
        assert_eq!(Name::new("a--b"), Err(NameError::DoubleHyphen));
    }

    #[test]
    fn env_prefix_and_snake() {
        let name = Name::new("orders-queue").unwrap();
        assert_eq!(name.env_prefix(), "ORDERS_QUEUE");
        assert_eq!(name.snake(), "orders_queue");
    }

    #[test]
    fn fixes_common_mistakes() {
        assert_eq!(suggest_fix("Main_DB").as_deref(), Some("main-db"));
        assert_eq!(suggest_fix("orders queue").as_deref(), Some("orders-queue"));
        assert_eq!(suggest_fix("api-").as_deref(), Some("api"));
        assert_eq!(suggest_fix("123"), None);
    }

    #[test]
    fn env_var_names() {
        assert!(is_env_var("LOG_LEVEL"));
        assert!(is_env_var("_x1"));
        assert!(!is_env_var("1X"));
        assert!(!is_env_var("A-B"));
        assert!(!is_env_var(""));
    }
}

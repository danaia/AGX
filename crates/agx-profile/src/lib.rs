//! Account provisioning models and validation shared by the UI and root service.
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
pub mod service;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub name: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub hostname: String,
    pub accounts: Vec<Account>,
}
// No Debug: this command contains a password.
#[derive(Serialize, Deserialize)]
pub struct Save {
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub create: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quality {
    pub score: u16,
    pub label: &'static str,
    pub accepted: bool,
}
pub fn quality(password: &str, username: &str, name: &str) -> Quality {
    let length = password.chars().count();
    let lower = password.to_lowercase();
    let common = [
        "password", "qwerty", "letmein", "123456", "welcome", "changeme", "abcdefgh", "iloveyou",
    ]
    .iter()
    .any(|word| lower.contains(word));
    let personal = [username, name]
        .iter()
        .filter(|v| v.len() >= 3)
        .any(|v| lower.contains(&v.to_lowercase()));
    let unique = password
        .chars()
        .collect::<std::collections::HashSet<_>>()
        .len();
    let valid = (1..=128).contains(&length)
        && password.len() <= 512
        && !password.chars().any(char::is_control);
    let classes = [
        password.chars().any(char::is_uppercase),
        password.chars().any(char::is_lowercase),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    let score = if !valid || length < 12 || common || personal || unique < 6 {
        0
    } else if length >= 24 || (length >= 16 && classes >= 3) {
        100
    } else if length >= 16 || classes >= 3 {
        75
    } else {
        50
    };
    Quality {
        score,
        label: match score {
            0 => "Weak",
            50 => "Fair",
            75 => "Good",
            _ => "Strong",
        },
        accepted: valid,
    }
}
pub fn validate(save: &Save) -> Result<(), String> {
    let user = &save.username;
    if !(1..=32).contains(&user.len())
        || !user.as_bytes()[0].is_ascii_lowercase()
        || !user
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        || ["root", "daemon", "nobody", "agx-admin"].contains(&user.as_str())
    {
        return Err("Username: 1–32 lowercase letters, digits, _ or -; start with a letter. System accounts are reserved.".into());
    }
    if save.name.trim().is_empty()
        || save.name.len() > 80
        || save.name.chars().any(|c| c.is_control() || c == ':')
    {
        return Err("Enter a name of 1–80 bytes, without colons or control characters.".into());
    }
    let host = &save.hostname;
    if host.is_empty()
        || host.len() > 63
        || !host
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || host.starts_with('-')
        || host.ends_with('-')
    {
        return Err("Hostname: 1–63 lowercase letters, digits or internal hyphens.".into());
    }
    if (save.create || !save.password.is_empty())
        && !quality(&save.password, user, &save.name).accepted
    {
        return Err("Enter a password of 1–128 characters, without control characters.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn form() -> Save {
        Save {
            name: "Test Person".into(),
            hostname: "agx-lab".into(),
            username: "tester".into(),
            password: Zeroizing::new("orbit cedar glacier meadow".into()),
            create: true,
        }
    }
    #[test]
    fn password_quality_is_advisory_and_simple_passwords_are_allowed() {
        for bad in [
            "",
            "Password123456!",
            "aaaaaaaaaaaaaaaaaaaa",
            "tester-symbols-123",
            "qwerty123456789",
            "short!23",
        ] {
            assert_eq!(quality(bad, "tester", "Test Person").score, 0);
        }
        for simple in ["1", "abc", "1234", "password"] {
            let mut save = form();
            save.password = Zeroizing::new(simple.into());
            assert!(validate(&save).is_ok());
            assert_eq!(quality(simple, "tester", "Test Person").score, 0);
        }
        assert_eq!(
            quality("orbit cedar glacier meadow", "tester", "Test Person").label,
            "Strong"
        );
        assert!(!quality("glacier\ncedar meadow!", "tester", "Test Person").accepted);
    }
    #[test]
    fn identity_validation_blocks_system_names_and_record_injection() {
        assert!(validate(&form()).is_ok());
        for user in ["root", "-abc", "a:b", "a/b", "Upper", "a\nx"] {
            let mut v = form();
            v.username = user.into();
            assert!(validate(&v).is_err());
        }
        for host in ["-host", "host-", "a.b", "a\nx", ""] {
            let mut v = form();
            v.hostname = host.into();
            assert!(validate(&v).is_err());
        }
        let mut v = form();
        v.name = "bad:record".into();
        assert!(validate(&v).is_err());
        let mut v = form();
        v.password.clear();
        assert!(validate(&v).is_err());
        v.create = false;
        assert!(validate(&v).is_ok());
    }
}

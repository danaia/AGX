//! Explainable deletion-risk estimates from file location/type, independent of size.
use std::path::Path;

pub(super) struct Risk {
    pub level: i32,
    pub label: &'static str,
    pub reason: &'static str,
}

pub(super) fn assess(path: &Path) -> Risk {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let parts: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let has = |names: &[&str]| parts.iter().any(|part| names.contains(&part.as_str()));

    // Preserve data-bearing files even when they happen to live in a cache tree.
    if ["db", "sqlite", "sqlite3", "db3", "mdb"].contains(&extension.as_str())
        || ["-wal", "-shm", ".wal"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
    {
        return Risk {
            level: 2,
            label: "High risk",
            reason: "Looks like a database or its working data. Removing it may lose application records or disrupt an app.",
        };
    }
    if has(&["backup", "backups"])
        || name.contains("backup")
        || ["bak", "backup", "key", "pem", "p12", "pfx", "kdbx"].contains(&extension.as_str())
    {
        return Risk {
            level: 2,
            label: "High risk",
            reason: "Looks like a backup, key, or vault. It may be your only recovery copy or contain important credentials.",
        };
    }
    if has(&[".cache", "cache", "tmp", "temp"])
        || path.starts_with("/var/lib/agx-data/agx-opencode-users") && has(&["target"])
        || ["o", "rlib", "rmeta", "pyc"].contains(&extension.as_str())
    {
        return Risk {
            level: 0,
            label: "Low risk",
            reason: "Appears to be cached, temporary, or compiled data. An app may need to download or rebuild it.",
        };
    }
    if has(&[
        "documents",
        "pictures",
        "videos",
        "music",
        "projects",
        "angi-dev",
    ]) || [
        "rs", "c", "cpp", "h", "py", "js", "ts", "slint", "qml", "go", "java", "csv", "parquet",
        "json",
    ]
    .contains(&extension.as_str())
    {
        return Risk {
            level: 2,
            label: "High risk",
            reason: "Appears to be original personal or project data. Deleting it could lose work unless you have another copy.",
        };
    }
    Risk {
        level: 1,
        label: "Medium risk",
        reason: "This is a personal file of uncertain importance. Check that you no longer need it or have another copy.",
    }
}

#[cfg(test)]
mod tests {
    use super::assess;
    use std::path::Path;

    #[test]
    fn caches_are_low_risk_but_databases_and_backups_override_the_location() {
        assert_eq!(
            assess(Path::new("/home/dana/.cache/mesa_shader_cache/index")).level,
            0
        );
        assert_eq!(
            assess(Path::new(
                "/var/lib/agx-data/agx-opencode-users/1000/cargo/target/debug/app"
            ))
            .level,
            0
        );
        assert_eq!(
            assess(Path::new("/home/dana/.cache/session.sqlite")).level,
            2
        );
        assert_eq!(
            assess(Path::new("/home/dana/.cache/session.sqlite-wal")).level,
            2
        );
        assert_eq!(
            assess(Path::new("/home/dana/Downloads/backup.zip")).level,
            2
        );
    }

    #[test]
    fn original_files_are_high_risk_and_unknown_downloads_are_medium() {
        for path in [
            "/home/dana/Documents/notes.pdf",
            "/home/dana/Pictures/photo.jpg",
            "/var/lib/agx-data/angi-dev/source.rs",
            "/home/dana/Downloads/records.CSV",
        ] {
            let risk = assess(Path::new(path));
            assert_eq!(risk.level, 2);
            assert!(!risk.reason.is_empty());
        }
        let risk = assess(Path::new("/home/dana/Downloads/archive.zip"));
        assert_eq!(risk.level, 1);
        assert_eq!(risk.label, "Medium risk");
    }
}

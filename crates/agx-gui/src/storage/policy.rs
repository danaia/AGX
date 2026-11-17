//! Files eligible for personal-space cleanup; never elevate desktop permissions.
use std::{
    ffi::CString,
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

pub(super) const MIN_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub(super) struct Policy {
    pub uid: u32,
    pub roots: Vec<PathBuf>,
}

impl Policy {
    pub fn current() -> Self {
        let uid = unsafe { libc::geteuid() };
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            if home.starts_with("/home") && home != Path::new("/home") {
                roots.push(home);
            }
        }
        // The appliance keeps per-account data and development files on NVMe.
        for path in [
            format!("/var/lib/agx-data/agx-web-users/{uid}/cache"),
            format!("/var/lib/agx-data/agx-opencode-users/{uid}/cache"),
            format!("/var/lib/agx-data/agx-opencode-users/{uid}/tmp"),
            format!("/var/lib/agx-data/agx-opencode-users/{uid}/cargo/target"),
            "/var/lib/agx-data/angi-dev".into(),
        ] {
            roots.push(path.into());
        }
        if let Some(name) = std::env::var_os("USER") {
            let name = PathBuf::from(name);
            if name.components().count() == 1 && name.file_name().is_some() {
                roots.push(Path::new("/var/lib/agx-data/users").join(name));
            }
        }
        Self { uid, roots }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
            && !path.components().any(|part| match part {
                std::path::Component::Normal(name) => {
                    name.as_bytes().starts_with(b".angi-delete-")
                        || [".git", ".ssh", ".gnupg", ".config", ".codex"]
                            .iter()
                            .any(|blocked| name == *blocked)
                }
                std::path::Component::ParentDir => true,
                _ => false,
            })
            && !self
                .roots
                .iter()
                .any(|root| path.starts_with(root.join(".local/bin")))
    }

    pub fn eligible(&self, path: &Path, metadata: &fs::Metadata) -> bool {
        metadata.is_file()
            && metadata.uid() == self.uid
            && metadata.len() >= MIN_BYTES
            && self.contains(path)
            && path.parent().is_some_and(writable_directory)
    }
}

fn writable_directory(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Test the signed-in process's effective permissions, including directory ACLs.
    unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::W_OK | libc::X_OK,
            libc::AT_EACCESS,
        ) == 0
    }
}

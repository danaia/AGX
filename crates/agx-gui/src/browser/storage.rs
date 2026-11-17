//! The root greeter prepares a private NVMe directory before dropping privileges.
//! Web content never receives access to other users' stores or the SD home.
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
const VOLUME: &str = "/var/lib/agx-data";
const USERS: &str = "/var/lib/agx-data/agx-web-users";
fn directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}
fn create(path: &Path, mode: u32) -> io::Result<()> {
    match DirBuilder::new().mode(mode).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}
fn private_directory(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    create(path, 0o700)?;
    let file = directory(path)?;
    let metadata = file.metadata()?;
    if metadata.uid() != 0 && metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Web storage belongs to another account",
        ));
    }
    // The descriptor cannot follow a symlink or change when a directory is renamed.
    if unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } != 0
        || unsafe { libc::fchmod(file.as_raw_fd(), 0o700) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
/// Configure before exec so native-library initializers also use NVMe.
pub(super) fn configure_child(command: &mut std::process::Command) {
    if let (Ok(volume), Ok(root)) = (fs::metadata(VOLUME), fs::metadata("/")) {
        if volume.dev() != root.dev() {
            let user = Path::new(USERS).join(unsafe { libc::geteuid() }.to_string());
            command.env("XDG_DATA_HOME", user.join("data"));
            command.env("XDG_CACHE_HOME", user.join("cache"));
            command.env("MESA_SHADER_CACHE_DIR", user.join("cache/shaders"));
        }
    }
}
pub(crate) fn prepare_accounts() -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Root is required to prepare account storage",
        ));
    }
    for line in fs::read_to_string("/etc/passwd")?.lines() {
        let mut fields = line.split(':').skip(2);
        let (Some(uid), Some(gid)) = (
            fields.next().and_then(|s| s.parse::<u32>().ok()),
            fields.next().and_then(|s| s.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if uid >= 1000 && uid != 65534 {
            prepare_user_storage(uid, gid)?;
        }
    }
    Ok(())
}
pub(crate) fn prepare_user_storage(uid: u32, gid: u32) -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Only the greeter can prepare Web storage",
        ));
    }
    let volume = directory(Path::new(VOLUME))?.metadata()?;
    if volume.dev() == fs::metadata("/")?.dev() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "NVMe data volume is not mounted",
        ));
    }
    if volume.uid() != 0 || volume.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NVMe data root must be protected from account writes",
        ));
    }
    create(Path::new(USERS), 0o755)?;
    let parent = directory(Path::new(USERS))?;
    let metadata = parent.metadata()?;
    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Web storage parent must be root-owned and protected",
        ));
    }
    if unsafe { libc::fchmod(parent.as_raw_fd(), 0o755) } != 0 {
        return Err(io::Error::last_os_error());
    }
    private_directory(&Path::new(USERS).join(uid.to_string()), uid, gid)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn temporary() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("angi-web-storage-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn storage_is_private_and_existing_permissions_are_repaired() {
        let parent = temporary();
        let path = parent.join("user");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        private_directory(&path, unsafe { libc::getuid() }, unsafe { libc::getgid() }).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn preparation_refuses_a_symlink_without_changing_its_target() {
        let parent = temporary();
        let target = parent.join("target");
        let link = parent.join("user");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&target, &link).unwrap();
        assert!(
            private_directory(&link, unsafe { libc::getuid() }, unsafe { libc::getgid() }).is_err()
        );
        assert_eq!(fs::metadata(target).unwrap().mode() & 0o777, 0o755);
        fs::remove_dir_all(parent).unwrap();
    }
}

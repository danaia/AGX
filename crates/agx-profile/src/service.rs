//! Root service account database operations. No credential enters process arguments.
use crate::{Account, Save, Status, validate};
use std::{
    ffi::{CStr, CString},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
    },
    path::Path,
    sync::Mutex,
};
use zeroize::Zeroizing;
const GROUP: &str = "agx-admin";
static CRYPT: Mutex<()> = Mutex::new(());
#[link(name = "crypt")]
unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}
unsafe extern "C" {
    fn lckpwdf() -> libc::c_int;
    fn ulckpwdf() -> libc::c_int;
}
fn err(text: &str) -> io::Error {
    io::Error::other(text)
}
struct Lock;
impl Lock {
    fn acquire() -> io::Result<Self> {
        if unsafe { lckpwdf() } != 0 {
            return Err(err("Account database is busy; retry shortly"));
        }
        Ok(Self)
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            ulckpwdf();
        }
    }
}
fn atomic(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let temp = path.with_extension("agx-new");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(mode)
        .open(&temp)?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    File::open(path.parent().unwrap())?.sync_all()
}
fn chown(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    let name = CString::new(path.as_os_str().as_bytes())?;
    if unsafe { libc::chown(name.as_ptr(), uid, gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn admin_gid() -> io::Result<u32> {
    let groups = fs::read_to_string("/etc/group")?;
    groups
        .lines()
        .find_map(|line| {
            let f: Vec<_> = line.split(':').collect();
            (f.len() == 4 && f[0] == GROUP)
                .then(|| f[2].parse().ok())
                .flatten()
        })
        .ok_or_else(|| err("ANGI administration group is missing"))
}
pub fn prepare_access(socket: &str) -> io::Result<()> {
    let gid = ensure_group()?;
    chown(Path::new("/run/agx"), 0, gid)?;
    fs::set_permissions("/run/agx", fs::Permissions::from_mode(0o750))?;
    chown(Path::new(socket), 0, gid)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660))
}
fn ensure_group() -> io::Result<u32> {
    let _lock = Lock::acquire()?;
    if let Ok(gid) = admin_gid() {
        return Ok(gid);
    }
    let mut groups = fs::read_to_string("/etc/group")?;
    let used: Vec<u32> = groups
        .lines()
        .filter_map(|l| l.split(':').nth(2)?.parse().ok())
        .collect();
    let gid = (1000..60000)
        .find(|id| !used.contains(id))
        .ok_or_else(|| err("No free group ID"))?;
    groups.push_str(&format!("{GROUP}:x:{gid}:\n"));
    atomic(Path::new("/etc/group"), groups.as_bytes(), 0o644)?;
    Ok(gid)
}
pub fn status() -> io::Result<Status> {
    let gid = admin_gid()?;
    let passwd = fs::read_to_string("/etc/passwd")?;
    let accounts = passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<_> = l.split(':').collect();
            if f.len() != 7 || f[2].parse::<u32>().ok()? < 1000 || f[3].parse::<u32>().ok()? != gid
            {
                return None;
            }
            Some(Account {
                username: f[0].into(),
                name: f[4].into(),
            })
        })
        .collect();
    Ok(Status {
        hostname: fs::read_to_string("/etc/hostname")?.trim().into(),
        accounts,
    })
}
fn hash(password: &str) -> io::Result<Zeroizing<String>> {
    let _guard = CRYPT
        .lock()
        .map_err(|_| err("Password hasher unavailable"))?;
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let alphabet = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let salt: String = random
        .into_iter()
        .map(|b| alphabet[(b & 63) as usize] as char)
        .collect();
    let salt = CString::new(format!("$6$rounds=100000${salt}$"))?;
    let mut input = Zeroizing::new(password.as_bytes().to_vec());
    input.push(0);
    let output = unsafe { crypt(input.as_ptr().cast(), salt.as_ptr()) };
    if output.is_null() {
        return Err(err("Password hashing failed"));
    }
    let result = Zeroizing::new(
        unsafe { CStr::from_ptr(output) }
            .to_string_lossy()
            .into_owned(),
    );
    if !result.starts_with("$6$rounds=100000$") {
        return Err(err("SHA-512 password hashing is unavailable"));
    }
    Ok(result)
}

/// Authenticate an existing AGX administrator for the local graphical session.
/// Root, locked accounts, expired credentials and passwordless accounts are rejected.
pub fn authenticate(username: &str, password: &str) -> io::Result<()> {
    let denied = || err("Username or password is incorrect, or the account is unavailable");
    if password.is_empty() || password.len() > 512 || password.contains('\0') {
        return Err(denied());
    }
    if !status()?.accounts.iter().any(|a| a.username == username) {
        return Err(denied());
    }
    let shadow = Zeroizing::new(fs::read_to_string("/etc/shadow")?);
    let fields: Vec<_> = shadow
        .lines()
        .find(|line| line.split(':').next() == Some(username))
        .ok_or_else(denied)?
        .split(':')
        .collect();
    let today = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs()
        / 86400) as i64;
    if !shadow_allows_login(&fields, today) {
        return Err(denied());
    }
    if !password_matches(password, fields[1])? {
        return Err(denied());
    }
    Ok(())
}

fn password_matches(password: &str, hashed: &str) -> io::Result<bool> {
    if password.contains('\0') || !hashed.starts_with('$') {
        return Ok(false);
    }
    let salt = CString::new(hashed)?;
    let mut input = Zeroizing::new(password.as_bytes().to_vec());
    input.push(0);
    let _guard = CRYPT
        .lock()
        .map_err(|_| err("Password hasher unavailable"))?;
    let output = unsafe { crypt(input.as_ptr().cast(), salt.as_ptr()) };
    if output.is_null() {
        return Ok(false);
    }
    let actual = unsafe { CStr::from_ptr(output) }.to_bytes();
    let expected = hashed.as_bytes();
    Ok(actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .fold(0u8, |v, (a, b)| v | (a ^ b))
            == 0)
}

fn shadow_allows_login(fields: &[&str], today: i64) -> bool {
    if fields.len() != 9 || !fields[1].starts_with('$') {
        return false;
    }
    let day = |i: usize| -> Result<Option<i64>, ()> {
        match fields[i] {
            "" | "-1" => Ok(None),
            value => value
                .parse::<i64>()
                .ok()
                .filter(|v| *v >= 0)
                .map(Some)
                .ok_or(()),
        }
    };
    let (Ok(last), Ok(max), Ok(expire)) = (day(2), day(4), day(7)) else {
        return false;
    };
    last != Some(0)
        && !expire.is_some_and(|date| today >= date)
        && !last
            .zip(max)
            .is_some_and(|(last, max)| today > last.saturating_add(max))
}

#[cfg(test)]
mod login_tests {
    use super::*;
    #[test]
    fn existing_password_hash_accepts_only_the_correct_password() {
        let hashed = hash("test login password").unwrap();
        assert!(password_matches("test login password", &hashed).unwrap());
        assert!(!password_matches("incorrect password", &hashed).unwrap());
        assert!(!password_matches("test login password\0suffix", &hashed).unwrap());
        assert!(!password_matches("test login password", "!locked").unwrap());
    }
    #[test]
    fn shadow_policy_rejects_locked_and_expired_accounts() {
        for (record, allowed) in [
            ("user:$6$salt$hash:20000:0:99999:7:::", true),
            ("user:!$6$salt$hash:20000:0:99999:7:::", false),
            ("user::20000:0:99999:7:::", false),
            ("user:$6$salt$hash:0:0:99999:7:::", false),
            ("user:$6$salt$hash:20000:0:30:7:::", false),
            ("user:$6$salt$hash:20000:0:99999:7::20001:", false),
            ("user:$6$salt$hash:bad:0:99999:7:::", false),
        ] {
            assert_eq!(
                shadow_allows_login(&record.split(':').collect::<Vec<_>>(), 20040),
                allowed,
                "{record}"
            );
        }
    }
}
pub fn save(save: Save) -> io::Result<String> {
    validate(&save).map_err(io::Error::other)?;
    let gid = admin_gid()?;
    let hashed = if !save.password.is_empty() {
        Some(hash(&save.password)?)
    } else {
        None
    };
    let _lock = Lock::acquire()?;
    let passwd = fs::read_to_string("/etc/passwd")?;
    let shadow = Zeroizing::new(fs::read_to_string("/etc/shadow")?);
    let existing = passwd
        .lines()
        .find(|l| l.split(':').next() == Some(&save.username));
    if save.create && existing.is_some() {
        return Err(err(
            "That username already exists. Select the saved account to edit it.",
        ));
    }
    if !save.create
        && !status()?
            .accounts
            .iter()
            .any(|a| a.username == save.username)
    {
        return Err(err("Only ANGI administrator accounts can be edited here"));
    }
    let uid = if let Some(line) = existing {
        line.split(':')
            .nth(2)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| err("Invalid account record"))?
    } else {
        let used: Vec<u32> = passwd
            .lines()
            .filter_map(|l| l.split(':').nth(2)?.parse().ok())
            .collect();
        (1000..60000)
            .find(|id| !used.contains(id))
            .ok_or_else(|| err("No free user ID"))?
    };
    let home = format!("/home/{}", save.username);
    if save.create {
        fs::create_dir_all("/home")?;
        fs::set_permissions("/home", fs::Permissions::from_mode(0o755))?;
        // Never adopt or chown an existing home belonging to another account.
        fs::create_dir(&home)
            .map_err(|_| err("Home directory already exists or cannot be created"))?;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        chown(Path::new(&home), uid, gid)?;
    }
    let mut new_passwd = String::new();
    for line in passwd.lines() {
        if line.split(':').next() != Some(&save.username) {
            new_passwd.push_str(line);
            new_passwd.push('\n');
        }
    }
    new_passwd.push_str(&format!(
        "{}:x:{uid}:{gid}:{}:{home}:/bin/sh\n",
        save.username,
        save.name.trim()
    ));
    let mut new_shadow = Zeroizing::new(String::new());
    let mut found = false;
    for line in shadow.lines() {
        let mut fields: Vec<_> = line.split(':').map(str::to_owned).collect();
        if fields.first().map(String::as_str) == Some(&save.username) {
            found = true;
            if fields.len() != 9 {
                return Err(err("Invalid shadow record"));
            }
            if let Some(hash) = &hashed {
                fields[1] = hash.to_string();
                fields[2] = days().to_string();
            }
            new_shadow.push_str(&fields.join(":"));
        } else if fields.first().map(String::as_str) == Some("root")
            && fields.get(1).is_some_and(String::is_empty)
        {
            // End the development console's passwordless root login. SSH keys still work.
            fields[1] = "!".into();
            new_shadow.push_str(&fields.join(":"));
        } else {
            new_shadow.push_str(line);
        }
        new_shadow.push('\n');
    }
    if !found {
        let hash = hashed
            .as_ref()
            .ok_or_else(|| err("A password is required for a new account"))?;
        new_shadow.push_str(&format!(
            "{}:{}:{}:0:99999:7:::\n",
            save.username,
            hash.as_str(),
            days()
        ));
    }
    // Shadow first: a newly exposed passwd entry can never have a blank password.
    atomic(Path::new("/etc/shadow"), new_shadow.as_bytes(), 0o600)?;
    if let Err(error) = atomic(Path::new("/etc/passwd"), new_passwd.as_bytes(), 0o644) {
        let _ = atomic(Path::new("/etc/shadow"), shadow.as_bytes(), 0o600);
        return Err(error);
    }
    let result = apply_hostname(&save.hostname);
    if result.is_err() {
        return Err(err(
            "Account saved, but hostname update failed. Retry Save to apply the hostname.",
        ));
    }
    Ok(format!(
        "Saved {}. SSH: ssh {}@{} (or use this Pi's IP). Password login is ready.",
        save.name.trim(),
        save.username,
        save.hostname
    ))
}
fn days() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400
}
fn apply_hostname(host: &str) -> io::Result<()> {
    atomic(
        Path::new("/etc/hostname"),
        format!("{host}\n").as_bytes(),
        0o644,
    )?;
    let old = fs::read_to_string("/etc/hosts")?;
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| l.split_whitespace().next() != Some("127.0.1.1"))
        .map(str::to_owned)
        .collect();
    lines.push(format!("127.0.1.1\t{host}"));
    atomic(
        Path::new("/etc/hosts"),
        format!("{}\n", lines.join("\n")).as_bytes(),
        0o644,
    )?;
    if unsafe { libc::sethostname(host.as_ptr().cast(), host.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn salted_hashes_are_sha512_and_never_plaintext() {
        let a = hash("cedar orbit glacier meadow").unwrap();
        let b = hash("cedar orbit glacier meadow").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("$6$rounds=100000$"));
        assert!(!a.contains("cedar"));
    }
}

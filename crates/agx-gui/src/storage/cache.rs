//! Bulk cleanup is limited to verified content-addressed npm downloads.
use super::{delete, scan::File};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const QUIET: Duration = Duration::from_secs(60 * 60);
const MAX_ENTRIES: usize = 32_768;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(super) struct Plan {
    uid: u32,
    entries: Vec<Candidate>,
    pub skipped: usize,
}

struct Candidate {
    file: File,
    changed: (i64, i64),
}

pub(super) struct Outcome {
    pub removed: Vec<File>,
    pub failed: usize,
    pub first_error: Option<String>,
}

impl Plan {
    pub fn check(uid: u32) -> io::Result<Self> {
        Self::inspect(
            Path::new(&format!(
                "/var/lib/agx-data/agx-opencode-users/{uid}/cache/npm/_cacache/content-v2/sha512"
            )),
            uid,
            SystemTime::now(),
        )
    }

    fn inspect(root: &Path, uid: u32, now: SystemTime) -> io::Result<Self> {
        let mut plan = Self {
            uid,
            entries: Vec::new(),
            skipped: 0,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut walked = 0;
        let mut remaining = MAX_BYTES;
        let directory = match delete::parent(&root.join("entry")) {
            Ok(dir) => dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(plan),
            Err(err) => return Err(err),
        };
        let device = directory.metadata()?.dev();
        plan.walk(
            &directory,
            root,
            &mut String::new(),
            now,
            device,
            deadline,
            &mut walked,
            &mut remaining,
        )?;
        Ok(plan)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        dir: &fs::File,
        path: &Path,
        digest: &mut String,
        now: SystemTime,
        device: u64,
        deadline: Instant,
        walked: &mut usize,
        remaining: &mut u64,
    ) -> io::Result<()> {
        for item in fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))? {
            *walked += 1;
            if *walked > MAX_ENTRIES || Instant::now() >= deadline {
                return Err(limit());
            }
            let item = item?;
            let name = item.file_name();
            let width = if digest.len() == 4 { 124 } else { 2 };
            let Some(part) = name.to_str().filter(|s| {
                s.len() == width
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }) else {
                self.skipped += 1;
                continue;
            };
            let path = path.join(part);
            digest.push_str(part);
            let kind = item.file_type()?;
            if digest.len() < 128 && kind.is_dir() {
                let subdir = open_at(dir, part, libc::O_DIRECTORY)?;
                if subdir.metadata()?.dev() == device {
                    self.walk(
                        &subdir, &path, digest, now, device, deadline, walked, remaining,
                    )?;
                } else {
                    self.skipped += 1;
                }
            } else if digest.len() == 128 && kind.is_file() {
                match verify(
                    dir, part, path, digest, self.uid, now, device, deadline, remaining,
                )? {
                    Some(entry) => self.entries.push(entry),
                    None => self.skipped += 1,
                }
            } else {
                self.skipped += 1;
            }
            digest.truncate(digest.len() - part.len());
        }
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }
    pub fn bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.file.allocated).sum()
    }

    pub fn clear(self) -> Outcome {
        let mut result = Outcome {
            removed: Vec::new(),
            failed: 0,
            first_error: None,
        };
        for entry in self.entries {
            match delete::remove_checked(&entry.file, |meta, moved| {
                cache_file(meta, self.uid) && (moved || changed(meta) == entry.changed)
            }) {
                Ok(()) => result.removed.push(entry.file),
                Err(error) => {
                    result.failed += 1;
                    result
                        .first_error
                        .get_or_insert_with(|| format!("{}: {error}", entry.file.path.display()));
                }
            }
        }
        result
    }
}

fn changed(meta: &fs::Metadata) -> (i64, i64) {
    (meta.ctime(), meta.ctime_nsec())
}

fn cache_file(meta: &fs::Metadata, uid: u32) -> bool {
    meta.is_file() && meta.uid() == uid && meta.nlink() == 1 && meta.mode() & 0o7111 == 0
}

fn limit() -> io::Error {
    io::Error::other("The cache check reached its work limit. Nothing was selected.")
}

fn open_at(dir: &fs::File, part: &str, flags: libc::c_int) -> io::Result<fs::File> {
    let part = delete::name(Path::new(part))?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            part.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[allow(clippy::too_many_arguments)]
fn verify(
    dir: &fs::File,
    name: &str,
    path: PathBuf,
    expected: &str,
    uid: u32,
    now: SystemTime,
    device: u64,
    deadline: Instant,
    remaining: &mut u64,
) -> io::Result<Option<Candidate>> {
    let input = open_at(dir, name, libc::O_RDONLY)?;
    let meta = input.metadata()?;
    if !cache_file(&meta, uid)
        || meta.dev() != device
        || meta.len() == 0
        || !meta
            .modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age >= QUIET)
    {
        return Ok(None);
    }
    // Match the entire SHA-512 digest so arbitrary files in the directory are not selected.
    let mut hash = Sha512::new();
    let mut reader = (&input).take(meta.len() + 1);
    let mut buffer = [0; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(limit());
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        *remaining = remaining.checked_sub(read as u64).ok_or_else(limit)?;
        hash.update(&buffer[..read]);
    }
    if format!("{:x}", hash.finalize()) != expected || changed(&input.metadata()?) != changed(&meta)
    {
        return Ok(None);
    }
    Ok(Some(Candidate {
        changed: changed(&meta),
        file: File {
            path,
            bytes: meta.len(),
            allocated: meta.blocks().saturating_mul(512),
            device: meta.dev(),
            inode: meta.ino(),
            modified: (meta.mtime(), meta.mtime_nsec()),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn blob(root: &Path, content: &[u8]) -> PathBuf {
        let hex = format!("{:x}", Sha512::digest(content));
        let file = root.join(&hex[..2]).join(&hex[2..4]).join(&hex[4..]);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn cleanup_only_removes_unchanged_verified_downloads() {
        let fixture = super::super::tests::Fixture::new("cache-verified");
        let root = fixture.0.join("content-v2/sha512");
        let good = blob(&root, b"verified download");
        let replaced = blob(&root, b"replace after preview");
        let linked = blob(&root, b"hard linked data");
        let bad = blob(&root, b"inconsistent digest");
        fs::write(&bad, b"unrelated content").unwrap();
        let other = fixture.0.join("valuable.txt");
        fs::write(&other, b"keep").unwrap();
        fs::hard_link(&linked, fixture.0.join("hardlink")).unwrap();
        let symlinked = blob(&root, b"symlinked");
        fs::remove_file(&symlinked).unwrap();
        symlink(&other, &symlinked).unwrap();
        let unknown = root.join("notes.txt");
        fs::write(&unknown, b"private notes").unwrap();
        let index = fixture.0.join("index-v5");
        fs::write(&index, b"cache index").unwrap();
        let uid = unsafe { libc::geteuid() };
        let now = SystemTime::now();
        assert_eq!(Plan::inspect(&root, uid, now).unwrap().count(), 0);
        let plan = Plan::inspect(&root, uid, now + QUIET * 2).unwrap();
        assert_eq!((plan.count(), plan.skipped), (2, 4));
        assert!(good.exists(), "the preview must not remove anything");
        fs::rename(&replaced, fixture.0.join("old-download")).unwrap();
        fs::write(&replaced, b"keep this replacement").unwrap();
        let outcome = plan.clear();
        assert_eq!((outcome.removed.len(), outcome.failed), (1, 1));
        assert!(!good.exists());
        assert_eq!(fs::read(replaced).unwrap(), b"keep this replacement");
        for path in [other, linked, bad, symlinked, unknown, index] {
            assert!(path.exists(), "{} was unexpectedly removed", path.display());
        }
    }

    #[test]
    fn cache_check_refuses_a_symlinked_root() {
        let fixture = super::super::tests::Fixture::new("cache-root-symlink");
        let root = fixture.0.join("real");
        let good = blob(&root, b"verified download");
        let alias = fixture.0.join("sha512");
        symlink(&root, &alias).unwrap();
        let uid = unsafe { libc::geteuid() };
        assert!(Plan::inspect(&alias, uid, SystemTime::now() + QUIET * 2).is_err());
        assert!(good.exists());
    }
}

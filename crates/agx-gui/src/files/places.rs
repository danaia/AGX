//! The sidebar: home and its standard folders, the personal NVMe space, and mounted drives.
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub struct Place {
    pub label: String,
    pub glyph: &'static str,
    pub path: PathBuf,
    /// 0 for personal folders, 1 for drives.
    pub section: i32,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn places() -> Vec<Place> {
    let home = home();
    let mut found = vec![Place { label: "Home".into(), glyph: "󰋜", path: home.clone(), section: 0 }];
    for (name, glyph) in [("Desktop", "󰟀"), ("Documents", "󰈙"), ("Downloads", "󰇚"), ("Music", "󰝚"), ("Pictures", "󰋩"), ("Videos", "󰕧")] {
        let path = home.join(name);
        if path.is_dir() {
            found.push(Place { label: name.into(), glyph, path, section: 0 });
        }
    }
    let user = std::env::var("USER").ok().or_else(|| home.file_name().map(|n| n.to_string_lossy().into_owned()));
    let space = user.map(|u| Path::new("/var/lib/agx-data/users").join(u)).filter(|p| p.is_dir());
    let mut seen: HashSet<PathBuf> = HashSet::new();
    if let Some(path) = space {
        seen.insert(path.clone());
        found.push(Place { label: "My NVMe space".into(), glyph: "󰋊", path, section: 1 });
    }
    if let Ok(mounts) = fs::read_to_string("/proc/self/mounts") {
        for path in drives(&mounts) {
            if seen.insert(path.clone()) && path != Path::new("/") {
                let label = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                found.push(Place { label, glyph: "󰋊", path, section: 1 });
            }
        }
    }
    found.push(Place { label: "File system".into(), glyph: "󰋊", path: "/".into(), section: 1 });
    found
}

/// Mount points of real block devices (SD card, NVMe, USB), in mount order.
pub(super) fn drives(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (device, point, kind) = (fields.next()?, fields.next()?, fields.next()?);
            let block = ["/dev/mmcblk", "/dev/nvme", "/dev/sd"].iter().any(|p| device.starts_with(p));
            (block && kind != "squashfs" && !point.starts_with("/boot") && !point.starts_with("/var/lib/agx-data/users")).then(|| unescape(point))
        })
        .collect()
}

/// /proc/self/mounts writes space, tab, newline and backslash as octal escapes.
fn unescape(field: &str) -> PathBuf {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = (bytes[i] == b'\\' && i + 3 < bytes.len() + 0)
            .then(|| &bytes[i + 1..i + 4])
            .filter(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)));
        match octal {
            Some(d) => {
                out.push((d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0'));
                i += 4;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    PathBuf::from(OsStr::from_bytes(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_block_device_mounts_and_decodes_escapes() {
        let table = "/dev/mmcblk0p2 / ext4 rw 0 0\n\
                     proc /proc proc rw 0 0\n\
                     /dev/nvme0n1p1 /var/lib/agx-data ext4 rw 0 0\n\
                     /dev/sda1 /media/My\\040Drive vfat rw 0 0\n\
                     /dev/mmcblk0p1 /boot vfat rw 0 0\n";
        assert_eq!(drives(table), [PathBuf::from("/"), "/var/lib/agx-data".into(), "/media/My Drive".into()]);
    }
}

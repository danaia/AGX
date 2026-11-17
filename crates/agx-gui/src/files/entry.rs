//! One directory entry, its classification and its display text.
use std::{fs, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    /// Sockets, devices and pipes: listed, never opened or copied.
    Special,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// The file name, or the path below the search root for search results.
    pub name: String,
    pub kind: Kind,
    pub link: bool,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: i64,
    pub executable: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    pub fn hidden(&self) -> bool {
        let leaf = self.name.rsplit('/').next().unwrap_or(&self.name);
        leaf.starts_with('.')
    }

    /// Reads one entry from its path. A symlink reports the kind of its target but keeps `link`.
    pub fn read(path: &Path, name: String) -> Option<Self> {
        let own = fs::symlink_metadata(path).ok()?;
        let link = own.file_type().is_symlink();
        let meta = if link { fs::metadata(path).unwrap_or(own) } else { own };
        Some(Self::from_metadata(name, link, &meta))
    }

    pub fn from_metadata(name: String, link: bool, meta: &fs::Metadata) -> Self {
        let kind = if meta.is_dir() {
            Kind::Dir
        } else if meta.is_file() {
            Kind::File
        } else {
            Kind::Special
        };
        Self {
            name,
            kind,
            link,
            size: if kind == Kind::File { meta.len() } else { 0 },
            modified: meta.mtime(),
            executable: kind == Kind::File && meta.mode() & 0o111 != 0,
        }
    }

    pub fn extension(&self) -> String {
        let leaf = self.name.rsplit('/').next().unwrap_or(&self.name);
        match leaf.rfind('.') {
            Some(dot) if dot > 0 => leaf[dot + 1..].to_ascii_lowercase(),
            _ => String::new(),
        }
    }

    /// Nerd Font glyph and a colour tone (0 folder, 1 plain, 2 media, 3 code, 4 archive, 5 other).
    pub fn glyph(&self) -> (&'static str, i32) {
        match self.kind {
            Kind::Dir if self.link => ("󰉒", 0),
            Kind::Dir => ("󰉋", 0),
            Kind::Special => ("󰈔", 5),
            Kind::File => match category(&self.extension()) {
                Category::Image => ("󰋩", 2),
                Category::Audio => ("󰝚", 2),
                Category::Video => ("󰕧", 2),
                Category::Archive => ("󰗄", 4),
                Category::Code => ("󰅩", 3),
                Category::Document => ("󰈙", 1),
                Category::Pdf => ("󰈦", 1),
                Category::Other if self.executable => ("󰆍", 3),
                Category::Other => ("󰈔", 5),
            },
        }
    }

    pub fn type_label(&self) -> &'static str {
        match self.kind {
            Kind::Dir if self.link => "Link to folder",
            Kind::Dir => "Folder",
            Kind::Special => "Special file",
            Kind::File => match category(&self.extension()) {
                Category::Image => "Image",
                Category::Audio => "Audio",
                Category::Video => "Video",
                Category::Archive => "Archive",
                Category::Code => "Source code",
                Category::Document => "Document",
                Category::Pdf => "PDF",
                Category::Other if self.executable => "Program",
                Category::Other => "File",
            },
        }
    }
}

#[derive(PartialEq)]
enum Category {
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Document,
    Pdf,
    Other,
}

fn category(extension: &str) -> Category {
    match extension {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "tiff" | "avif" => Category::Image,
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" => Category::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => Category::Video,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "tgz" | "deb" => Category::Archive,
        "rs" | "py" | "c" | "h" | "cpp" | "hpp" | "js" | "ts" | "tsx" | "jsx" | "go" | "java" | "sh" | "bash"
        | "lua" | "rb" | "php" | "html" | "css" | "slint" | "toml" | "json" | "yaml" | "yml" | "sql" | "mk" => {
            Category::Code
        }
        "txt" | "md" | "rst" | "log" | "csv" | "ini" | "conf" | "cfg" | "doc" | "docx" | "odt" | "rtf" => {
            Category::Document
        }
        "pdf" => Category::Pdf,
        _ => Category::Other,
    }
}

/// "1.4 MiB"; a plain byte count below 1 KiB.
pub fn size_text(bytes: u64) -> String {
    crate::system_ui::bytes(bytes)
}

/// Local "YYYY-MM-DD HH:MM", or the empty string for an unknown time.
pub fn time_text(seconds: i64) -> String {
    if seconds == 0 {
        return String::new();
    }
    let time = seconds as libc::time_t;
    // SAFETY: localtime_r writes only into the zeroed tm we pass.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return String::new();
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        )
    }
}

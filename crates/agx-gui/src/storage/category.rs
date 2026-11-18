//! Classify personal cleanup files for browsing. Categories never grant removal access.
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Category {
    #[default]
    All,
    Apps,
    Media,
    Downloads,
    Documents,
    Cache,
    Other,
    Trash,
}

impl Category {
    pub fn from_index(index: i32) -> Option<Self> {
        [
            Self::All,
            Self::Apps,
            Self::Media,
            Self::Downloads,
            Self::Documents,
            Self::Cache,
            Self::Other,
            Self::Trash,
        ]
        .get(usize::try_from(index).ok()?)
        .copied()
    }

    pub fn in_downloads(path: &Path) -> bool {
        in_folder(path, &["Downloads"])
    }

    pub fn includes(self, path: &Path) -> bool {
        if self == Self::Downloads {
            return Self::in_downloads(path);
        }
        Self::of(path) == self
    }

    fn of(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|part| part.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "appimage" | "apk" | "deb" | "rpm" | "msi" | "dmg" | "exe" | "flatpak" | "snap"
            | "nes" | "sfc" | "snes" | "gb" | "gbc" | "gba" | "n64" | "z64" | "nds" => {
                return Self::Apps;
            }
            _ => {}
        }
        if Self::in_downloads(path) {
            return Self::Downloads;
        }
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "heic" | "avif" | "tiff" | "bmp"
            | "mp4" | "mkv" | "mov" | "avi" | "webm" | "mp3" | "wav" | "flac" | "ogg" | "m4a"
            | "aac" => Self::Media,
            "pdf" | "txt" | "md" | "rtf" | "doc" | "docx" | "odt" | "xls" | "xlsx" | "ods"
            | "csv" | "ppt" | "pptx" | "odp" | "epub" => Self::Documents,
            _ if in_folder(path, &["Pictures", "Photos", "Music", "Videos", "Movies"]) => {
                Self::Media
            }
            _ if in_folder(path, &["Documents"]) => Self::Documents,
            _ => Self::Other,
        }
    }
}

fn in_folder(path: &Path, names: &[&str]) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| names.iter().any(|value| name.eq_ignore_ascii_case(value)))
        })
    })
}

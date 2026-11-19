//! Bookmark bar data: importing Google/Chrome bookmarks and the account's saved bar.
//! Google exposes no bookmark API, so the supported sources are Chrome's own `Bookmarks`
//! file and the HTML export from Chrome's Bookmark manager. Nothing is sent over the network.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_DEPTH: usize = 12;
const MAX_URLS: usize = 20_000;
const MAX_URL_LEN: usize = 8192;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Entry {
    pub title: String,
    /// `Some` for a link, `None` for a folder.
    pub url: Option<String>,
    pub children: Vec<Entry>,
}

impl Entry {
    fn link(title: &str, url: &str) -> Option<Self> {
        let url = url.trim();
        let web = url.starts_with("https://") || url.starts_with("http://");
        (web && url.len() <= MAX_URL_LEN).then(|| Self {
            title: title.trim().to_string(),
            url: Some(url.to_string()),
            children: Vec::new(),
        })
    }

    fn folder(title: &str, children: Vec<Entry>) -> Self {
        Self { title: title.trim().to_string(), url: None, children }
    }

    /// Every link below this entry, depth first.
    pub fn links(&self) -> Vec<&Entry> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a Entry>) {
        if self.url.is_some() {
            out.push(self);
        }
        for child in &self.children {
            child.collect(out);
        }
    }

    fn to_json(&self) -> Value {
        match &self.url {
            Some(url) => json!({"title": self.title, "url": url}),
            None => json!({"title": self.title, "children": self.children.iter().map(Entry::to_json).collect::<Vec<_>>()}),
        }
    }

    fn from_json(value: &Value, depth: usize) -> Option<Self> {
        let title = value.get("title").or_else(|| value.get("name"))?.as_str()?;
        if let Some(url) = value.get("url").and_then(Value::as_str) {
            return Self::link(title, url);
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        let children = value.get("children")?.as_array()?;
        Some(Self::folder(
            title,
            children.iter().filter_map(|child| Self::from_json(child, depth + 1)).collect(),
        ))
    }
}

/// Chrome's three roots: the bar's links and folders, "Other bookmarks" and "Mobile bookmarks".
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Bar {
    pub items: Vec<Entry>,
    pub other: Vec<Entry>,
    pub mobile: Vec<Entry>,
}

impl Bar {
    fn all(&self) -> impl Iterator<Item = &Entry> {
        self.items.iter().chain(&self.other).chain(&self.mobile)
    }

    pub fn count(&self) -> usize {
        self.all().map(|item| item.links().len()).sum()
    }

    pub fn contains(&self, url: &str) -> bool {
        self.all().any(|item| item.links().iter().any(|link| link.url.as_deref() == Some(url)))
    }

    /// Adds a link at the end of the bar, or removes every top-level copy. Returns true if added.
    pub fn toggle(&mut self, title: &str, url: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.url.as_deref() != Some(url));
        if self.items.len() != before {
            return false;
        }
        match Entry::link(title, url) {
            Some(entry) => {
                self.items.push(entry);
                true
            }
            None => false,
        }
    }

    pub fn load(path: &Path) -> Option<Self> {
        let text = read_limited(path)?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let list = |key: &str| -> Vec<Entry> {
            value
                .get(key)
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| Entry::from_json(item, 0)).collect())
                .unwrap_or_default()
        };
        value.get("bar")?;
        Some(Self { items: list("bar"), other: list("other"), mobile: list("mobile") })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let list = |items: &[Entry]| items.iter().map(Entry::to_json).collect::<Vec<_>>();
        let text = json!({"bar": list(&self.items), "other": list(&self.other), "mobile": list(&self.mobile)});
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&text)?)?;
        fs::rename(temporary, path)
    }
}

fn read_limited(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= MAX_FILE).then_some(())?;
    fs::read_to_string(path).ok()
}

fn config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

pub(super) fn config_dir() -> Option<PathBuf> {
    config_base().map(|base| base.join("agx"))
}

pub(super) fn store_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("bookmarks.json"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Where this account's Google bookmarks may be: an HTML or Chrome JSON export in Downloads (newest first),
/// then a local Chrome or Chromium profile.
pub(super) fn sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Some(home) = home() else { return found };
    if let Ok(dir) = fs::read_dir(home.join("Downloads")) {
        let mut exports: Vec<_> = dir
            .flatten()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                name.starts_with("bookmarks") && (name.ends_with(".html") || name.ends_with(".htm") || name.ends_with(".json"))
            })
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        exports.sort_by(|a, b| b.0.cmp(&a.0));
        found.extend(exports.into_iter().map(|(_, path)| path));
    }
    let config = config_base().unwrap_or_else(|| home.join(".config"));
    for browser in ["google-chrome", "chromium", "google-chrome-beta"] {
        // Signed-in Chrome keeps synced bookmarks in `AccountBookmarks`; older profiles use `Bookmarks`.
        // Profiles are tried most bookmarks first, so the main Google account wins.
        let mut profiles: Vec<(u64, PathBuf)> = fs::read_dir(config.join(browser))
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|dir| ["AccountBookmarks", "Bookmarks"].map(|name| dir.path().join(name)))
            .filter_map(|path| Some((fs::metadata(&path).ok()?.len(), path)))
            .collect();
        profiles.sort_by(|a, b| b.0.cmp(&a.0));
        found.extend(profiles.into_iter().map(|(_, path)| path));
    }
    found.into_iter().filter(|path| path.is_file()).collect()
}

/// Parses either Chrome's `Bookmarks` JSON or a Netscape-format HTML export.
pub(super) fn import(path: &Path) -> Option<Bar> {
    let text = read_limited(path)?;
    let bar = if text.trim_start().starts_with('{') {
        parse_chrome(&text)?
    } else {
        parse_html(&text)
    };
    (bar.count() > 0).then_some(bar)
}

pub(super) fn parse_chrome(text: &str) -> Option<Bar> {
    let value: Value = serde_json::from_str(text).ok()?;
    let roots = value.get("roots")?;
    let list = |key: &str| -> Vec<Entry> {
        roots
            .get(key)
            .and_then(|root| root.get("children"))
            .and_then(Value::as_array)
            .map(|children| children.iter().filter_map(|c| Entry::from_json(c, 0)).collect())
            .unwrap_or_default()
    };
    Some(assemble(list("bookmark_bar"), list("other"), list("synced")))
}

fn assemble(mut bar: Vec<Entry>, mut other: Vec<Entry>, mut mobile: Vec<Entry>) -> Bar {
    let mut remaining = MAX_URLS;
    for list in [&mut bar, &mut other, &mut mobile] {
        list.retain_mut(|item| trim(item, &mut remaining));
    }
    Bar { items: bar, other, mobile }
}

/// Enforces the total link limit; false when the entry ends up empty.
fn trim(entry: &mut Entry, remaining: &mut usize) -> bool {
    if entry.url.is_some() {
        return match remaining.checked_sub(1) {
            Some(left) => {
                *remaining = left;
                true
            }
            None => false,
        };
    }
    entry.children.retain_mut(|child| trim(child, remaining));
    !entry.children.is_empty()
}

fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(decode(&tag[at..at + end]))
}

pub(super) fn parse_html(text: &str) -> Bar {
    // `to_ascii_lowercase` keeps byte offsets identical, so tag positions index `text` directly.
    let lower = text.to_ascii_lowercase();
    let mut stack: Vec<Entry> = Vec::new();
    let mut pending: Option<(String, bool)> = None;
    let mut toolbar: Vec<Entry> = Vec::new();
    let mut rest: Vec<Entry> = Vec::new();
    let mut toolbar_depth: Option<usize> = None;
    let mut cursor = 0;
    while let Some(open) = lower[cursor..].find('<').map(|i| i + cursor) {
        let Some(close) = lower[open..].find('>').map(|i| i + open) else { break };
        let tag = &text[open..=close];
        let name = lower[open + 1..close].split(|c: char| c.is_whitespace()).next().unwrap_or("").trim_end_matches('/');
        let after = close + 1;
        cursor = after;
        match name {
            "h3" => {
                let end = lower[after..].find("</h3").map(|i| i + after).unwrap_or(after);
                let title = decode(&text[after..end]);
                let is_bar = tag.to_ascii_lowercase().contains("personal_toolbar_folder=\"true\"");
                pending = Some((title, is_bar));
                cursor = end;
            }
            "dl" => {
                if stack.len() >= MAX_DEPTH {
                    break;
                }
                let (title, is_bar) = pending.take().unwrap_or_default();
                if is_bar && toolbar_depth.is_none() {
                    toolbar_depth = Some(stack.len());
                }
                stack.push(Entry::folder(&title, Vec::new()));
            }
            "/dl" => {
                if let Some(done) = stack.pop() {
                    let is_toolbar = toolbar_depth == Some(stack.len());
                    match stack.last_mut() {
                        Some(parent) if !is_toolbar => {
                            if !done.children.is_empty() {
                                parent.children.push(done)
                            }
                        }
                        Some(_) => {
                            toolbar = done.children;
                            toolbar_depth = None;
                        }
                        None => rest.extend(done.children),
                    }
                }
            }
            "a" => {
                let end = lower[after..].find("</a").map(|i| i + after).unwrap_or(after);
                let title = decode(&text[after..end]);
                cursor = end;
                if let (Some(url), Some(parent)) = (attribute(tag, "href"), stack.last_mut()) {
                    if let Some(link) = Entry::link(&title, &url) {
                        parent.children.push(link);
                    }
                }
            }
            _ => {}
        }
    }
    // A top-level "Other bookmarks" folder stays a folder; loose top-level links join the bar.
    let (mut bar, mut other) = (toolbar, Vec::new());
    for entry in rest {
        if entry.url.is_some() {
            bar.push(entry);
        } else {
            other.push(entry);
        }
    }
    assemble(bar, other.into_iter().flat_map(|f| f.children).collect(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><H3 ADD_DATE="1" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://example.org/?a=1&amp;b=2" ADD_DATE="1">Example &amp; Co</A>
        <DT><H3>News</H3>
        <DL><p>
            <DT><A HREF="https://news.test/">News</A>
            <DT><A HREF="javascript:alert(1)">Bad</A>
        </DL><p>
    </DL><p>
    <DT><H3>Other bookmarks</H3>
    <DL><p>
        <DT><A HREF="http://other.test/">Other</A>
    </DL><p>
</DL><p>"#;

    #[test]
    fn html_export_keeps_bar_folders_and_drops_unsafe_links() {
        let bar = parse_html(EXPORT);
        let titles: Vec<_> = bar.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Example & Co", "News"]);
        assert_eq!(bar.other.len(), 1);
        assert_eq!(bar.items[0].url.as_deref(), Some("https://example.org/?a=1&b=2"));
        assert_eq!(bar.items[1].children.len(), 1);
        assert_eq!(bar.count(), 3);
    }

    #[test]
    fn chrome_profile_json_maps_the_three_roots() {
        let json = r#"{"roots":{
            "bookmark_bar":{"children":[{"name":"A","type":"url","url":"https://a.test/"}]},
            "other":{"children":[{"name":"B","type":"url","url":"https://b.test/"}]},
            "synced":{"children":[{"name":"C","type":"url","url":"chrome://settings"}]}}}"#;
        let bar = parse_chrome(json).unwrap();
        assert_eq!((bar.items.len(), bar.other.len(), bar.mobile.len()), (1, 1, 0));
        assert_eq!(bar.count(), 2);
    }

    #[test]
    fn store_round_trips_and_toggle_adds_then_removes() {
        let dir = std::env::temp_dir().join(format!("angi-bookmarks-{}", std::process::id()));
        let path = dir.join("bookmarks.json");
        let mut bar = parse_html(EXPORT);
        assert!(bar.toggle("New", "https://new.test/"));
        bar.save(&path).unwrap();
        let loaded = Bar::load(&path).unwrap();
        assert_eq!(loaded, bar);
        let mut loaded = loaded;
        assert!(!loaded.toggle("New", "https://new.test/"));
        assert!(!loaded.contains("https://new.test/"));
        fs::remove_dir_all(dir).unwrap();
    }
}

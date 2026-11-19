use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

const THEME_KEY: &str = "agenthub.theme";
const NAV_COLLAPSED_KEY: &str = "agenthub.nav-collapsed";
const BUNDLE_IDS: &[&str] = &["com.lich13.agenthub"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyWebviewPrefs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_collapsed: Option<bool>,
}

pub fn read_legacy_webview_prefs(home_dir: &Path) -> LegacyWebviewPrefs {
    let mut candidates = discover_sqlite_candidates(home_dir);
    candidates.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));

    let mut merged = LegacyWebviewPrefs::default();
    for (path, _) in candidates {
        let Ok(prefs) = read_sqlite_prefs(&path) else {
            continue;
        };
        if merged.theme.is_none() {
            merged.theme = prefs.theme;
        }
        if merged.nav_collapsed.is_none() {
            merged.nav_collapsed = prefs.nav_collapsed;
        }
        if merged.theme.is_some() && merged.nav_collapsed.is_some() {
            break;
        }
    }
    merged
}

fn discover_sqlite_candidates(home_dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut paths = Vec::new();
    for bundle_id in BUNDLE_IDS {
        let base = home_dir.join("Library/WebKit").join(bundle_id);
        for direct in [
            base.join("WebsiteData/LocalStorage/tauri_localhost_0.localstorage"),
            base.join("WebsiteData/LocalStorage/http_tauri.localhost_0.localstorage"),
        ] {
            if let Ok(meta) = std::fs::metadata(&direct) {
                if meta.is_file() {
                    paths.push((direct, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                }
            }
        }
        let pattern = base.join("WebsiteData/Default/*/*/LocalStorage/localstorage.sqlite3");
        if let Some(pattern_str) = pattern.to_str() {
            if let Ok(entries) = glob::glob(pattern_str) {
                for entry in entries.flatten() {
                    if let Ok(meta) = std::fs::metadata(&entry) {
                        if meta.is_file() {
                            paths.push((entry, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                        }
                    }
                }
            }
        }
    }
    paths
}

fn decode_utf16le_blob(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) || bytes.len() > 4096 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .collect();
    let stripped = units.strip_prefix(&[0xFEFF]).unwrap_or(&units);
    String::from_utf16(stripped).ok()
}

fn read_sqlite_prefs(path: &Path) -> anyhow::Result<LegacyWebviewPrefs> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stmt =
        conn.prepare("SELECT key, CAST(value AS BLOB) FROM ItemTable WHERE key IN (?1, ?2)")?;
    let mut rows = stmt.query([THEME_KEY, NAV_COLLAPSED_KEY])?;
    let mut out = LegacyWebviewPrefs::default();
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        let Some(decoded) = decode_utf16le_blob(&blob) else {
            continue;
        };
        let trimmed = decoded.trim_matches('\0').trim();
        match key.as_str() {
            THEME_KEY if matches!(trimmed, "system" | "dark" | "light") => {
                out.theme = Some(trimmed.to_string());
            }
            NAV_COLLAPSED_KEY => match trimmed {
                "1" | "true" => out.nav_collapsed = Some(true),
                "0" | "false" => out.nav_collapsed = Some(false),
                _ => {}
            },
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_utf16le(value: &str) -> Vec<u8> {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()
    }

    fn write_webkit_db(path: &Path, entries: &[(&str, Vec<u8>)]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL)",
            [],
        )
        .unwrap();
        for (k, v) in entries {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                (*k, v),
            )
            .unwrap();
        }
    }

    #[test]
    fn reads_utf16le_webkit_localstorage_without_modifying_files() {
        let home = tempfile::tempdir().unwrap();
        let db = home.path().join(
            "Library/WebKit/com.lich13.agenthub/WebsiteData/Default/abcd/efgh/LocalStorage/localstorage.sqlite3",
        );
        write_webkit_db(
            &db,
            &[
                (THEME_KEY, encode_utf16le("dark")),
                (NAV_COLLAPSED_KEY, encode_utf16le("1")),
            ],
        );
        let before = std::fs::read(&db).unwrap();

        let prefs = read_legacy_webview_prefs(home.path());
        assert_eq!(prefs.theme.as_deref(), Some("dark"));
        assert_eq!(prefs.nav_collapsed, Some(true));
        assert_eq!(std::fs::read(&db).unwrap(), before);
    }

    #[test]
    fn falls_back_to_legacy_tauri_localhost_and_ignores_invalid_values() {
        let home = tempfile::tempdir().unwrap();
        let legacy = home.path().join(
            "Library/WebKit/com.lich13.agenthub/WebsiteData/LocalStorage/tauri_localhost_0.localstorage",
        );
        write_webkit_db(
            &legacy,
            &[
                (THEME_KEY, encode_utf16le("invalid-theme")),
                (NAV_COLLAPSED_KEY, encode_utf16le("0")),
            ],
        );
        let prefs = read_legacy_webview_prefs(home.path());
        assert_eq!(prefs.theme, None);
        assert_eq!(prefs.nav_collapsed, Some(false));
    }
}

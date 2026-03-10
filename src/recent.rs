use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 10;

#[derive(Default, Serialize, Deserialize)]
pub struct RecentFiles {
    paths: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn add(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.paths.retain(|p| p != &path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
        self.save();
    }

    pub fn entries(&self) -> &[PathBuf] {
        &self.paths
    }

    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("hyprboard").join("recent.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recent() -> RecentFiles {
        RecentFiles { paths: Vec::new() }
    }

    #[test]
    fn add_moves_to_front() {
        let mut r = make_recent();
        // Bypass save by directly manipulating paths
        r.paths.push(PathBuf::from("/a"));
        r.paths.push(PathBuf::from("/b"));
        // Re-add /b — should move to front
        r.paths.retain(|p| p != Path::new("/b"));
        r.paths.insert(0, PathBuf::from("/b"));
        assert_eq!(r.entries()[0], PathBuf::from("/b"));
        assert_eq!(r.entries()[1], PathBuf::from("/a"));
    }

    #[test]
    fn truncates_at_max() {
        let mut r = make_recent();
        for i in 0..15 {
            r.paths.push(PathBuf::from(format!("/file{i}")));
        }
        r.paths.truncate(MAX_RECENT);
        assert_eq!(r.entries().len(), MAX_RECENT);
    }

    #[test]
    fn no_duplicates() {
        let mut r = make_recent();
        r.paths.push(PathBuf::from("/x"));
        r.paths.push(PathBuf::from("/y"));
        // Add /x again
        r.paths.retain(|p| p != Path::new("/x"));
        r.paths.insert(0, PathBuf::from("/x"));
        assert_eq!(r.entries().len(), 2);
        assert_eq!(r.entries()[0], PathBuf::from("/x"));
    }
}

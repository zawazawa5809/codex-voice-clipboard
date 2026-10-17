//! Where history, settings and logs live (spec 2 "設定・ログの場所").

use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "CodexVoiceClipboard";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub sandbox: Option<PathBuf>,
    pub history: PathBuf,
    pub data_dir: PathBuf,
    pub settings: PathBuf,
    pub log_dir: PathBuf,
    pub log: PathBuf,
}

impl AppPaths {
    pub fn sandbox(root: &Path) -> Self {
        Self {
            sandbox: Some(root.to_path_buf()),
            history: root.join("history.jsonl"),
            data_dir: root.to_path_buf(),
            settings: root.join("settings.json"),
            log_dir: root.to_path_buf(),
            log: root.join("app.log"),
        }
    }

    pub fn real() -> Option<Self> {
        let home = dirs::home_dir()?;
        #[cfg(target_os = "macos")]
        let (data_dir, log_dir) = (
            home.join("Library/Application Support").join(APP_DIR),
            home.join("Library/Logs").join(APP_DIR),
        );
        #[cfg(not(target_os = "macos"))]
        let (data_dir, log_dir) = {
            let dir = dirs::data_local_dir()?.join(APP_DIR);
            (dir.clone(), dir)
        };
        Some(Self {
            sandbox: None,
            history: home.join(".codex").join("transcription-history.jsonl"),
            settings: data_dir.join("settings.json"),
            log: log_dir.join("app.log"),
            data_dir,
            log_dir,
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_keeps_everything_inside_the_root() {
        let paths = AppPaths::sandbox(Path::new("/tmp/root"));
        for path in [&paths.history, &paths.settings, &paths.log, &paths.data_dir, &paths.log_dir] {
            assert!(path.starts_with("/tmp/root"), "{path:?}");
        }
    }

    #[test]
    fn real_history_is_the_codex_file_in_home() {
        let paths = AppPaths::real().unwrap();
        assert!(paths.history.ends_with(".codex/transcription-history.jsonl"));
        assert!(paths.settings.ends_with("CodexVoiceClipboard/settings.json"));
    }
}

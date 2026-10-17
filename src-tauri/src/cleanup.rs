//! What `--uninstall` and the menu item remove (spec 1.9, 2 "削除").

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cvc_core::startup::Autostart;
use cvc_core::uninstall::UninstallOperations;

use crate::paths::AppPaths;
use crate::sandbox::remove_if_exists;

pub struct Cleanup<'a> {
    pub paths: &'a AppPaths,
    pub autostart: &'a mut dyn Autostart,
    /// Stops the resident's watcher and pending copy. Returns whether it stopped.
    pub stop: &'a mut dyn FnMut() -> bool,
    /// Only the menu's アンインストール deletes the app itself; `--uninstall` is the
    /// cleanup the OS uninstaller calls before it removes the files (spec 1.9, 2).
    pub delete_app: bool,
}

impl UninstallOperations for Cleanup<'_> {
    fn stop_resident(&mut self) -> bool {
        (self.stop)()
    }

    fn disable_startup(&mut self) -> io::Result<()> {
        if self.autostart.is_enabled()? {
            self.autostart.set_enabled(false)?;
        }
        Ok(())
    }

    /// Only files this app wrote. History, the clipboard file and the sandbox root stay.
    fn remove_data(&mut self) -> io::Result<()> {
        let p = self.paths;
        for path in [p.settings.clone(), with_suffix(&p.settings, ".tmp"), p.log.clone(), with_suffix(&p.log, ".1")] {
            remove_if_exists(&path)?;
        }
        if p.sandbox.is_none() {
            // Directories are removed only once empty; anything else in them is not ours.
            let _ = fs::remove_dir(&p.data_dir);
            let _ = fs::remove_dir(&p.log_dir);
        }
        Ok(())
    }

    fn delete_application(&mut self) -> io::Result<()> {
        if !self.delete_app {
            return Ok(());
        }
        let target = match &self.paths.sandbox {
            // Stands in for the installed `.app`.
            Some(root) => Some(root.join("app")).filter(|p| p.exists()),
            None => installed_bundle_of_current_exe(),
        };
        match target {
            Some(path) => fs::remove_dir_all(path),
            None => Ok(()),
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// macOS deletes its own `.app`; Windows leaves file removal to the NSIS uninstaller.
fn installed_bundle_of_current_exe() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    installed_bundle(&std::env::current_exe().ok()?, &dirs::home_dir()?)
}

/// The `.app` containing `exe`, but only when it sits directly in `/Applications` or
/// `~/Applications`. A development build under `target/` is never deleted.
pub fn installed_bundle(exe: &Path, home: &Path) -> Option<PathBuf> {
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?;
    if !exe.starts_with(bundle.join("Contents/MacOS")) {
        return None;
    }
    let parent = bundle.parent()?;
    (parent == Path::new("/Applications") || parent == home.join("Applications")).then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxAutostart;
    use cvc_core::uninstall;

    #[test]
    fn sandbox_cleanup_removes_only_what_the_app_created() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let paths = AppPaths::sandbox(root);
        for name in ["settings.json", "app.log", "app.log.1", "startup.txt", "history.jsonl", "clipboard.txt"] {
            fs::write(root.join(name), "x").unwrap();
        }
        fs::create_dir_all(root.join("app/Contents")).unwrap();
        let mut autostart = SandboxAutostart::new(root);
        let mut stopped = false;
        let mut stop = || {
            stopped = true;
            true
        };
        uninstall::execute(&mut Cleanup { paths: &paths, autostart: &mut autostart, stop: &mut stop, delete_app: true })
            .unwrap();
        assert!(stopped);
        for gone in ["settings.json", "app.log", "app.log.1", "startup.txt", "app"] {
            assert!(!root.join(gone).exists(), "{gone} remains");
        }
        for kept in ["history.jsonl", "clipboard.txt"] {
            assert!(root.join(kept).exists(), "{kept} was removed");
        }
    }

    #[test]
    fn cleanup_without_delete_app_keeps_the_application() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let paths = AppPaths::sandbox(root);
        fs::write(root.join("settings.json"), "{}").unwrap();
        fs::create_dir_all(root.join("app/Contents")).unwrap();
        let mut autostart = SandboxAutostart::new(root);
        let mut stop = || true;
        uninstall::execute(&mut Cleanup { paths: &paths, autostart: &mut autostart, stop: &mut stop, delete_app: false })
            .unwrap();
        assert!(!root.join("settings.json").exists());
        assert!(root.join("app/Contents").exists());
    }

    #[test]
    fn only_bundles_installed_in_an_applications_folder_are_deleted() {
        let home = Path::new("/Users/me");
        let exe = |p: &str| PathBuf::from(p).join("Contents/MacOS/codex-voice-clipboard");
        assert_eq!(
            Some(PathBuf::from("/Applications/Codex Voice Clipboard.app")),
            installed_bundle(&exe("/Applications/Codex Voice Clipboard.app"), home)
        );
        assert_eq!(
            Some(PathBuf::from("/Users/me/Applications/C.app")),
            installed_bundle(&exe("/Users/me/Applications/C.app"), home)
        );
        assert_eq!(None, installed_bundle(&exe("/Users/me/dev/target/release/bundle/macos/C.app"), home));
        assert_eq!(None, installed_bundle(Path::new("/Users/me/dev/target/release/codex-voice-clipboard"), home));
        assert_eq!(None, installed_bundle(Path::new("/Applications/C.app/Contents/Resources/x"), home));
    }
}

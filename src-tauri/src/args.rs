//! Command-line arguments (spec 1.9).

use std::path::PathBuf;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Replace history, settings, clipboard, login item and app registration with files here.
    pub sandbox: Option<PathBuf>,
    /// Sandbox only: poll without change notifications.
    pub no_watcher: bool,
    /// Remove what the app created (login item, settings, logs).
    pub uninstall: bool,
    /// Skip the confirmation. Used by the Windows uninstaller, which already asked.
    pub assume_yes: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ArgError;

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, ArgError> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sandbox" => {
                let path = PathBuf::from(args.next().ok_or(ArgError)?);
                if !path.is_absolute() || parsed.sandbox.is_some() {
                    return Err(ArgError);
                }
                parsed.sandbox = Some(path);
            }
            "--no-watcher" => parsed.no_watcher = true,
            "--uninstall" => parsed.uninstall = true,
            "--assume-yes" => parsed.assume_yes = true,
            // Finder adds a process serial number on some macOS versions.
            other if other.starts_with("-psn_") => {}
            _ => return Err(ArgError),
        }
    }
    if parsed.no_watcher && parsed.sandbox.is_none() {
        return Err(ArgError);
    }
    if parsed.assume_yes && !parsed.uninstall {
        return Err(ArgError);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, ArgError> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_is_the_real_resident() {
        assert_eq!(Ok(Args::default()), p(&[]));
    }

    #[test]
    fn sandbox_needs_an_absolute_path() {
        assert_eq!(Some(PathBuf::from("/tmp/x")), p(&["--sandbox", "/tmp/x"]).unwrap().sandbox);
        assert_eq!(Err(ArgError), p(&["--sandbox", "relative"]));
        assert_eq!(Err(ArgError), p(&["--sandbox"]));
    }

    #[test]
    fn no_watcher_is_sandbox_only() {
        assert_eq!(Err(ArgError), p(&["--no-watcher"]));
        assert!(p(&["--sandbox", "/tmp/x", "--no-watcher"]).unwrap().no_watcher);
    }

    #[test]
    fn assume_yes_is_uninstall_only_and_unknown_flags_fail() {
        assert_eq!(Err(ArgError), p(&["--assume-yes"]));
        assert!(p(&["--uninstall", "--assume-yes"]).unwrap().assume_yes);
        assert_eq!(Err(ArgError), p(&["--install"]));
        assert_eq!(Ok(Args::default()), p(&["-psn_0_12345"]));
    }
}

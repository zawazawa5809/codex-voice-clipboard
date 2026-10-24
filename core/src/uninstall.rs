//! Cleanup ordering for `--uninstall` and the menu item (spec 1.9, 2 "削除").

use std::io;

pub trait UninstallOperations {
    /// Stop watching, timers, and pending copies. `false` aborts the removal.
    fn stop_resident(&mut self) -> bool;
    fn disable_startup(&mut self) -> io::Result<()>;
    /// Settings and logs the app created.
    fn remove_data(&mut self) -> io::Result<()>;
    fn delete_application(&mut self) -> io::Result<()>;
}

/// Stop first, so nothing re-creates the log or settings while they are being removed;
/// delete the application last, so a failure earlier leaves a runnable app to retry with.
pub fn execute(ops: &mut impl UninstallOperations) -> io::Result<()> {
    if !ops.stop_resident() {
        return Err(io::Error::other("resident did not stop; removal cancelled"));
    }
    ops.disable_startup()?;
    ops.remove_data()?;
    ops.delete_application()
}

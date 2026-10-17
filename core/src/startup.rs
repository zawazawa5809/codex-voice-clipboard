//! Login-item switch over an injected store (spec 1.6, 2).

use std::io;

/// OS login-item registration. Real and sandbox implementations live in the app crate.
pub trait Autostart {
    fn is_enabled(&self) -> io::Result<bool>;
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()>;
    /// Whether the user switched the item off in OS settings. `None` when unknown.
    fn disabled_by_os(&self) -> Option<bool>;
}

pub struct StartupController<A: Autostart> {
    store: A,
}

impl<A: Autostart> StartupController<A> {
    pub fn new(store: A) -> Self {
        Self { store }
    }
    pub fn store(&self) -> &A {
        &self.store
    }
    pub fn store_mut(&mut self) -> &mut A {
        &mut self.store
    }
    pub fn enabled(&self) -> io::Result<bool> {
        self.store.is_enabled()
    }
    /// Writes only when the state differs, then returns the state read back.
    pub fn set_enabled(&mut self, enabled: bool) -> io::Result<bool> {
        if self.store.is_enabled()? != enabled {
            self.store.set_enabled(enabled)?;
        }
        self.store.is_enabled()
    }
}

impl<T: Autostart + ?Sized> Autostart for Box<T> {
    fn is_enabled(&self) -> io::Result<bool> {
        (**self).is_enabled()
    }
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        (**self).set_enabled(enabled)
    }
    fn disabled_by_os(&self) -> Option<bool> {
        (**self).disabled_by_os()
    }
}

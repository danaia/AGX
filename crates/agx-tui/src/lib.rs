//! Interactive AGX system console. Public options and session entry point.
mod app;
mod palette;
mod profile;
mod remote_access;
mod runtime;
mod shell;
mod services;
mod system;
mod terminal;
mod wifi;
mod worker;

use palette::Palette;
pub use runtime::run;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorMode {
    Auto,
    TrueColor,
    Indexed,
    Basic,
    Never,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub color: ColorMode,
    pub ascii: bool,
    pub mouse: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            color: ColorMode::Auto,
            ascii: false,
            mouse: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Quit,
    Gui,
}

#[cfg(test)]
mod tests;

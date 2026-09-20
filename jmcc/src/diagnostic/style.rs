//! ANSI styling and color support for pretty diagnostics.

use std::fmt::Display;

/// Configuration for colored terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorConfig {
    pub enabled: bool,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            enabled: Self::detect_color_support(),
        }
    }
}

impl ColorConfig {
    /// Creates a color configuration with explicit enabled state.
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Detects whether terminal colors should be enabled based on environment.
    ///
    /// Respects the `NO_COLOR` standard (<https://no-color.org>) and `TERM=dumb`.
    #[must_use]
    pub fn detect_color_support() -> bool {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if let Ok(term) = std::env::var("TERM")
            && term == "dumb"
        {
            return false;
        }
        true
    }

    /// Renders bold red text (for error headers and primary spans).
    #[must_use]
    pub fn red<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[1;31m", text)
    }

    /// Renders bold yellow text (for warning headers).
    #[must_use]
    pub fn yellow<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[1;33m", text)
    }

    /// Renders bold cyan/green text (for help headers).
    #[must_use]
    pub fn cyan<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[1;36m", text)
    }

    /// Renders bold blue text (for gutter line numbers and delimiters).
    #[must_use]
    pub fn blue<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[1;34m", text)
    }

    /// Renders bold text.
    #[must_use]
    pub fn bold<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[1m", text)
    }

    /// Renders dim/secondary text.
    #[must_use]
    pub fn dim<T: Display>(&self, text: T) -> String {
        self.apply("\x1b[2m", text)
    }

    fn apply<T: Display>(&self, code: &str, text: T) -> String {
        if self.enabled {
            format!("{code}{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

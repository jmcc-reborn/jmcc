//! Переменные программы: глобальные, `save` и хранилище области видимости.
//!
//! Чтение и запись файла `save` — здесь же.

use std::path::Path;

use super::*;
use crate::value::Vars;

impl<'a> Runtime<'a> {
    /// The store of global variables (`game`).
    #[must_use]
    pub const fn global(&self) -> &Shared<'a> {
        &self.global
    }

    /// The store of `save` variables.
    #[must_use]
    pub const fn save(&self) -> &Shared<'a> {
        &self.save
    }

    /// Creates a global variable before the code runs.
    #[tracing::instrument(level = "debug", skip(self, name), fields(value = ?value))]
    pub fn set_global(&mut self, name: impl Into<String>, value: Rt<'a>) {
        self.global.set(&name.into(), value);
    }

    /// Creates a `save` variable before the code runs.
    #[tracing::instrument(level = "debug", skip(self, name), fields(value = ?value))]
    pub fn set_save(&mut self, name: impl Into<String>, value: Rt<'a>) {
        self.save.set(&name.into(), value);
    }

    /// Writes the `save` variables to the file from [`Config::save_file`].
    ///
    /// Does nothing if no file is set: the runtime does not decide when the
    /// program ends — that is the runtime owner's decision.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Serialize`] if a value does not serialize, and
    /// [`RuntimeError::Io`] if the file cannot be written.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn persist_save(&mut self) -> Result<()> {
        let Some(path) = self.config.save_file.clone() else {
            return Ok(());
        };
        let context = format!("save variables -> {}", path.display());
        let text = serde_json::to_string_pretty(&self.save.snapshot()).map_err(|source| {
            RuntimeError::Serialize {
                context: context.clone(),
                source,
            }
        })?;
        std::fs::write(&path, text).map_err(|source| RuntimeError::Io { context, source })
    }

    /// Replaces the `save` variables with the contents of the file from
    /// [`Config::save_file`].
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Io`] if the file exists but cannot be read, and
    /// [`RuntimeError::Json`] if its contents do not parse.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn load_save(&mut self) -> Result<()> {
        let Some(path) = self.config.save_file.clone() else {
            return Ok(());
        };
        self.save.restore(load_save(&path)?);
        Ok(())
    }

    /// The variable store of the requested scope.
    ///
    /// A [`Shared`] is returned — another reference to the same store, not a
    /// borrow. Holding a borrow for the duration of the code is not an option:
    /// the code writes to those same variables.
    #[tracing::instrument(level = "debug", skip(self, stream, scope))]
    pub(crate) fn scope_store(
        &self,
        stream: &Stream<'a>,
        scope: jmcdata::module::VariableScope,
    ) -> Shared<'a> {
        use jmcdata::module::VariableScope;
        match scope {
            VariableScope::Line => stream.line.clone(),
            VariableScope::Local => stream.local.clone(),
            VariableScope::Global => self.global.clone(),
            VariableScope::Save => self.save.clone(),
        }
    }
}

/// Reads the `save` variable file.
///
/// The file's string is leaked. [`Value`] does not borrow from its source:
/// `Cow` is parsed as `Owned`, by copy. But the generated `Deserialize` declares
/// the bound `'de: 'a`, that is, it requires the input to live at least as long
/// as `'a`, and `'a` here is the module's lifetime. The leak happens once, when
/// the runtime is created, and is bounded by the file's size; the alternative is
/// to rewrite `Value`'s parsing by hand, that is, to introduce a second data
/// model, which is exactly what the mock does not do.
#[tracing::instrument(level = "debug", skip(path))]
pub(super) fn load_save<'a>(path: &Path) -> Result<Vars<'a>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // A missing file is the first run, not an error.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vars::new()),
        Err(source) => {
            return Err(RuntimeError::Io {
                context: format!("save variables <- {}", path.display()),
                source,
            });
        }
    };
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    serde_json::from_str(leaked).map_err(|source| RuntimeError::Json {
        context: format!("save variables <- {}", path.display()),
        source,
    })
}

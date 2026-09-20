//! Вход в обработчики извне планировщика: вызов функции и запуск процесса.
//!
//! Внутри исполнения те же две вещи делает планировщик: `call_function` —
//! кадром [`Continuation::Call`](crate::run), `start_process` — постановкой
//! новой задачи. Здесь — точки входа для того, кто пользуется моком как
//! библиотекой: они ставят задачу и прокручивают планировщик.

use std::cell::RefCell;
use std::rc::Rc;

use super::scheduler::Completion;
use super::*;

impl<'a> Runtime<'a> {
    /// Calls a module function by name — the same way the `call_function` action
    /// does.
    ///
    /// `args` are the parameter values by name. What is returned is what the
    /// function left in the local variable [`RETURN_VARIABLE`].
    ///
    /// The function runs as its own task, so a `wait` inside it suspends it and
    /// lets the world clock move on. If it is still asleep when
    /// [`Config::tick_limit`] runs out, the value is not ready yet and the call
    /// reports [`RuntimeError::Suspended`] rather than an empty value.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownFunction`] if there is no such function,
    /// any runtime error from its body, and [`RuntimeError::Suspended`] if the
    /// tick budget ended first.
    #[tracing::instrument(level = "debug", skip(self), fields(name = %name, args = ?args))]
    pub fn call_function(&mut self, name: &str, args: Vec<(String, Rt<'a>)>) -> Result<Rt<'a>> {
        let index = self.program.function_index(name)?;
        let origin = format!("function {name}");
        let targets = self.resolve_targets(Vec::new());
        let stream = spawn_stream(targets, origin);
        let completion: Completion<'a> = Rc::new(RefCell::new(None));
        self.enqueue_handler(stream, index, by_value(args), Some(Rc::clone(&completion)))?;
        self.run_scheduler()?;
        let value = completion.borrow_mut().take();
        value.ok_or_else(|| RuntimeError::Suspended {
            name: name.to_owned(),
            limit: self.config.tick_limit,
        })
    }

    /// Starts a module process by name.
    ///
    /// The process becomes its own task and does not block the caller. The
    /// scheduler is drained here so that a process started from the outside
    /// library API actually runs; inside a handler `start_process` only enqueues.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownProcess`] if there is no such process, and
    /// any runtime error from its body.
    #[tracing::instrument(level = "debug", skip(self), fields(name = %name, args = ?args))]
    pub fn start_process(&mut self, name: &str, args: Vec<(String, Rt<'a>)>) -> Result<()> {
        let index = self.program.process_index(name)?;
        let origin = format!("process {name}");
        let targets = self.resolve_targets(Vec::new());
        let stream = spawn_stream(targets, origin);
        self.enqueue_handler(stream, index, by_value(args), None)?;
        self.run_scheduler()
    }
}

/// Wraps the arguments of the library entry points as plain values.
///
/// A caller of the library has no frame to point a `variable`-typed parameter at,
/// so every argument is a value.
#[tracing::instrument(level = "debug", skip(args))]
fn by_value<'a>(args: Vec<(String, Rt<'a>)>) -> Vec<(String, Bound<'a>)> {
    args.into_iter()
        .map(|(name, value)| (name, Bound::Value(value)))
        .collect()
}

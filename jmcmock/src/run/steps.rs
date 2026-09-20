//! Шаги исполнения и журнал: счётчик шагов, его предел и запись в журнал.
//!
//! Сюда же относится реакция на то, чего мок не умеет: необработанное
//! действие по умолчанию останавливает прогон.

use jmcdata::generated::ActionId;

use super::*;
use crate::actions::code;
use crate::config::Unimplemented;
use crate::interp::Args;

impl<'a> Runtime<'a> {
    /// How many operations the runtime has run since it was created.
    #[must_use]
    pub const fn steps(&self) -> usize {
        self.steps
    }

    /// The limit [`Self::steps`] is compared against — what `%cpu_usage%` is
    /// reported as a percentage of.
    #[must_use]
    pub const fn step_limit(&self) -> usize {
        self.config.step_limit
    }

    /// Resets the step counter without touching the variables or the world.
    pub const fn reset_steps(&mut self) {
        self.steps = 0;
    }

    /// The next step of execution: counts steps and watches the limit.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::StepLimitExceeded`] when the program has run too
    /// many operations: the mock has no server to interrupt an endless loop at
    /// the end of a tick.
    pub(crate) const fn step(&mut self) -> Result<()> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.config.step_limit {
            return Err(RuntimeError::StepLimitExceeded {
                limit: self.config.step_limit,
            });
        }
        Ok(())
    }

    /// The reaction to something the mock cannot do: an action from the `JustMC`
    /// schema, an `if_*` condition, a `%math()` expression.
    ///
    /// Passing it over silently is not allowed: an action that was supposed to
    /// return a value would give a wrong result with no error at all. So by
    /// default it is an error, and [`Unimplemented::Record`] and
    /// [`Unimplemented::Ignore`] are enabled deliberately.
    ///
    /// `what` is substituted into the message whole, so the caller writes it
    /// together with the article: `format!("action '{}'", schema::name(op.action))`.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Unimplemented`] in the [`Unimplemented::Error`]
    /// mode.
    #[tracing::instrument(level = "debug", skip(self, what))]
    pub(crate) fn unimplemented(&mut self, what: impl Into<String>) -> Result<()> {
        let what = what.into();
        match self.config.unimplemented {
            Unimplemented::Error => Err(RuntimeError::Unimplemented { what }),
            Unimplemented::Record => {
                self.log(format!("<not implemented: {what}>"));
                Ok(())
            }
            Unimplemented::Ignore => Ok(()),
        }
    }

    /// Declares an action of the `JustMC` schema unimplemented: the group it
    /// belongs to knows no semantics for it.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Unimplemented`] in the [`Unimplemented::Error`]
    /// mode.
    #[tracing::instrument(level = "debug", skip(self), fields(action = ?action))]
    pub(crate) fn unimplemented_action(&mut self, action: ActionId) -> Result<Flow> {
        self.unimplemented(format!("action '{}'", crate::schema::name(action)))?;
        Ok(Flow::Continue)
    }

    /// Declares an operation unimplemented and, when the run continues, empties
    /// the variables it was to write.
    ///
    /// The action did nothing, so what it was to produce holds nothing rather
    /// than being absent. Otherwise one unknown action turns into `variable …
    /// does not exist` further along, and that message points at the wrong
    /// place: `variable::ray_trace_result` is what leaves
    /// `variable_for_hit_block_location` empty.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Unimplemented`] in the [`Unimplemented::Error`]
    /// mode.
    #[tracing::instrument(level = "debug", skip(self, stream, op))]
    pub(crate) fn unimplemented_op(&mut self, stream: &Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
        self.unimplemented_action(op.action)?;
        let Some(def) = crate::schema::def(op.action) else {
            return Ok(Flow::Continue);
        };
        let args = Args::of(op);
        let primary = def
            .args
            .iter()
            .any(|arg| arg.id == "variable")
            .then_some("variable");
        let slots = primary
            .into_iter()
            .chain(def.assign.into_iter().flatten().map(|arg| arg.id));
        for name in slots {
            let Ok(Some((scope, target))) = code::optional_target(self, stream, args, name) else {
                continue;
            };
            self.scope_store(stream, scope).set(&target, None);
        }
        Ok(Flow::Continue)
    }

    /// Writes a line to the world's journal.
    #[tracing::instrument(level = "debug", skip(self, entry))]
    pub(crate) fn log(&mut self, entry: impl Into<String>) {
        self.world.log_mut().push(entry);
    }
}

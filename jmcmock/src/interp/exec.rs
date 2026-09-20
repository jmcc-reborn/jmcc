//! Общие части исполнения: условия, ветвления-выборки и тело операции.
//!
//! Блоки, ветвления и контейнеры ведёт планировщик
//! ([`crate::run::Runtime::run_scheduler`]): у него явный стек продолжений и
//! операция может приостановиться на `control_wait`. Сюда перенесено то, что
//! не зависит от того, кто именно обходит дерево операций:
//!
//! * [`Runtime::condition`] — вычисление условия ветвления;
//! * [`Runtime::exec_conditional`] — условные *выборки*
//!   (`select_*_by_conditional`, `select_filter_by_conditional`): это не
//!   контейнеры с телом, а операции, меняющие выделение;
//! * [`body_of`] — тело операции;
//! * [`is_branch`] — ветвление ли это (`ActionDef::boolean` из схемы).

use super::*;

impl<'a> Runtime<'a> {
    /// Runs a container whose condition is written separately:
    /// `select_filter_by_conditional` and the `select_*_by_conditional` family.
    ///
    /// `repeat_while` is not here: it is a repeat container and the scheduler
    /// owns it, because its body can suspend.
    #[tracing::instrument(level = "debug", skip(self, stream, op, conditional))]
    pub(crate) fn exec_conditional(
        &mut self,
        stream: &mut Stream<'a>,
        op: &'a Op<'a>,
        conditional: Conditional,
    ) -> Result<Flow> {
        match op.action {
            ActionId::SelectFilterByConditional => self.exec_select_filter(stream, op, conditional),
            ActionId::SelectPlayerByConditional
            | ActionId::SelectEntityByConditional
            | ActionId::SelectAddPlayerByConditional
            | ActionId::SelectAddEntityByConditional => {
                crate::actions::select::conditional(self, stream, op, conditional)
            }
            other => {
                self.unimplemented(format!(
                    "the conditional container '{}'",
                    schema::name(other)
                ))?;
                Ok(Flow::Continue)
            }
        }
    }

    /// Narrows the selection to those for whom the condition is true.
    ///
    /// This is not a block: `select_filter_by_conditional` carries no body but
    /// changes the current selection, and the following code already works with
    /// it. The condition is checked separately for each target, so while it is
    /// checked the stream's target is swapped for the one being tested —
    /// otherwise `if_player_is_near` would look at the same target for all of
    /// them.
    #[tracing::instrument(level = "debug", skip(self, stream, op, conditional))]
    fn exec_select_filter(
        &mut self,
        stream: &mut Stream<'a>,
        op: &'a Op<'a>,
        conditional: Conditional,
    ) -> Result<Flow> {
        let candidates = std::mem::take(&mut stream.selection);
        let mut survivors = Vec::with_capacity(candidates.len());
        for target in candidates {
            stream.targets = vec![target];
            if self.condition(stream, conditional.action, op)? != conditional.is_inverted {
                survivors.push(target);
            }
        }
        stream.selection.clone_from(&survivors);
        stream.targets.clone_from(&survivors);
        Ok(Flow::Continue)
    }

    /// Evaluates a branching action's condition.
    ///
    /// `args` come from `op`, while the action name comes from `condition`: for
    /// `if_*` they coincide, for conditional containers they do not. Anything the
    /// mock cannot do stops execution rather than counting as false: a silent
    /// false would send the program down a branch the server would not have
    /// taken.
    #[tracing::instrument(level = "debug", skip(self, stream, op), fields(condition = ?condition))]
    pub(crate) fn condition(
        &mut self,
        stream: &mut Stream<'a>,
        condition: ActionId,
        op: &'a Op<'a>,
    ) -> Result<bool> {
        // The `variable` conditions are a group of their own: they need nothing
        // from the world, and keeping them in one file is what keeps this match
        // readable.
        if crate::actions::variable::condition::handles(condition) {
            return crate::actions::variable::condition::evaluate(self, stream, condition, op);
        }
        let args = Args::of_condition(op, condition);
        match condition {
            ActionId::IfPlayerChatMessageEquals => {
                let actual = crate::text::strip_legacy_codes(&stream.chat_message);
                Ok(self
                    .values_arg(stream, args, "chat_messages")?
                    .iter()
                    .any(|candidate| {
                        crate::text::strip_legacy_codes(&value::display(candidate)) == actual
                    }))
            }
            ActionId::IfPlayerIsNear => self.player_is_near(stream, args),
            other => {
                self.unimplemented(format!("the condition '{}'", schema::name(other)))?;
                Ok(false)
            }
        }
    }

    /// Checks `if_player_is_near`: the distance from the frame's target to a
    /// location.
    #[tracing::instrument(level = "debug", skip(self, stream, args))]
    fn player_is_near(&mut self, stream: &mut Stream<'a>, args: Args<'a>) -> Result<bool> {
        let range = self.number_arg(stream, args, "range")?;
        let [x, y, z, _, _] = self.location_arg(stream, args, "location")?;
        let ignore_y_axis = matches!(
            self.optional_enum_arg(stream, args, "ignore_y_axis")?,
            Some("TRUE")
        );
        let target = self.primary(stream)?;
        let from = self
            .world()
            .position_of(target)
            .ok_or_else(|| RuntimeError::NoTarget {
                context: stream.origin.clone(),
            })?;
        let mut to = Position::coords(x, y, z);
        if ignore_y_axis {
            to.y = from.y;
        }
        Ok(from.distance(to) <= range)
    }
}

/// An operation's body. A container without a body has an empty body, not a
/// missing one.
#[tracing::instrument(level = "debug", skip(op))]
pub fn body_of<'a>(op: &'a Op<'a>) -> &'a [Op<'a>] {
    op.operations.as_deref().unwrap_or_default()
}

/// Whether an action is a condition: `if_*` are branches rather than effects.
///
/// The flag comes from the schema (`ActionDef::boolean`) rather than from the
/// name: that way the list of conditions stays the same as the compiler's and
/// does not drift away from it.
#[tracing::instrument(level = "debug", fields(action = ?action))]
pub fn is_branch(action: ActionId) -> bool {
    schema::def(action).is_some_and(|definition| definition.boolean)
}

/// Whether a value matches the type from `if_variable_is_type`.
///
/// An empty variable has no type, so it matches nothing: that is false rather
/// than an error — `if_variable_is_type` is asking about the type.
#[tracing::instrument(level = "debug", fields(value = ?value, wanted = %wanted))]
pub fn matches_type(value: &Rt<'_>, wanted: &str) -> bool {
    value
        .as_ref()
        .is_some_and(|value| wanted.eq_ignore_ascii_case(value.type_name().as_ref()))
}

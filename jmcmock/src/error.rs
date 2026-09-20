//! Runtime errors.
//!
//! The compiler checks everything that can be checked statically: types, names,
//! action arguments. This module describes what cannot be: a missing variable,
//! a function that does not exist, an action whose semantics the mock does not
//! know. Such situations have to be found at the moment they happen and turned
//! into an error, not into a silent skip.

use jmcdata::module::VariableScope;
use thiserror::Error;

/// What `controller_exception` catches, and the kind a raised exception has.
///
/// One type covers both roles because the schema names them with one
/// enumeration. `control_call_exception` raises only `WARNING`, `ERROR` or
/// `FATAL`; `controller_exception` is configured with `ALL`, `ERROR` or
/// `WARNING`, and `ALL` is a catcher's value — nothing raises it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExceptionKind {
    /// Any catchable exception: `controller_exception` with the type `ALL`.
    All,
    /// A warning. Caught by `controller_exception` with the type `ALL` or
    /// `WARNING`.
    Warning,
    /// An error. Caught by `controller_exception` with the type `ALL` or
    /// `ERROR`.
    Error,
    /// A critical error. Caught by nothing and stops the thread.
    Fatal,
}

impl ExceptionKind {
    /// Parses the `type` of `control_call_exception` and the `exception_type` of
    /// `controller_exception`.
    #[must_use]
    #[tracing::instrument(level = "trace", fields(raw = %raw))]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "ALL" => Some(Self::All),
            "WARNING" => Some(Self::Warning),
            "ERROR" => Some(Self::Error),
            "FATAL" => Some(Self::Fatal),
            _ => None,
        }
    }
}

/// The handlers a "no such function/process" error can offer instead of the
/// requested name.
///
/// It carries the difference between the two ways a lookup fails. Either the
/// module declares nothing of the sort, and the list is everything it does
/// declare — a typo is then visible at a glance. Or the short name matched
/// several mangled handlers (edition 2026 writes them as `module::name`), and the
/// list is those handlers: the message says so, because the fix is to name one
/// of them in full.
#[derive(Debug, Clone, Default)]
pub struct Candidates {
    /// The full names, as the module declares them.
    names: Vec<String>,
    /// Whether several of them matched the requested short name.
    ambiguous: bool,
}

/// How many names the hint lists before it stops: a module can declare hundreds,
/// and what matters is the first few — or the whole list of matches, which is
/// short by construction.
const CANDIDATE_LIMIT: usize = 8;

impl Candidates {
    /// The module declares nothing with the requested name.
    #[must_use]
    pub const fn declared(names: Vec<String>) -> Self {
        Self {
            names,
            ambiguous: false,
        }
    }

    /// The requested short name matches every one of `names`.
    #[must_use]
    pub const fn matches(names: Vec<String>) -> Self {
        Self {
            names,
            ambiguous: true,
        }
    }
}

impl std::fmt::Display for Candidates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.names.is_empty() {
            return Ok(());
        }
        let listed = self
            .names
            .iter()
            .take(CANDIDATE_LIMIT)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("', '");
        let rest = self.names.len().saturating_sub(CANDIDATE_LIMIT);
        if self.ambiguous {
            write!(f, "; it matches several handlers: '{listed}'")?;
            if rest > 0 {
                write!(f, " and {rest} more")?;
            }
            write!(f, " — use the full name")
        } else {
            write!(f, "; the module declares: '{listed}'")?;
            if rest > 0 {
                write!(f, " and {rest} more")?;
            }
            Ok(())
        }
    }
}

/// Everything that can go wrong while a module runs.
///
/// The errors fall into two groups. The first is what a `.jc` author can catch
/// with `controller_exception`: [`Self::UndefinedVariable`],
/// [`Self::UnexpectedValue`] and the like. The second cannot be caught:
/// [`Self::StepLimitExceeded`], [`Self::Unimplemented`],
/// [`Self::UnknownFunction`] and [`Self::MisplacedElse`] mean the module itself
/// is inconsistent.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The variable is not in its scope. The compiler does not let that through
    /// inside a module, but a module can read a variable nobody created — after
    /// `set_variable_purge`, or in a branch where the assignment never ran.
    #[error("variable '{name}' does not exist in the '{}' scope", .scope.as_str())]
    UndefinedVariable {
        /// The variable name.
        name: String,
        /// The scope it was looked up in.
        scope: VariableScope,
    },

    /// A function or process with this name is declared twice in the module.
    /// The compiler does not produce that, but a module could have been built or
    /// edited by hand. Events are exempt: several blocks under one event are
    /// ordinary and all of them run.
    #[error("handler '{name}' is declared more than once in this module")]
    DuplicateHandler {
        /// The handler name.
        name: String,
    },

    /// There is no function with this name among the module's handlers.
    ///
    /// The same error covers an ambiguous name: edition 2026 mangles a module's
    /// handlers into `module::name`, and a reference by the short `name` that
    /// several modules declare cannot be resolved — running an arbitrary one of
    /// them would be worse than stopping.
    #[error("no function named '{name}' in this module{candidates}")]
    UnknownFunction {
        /// The name `call_function` asked for.
        name: String,
        /// What the module declares instead.
        candidates: Candidates,
    },

    /// There is no process with this name among the module's handlers.
    ///
    /// Ambiguous short names land here too, exactly as in
    /// [`Self::UnknownFunction`].
    #[error("no process named '{name}' in this module{candidates}")]
    UnknownProcess {
        /// The name `start_process` asked for.
        name: String,
        /// What the module declares instead.
        candidates: Candidates,
    },

    /// The event is not handled by any handler of the module.
    #[error("no handler for the '{event}' event in this module")]
    UnknownEvent {
        /// The event identifier.
        event: String,
    },

    /// The mock does not know the semantics of something in the module: an
    /// action from the `JustMC` schema, a `%math()` expression in text, a
    /// condition from `if_*`.
    ///
    /// Skipping this silently is not an option. An action that was supposed to
    /// return a value would give a wrong result without a single error, and a
    /// wrong result is worse than a stop: it gets taken for the real one.
    #[error("the mock runtime does not implement {what}")]
    Unimplemented {
        /// What exactly is not implemented — with its article, so the phrase
        /// reads as a whole: `action 'set_variable_get_item_custom_tag'`,
        /// `the %math() expression`.
        what: String,
    },

    /// An action was given an argument that is not in its schema.
    #[error("action '{action}' has no argument named '{arg}'")]
    UnknownArgument {
        /// The action identifier.
        action: &'static str,
        /// The name of the extra argument.
        arg: String,
    },

    /// A required argument of an action was not passed.
    #[error("action '{action}' is missing the required argument '{arg}'")]
    MissingArgument {
        /// The action identifier.
        action: &'static str,
        /// The name of the missing argument.
        arg: &'static str,
    },

    /// An action argument is not a variable, though the action needs a variable
    /// as its write target.
    #[error("action '{action}': argument '{arg}' expects a variable, got {actual}")]
    ExpectedVariable {
        /// The action identifier.
        action: &'static str,
        /// The argument name.
        arg: &'static str,
        /// What came instead of a variable.
        actual: String,
    },

    /// An action was given a value that cannot stand in this position: a
    /// parameter declaration (`Value::Parameter`) or an unevaluated variable
    /// reference. The compiler never writes such values into an operation, so
    /// meeting one means the module was built wrong.
    #[error("{context}: unexpected {actual} in this position")]
    UnexpectedValue {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A selection (`Op::selection`) refers to a way of picking targets the
    /// mock does not know. Quietly substituting the thread's target for it is
    /// not an option: the selection decides who an action is applied to.
    #[error("unsupported target selection '{selection}'")]
    UnimplementedSelection {
        /// The selection type from the operation.
        selection: String,
    },

    /// A variable holds nothing where a concrete value is needed.
    #[error("variable '{name}' (scope '{}') holds no value", .scope.as_str())]
    UnsetVariable {
        /// The variable name.
        name: String,
        /// The scope.
        scope: VariableScope,
    },

    /// An arithmetic operation was given something that is not a number.
    #[error("{context}: expected a number, got {actual}")]
    NotANumber {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A text operation was given something that is not text.
    #[error("{context}: expected text, got {actual}")]
    NotText {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A list operation was given something that is not a list.
    #[error("{context}: expected a list, got {actual}")]
    NotAList {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A map operation was given something that is not a map.
    #[error("{context}: expected a map, got {actual}")]
    NotAMap {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A coordinate operation was given something that is not a location.
    #[error("{context}: expected a location, got {actual}")]
    NotALocation {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A vector operation was given something that is not a vector.
    #[error("{context}: expected a vector, got {actual}")]
    NotAVector {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// A value is not an entity/player where one is needed.
    #[error("{context}: expected a player or entity, got {actual}")]
    NotATarget {
        /// What exactly was being evaluated.
        context: String,
        /// The actual type of the value.
        actual: String,
    },

    /// Division by zero.
    #[error("{context}: division by zero")]
    DivisionByZero {
        /// What exactly was being evaluated.
        context: String,
    },

    /// An index past the end of a list.
    #[error("{context}: index {index} is out of range for a list of {len} element(s)")]
    IndexOutOfRange {
        /// What exactly was being evaluated.
        context: String,
        /// The requested index.
        index: i64,
        /// The length of the list.
        len: usize,
    },

    /// The value of an enum argument is not in the allowed set.
    #[error("action '{action}': argument '{arg}' = '{value}' is not one of: {allowed}")]
    InvalidEnumArgument {
        /// The action identifier.
        action: &'static str,
        /// The argument name.
        arg: &'static str,
        /// The value that was passed.
        value: String,
        /// The allowed values, comma-separated.
        allowed: String,
    },

    /// A called function or process has a parameter the call passed no value
    /// for. The compiler fills `args` for every parameter, so this does not
    /// happen in a sound module; the error means the call was built wrong, and
    /// substituting an empty value is not an option — the function would
    /// compute a result from it.
    #[error("call to '{function}' does not pass a value for the parameter '{parameter}'")]
    MissingParameter {
        /// The name of the called function or process.
        function: String,
        /// The name of the parameter that got no value.
        parameter: String,
    },

    /// A call passed a value for a parameter the callee does not have.
    ///
    /// The compiler builds `args` from the declared parameters, so this does
    /// not happen in a sound module. There is no parameter name to give such a
    /// value — there is no such parameter — and creating a variable under an
    /// arbitrary name would mean running code nobody wrote.
    #[error("call to '{function}' passes a value for the unknown parameter '{parameter}'")]
    UnknownParameter {
        /// The name of the called function or process.
        function: String,
        /// The key the value arrived under.
        parameter: String,
    },

    /// An I/O error: reading or writing the `save` variable file.
    #[error("{context}: {source}")]
    Io {
        /// What was being done to the file.
        context: String,
        /// The file-system error.
        #[source]
        source: std::io::Error,
    },

    /// `control_break_label` named a label that is not on the stack. The
    /// compiler does not check labels, so a typo reaches the runtime; breaking
    /// out of an arbitrary place instead would be a guess.
    #[error("no enclosing label '{label}' for 'control_break_label'")]
    UnknownLabel {
        /// The label that was asked for.
        label: String,
    },

    /// An `else` operation appeared where there is no condition before it in
    /// the same block. The compiler writes `else` as the operation right after
    /// `if`, so this is either a hand-built module or a compiler bug. Running
    /// the `else` body here is not an option: it belongs to a branch that does
    /// not exist.
    #[error("'else' has no preceding condition in this block")]
    MisplacedElse,

    /// An action needs a player/entity as its target, and there is no target.
    #[error("{context}: the thread has no target")]
    NoTarget {
        /// The action identifier.
        context: String,
    },

    /// An exception raised by `control_call_exception`.
    #[error("{kind:?} exception{id}: {message}", id = .id.as_ref().map_or_else(String::new, |id| format!(" '{id}'")))]
    Raised {
        /// The identifier, if one was given.
        id: Option<String>,
        /// The message.
        message: String,
        /// The kind of the exception.
        kind: ExceptionKind,
    },

    /// A branch took too many steps and was stopped. A guard against
    /// `repeat_forever` and the like in a mock that has no server tick to
    /// interrupt them.
    #[error("step limit of {limit} exceeded; the program is probably looping forever")]
    StepLimitExceeded {
        /// The configured limit.
        limit: usize,
    },

    /// A JSON parse error in `set_variable_parse_json` and the like.
    #[error("{context}: invalid JSON: {source}")]
    Json {
        /// What exactly was being evaluated.
        context: String,
        /// The `serde_json` error.
        #[source]
        source: serde_json::Error,
    },

    /// An error serializing a value to JSON.
    #[error("{context}: cannot serialize value: {source}")]
    Serialize {
        /// What exactly was being evaluated.
        context: String,
        /// The `serde_json` error.
        #[source]
        source: serde_json::Error,
    },

    /// A handler started from outside the scheduler — a direct
    /// `Runtime::call_function` — was still asleep when the tick budget ran out.
    ///
    /// The scheduler stops at [`Config::tick_limit`](crate::Config::tick_limit)
    /// and leaves sleeping tasks for a later run, so the call has no value to
    /// return yet. Reporting an empty value instead would look like a function
    /// that returned nothing.
    #[error(
        "'{name}' is still running after the tick budget of {limit} ticks; raise Config::tick_limit to let it finish"
    )]
    Suspended {
        /// The function or process that did not finish.
        name: String,
        /// The budget that ran out.
        limit: u64,
    },

    /// An error carrying a location in the module. It wraps any other error as
    /// it leaves a handler, an action or a function, and serves diagnostics
    /// only: whether an error is catchable is decided by the nested one.
    #[error("{location}: {source}")]
    AtLocation {
        /// The handler, the action and the operation number.
        location: String,
        /// The original error.
        #[source]
        source: Box<Self>,
    },
}

impl RuntimeError {
    /// Adds a location in the module to an error, unless it already has one.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(location, source))]
    pub fn at(location: impl Into<String>, source: Self) -> Self {
        match source {
            Self::AtLocation { .. } => source,
            other => Self::AtLocation {
                location: location.into(),
                source: Box::new(other),
            },
        }
    }

    /// Adds a location in the module to an error, keeping the one already
    /// recorded.
    ///
    /// Unlike [`Self::at`] it does not discard the outer wrapper but chains the
    /// wrappers together. That is how a frame boundary is marked — a function
    /// call or a process launch: an [`Self::at`] inside the body has already
    /// recorded the most precise position, and `called_from` adds from where
    /// the frame was called, on the outside.
    ///
    /// A location equal to the innermost one already recorded is not added
    /// again: an event handler and its body share the same origin, and without
    /// this guard the same frame would be printed twice
    /// (`event player_quit: event player_quit: op #1`).
    #[must_use]
    #[tracing::instrument(level = "trace", skip(location, source))]
    pub fn called_from(location: impl Into<String>, source: Self) -> Self {
        let location = location.into();
        if let Self::AtLocation {
            location: inner, ..
        } = &source
            && (inner == &location || inner.starts_with(&format!("{location}: ")))
        {
            return source;
        }
        Self::AtLocation {
            location,
            source: Box::new(source),
        }
    }

    /// Strips the location wrappers and returns the original error.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn root(&self) -> &Self {
        match self {
            Self::AtLocation { source, .. } => source.root(),
            other => other,
        }
    }

    /// Whether `controller_exception` can catch this error.
    ///
    /// Critical errors, module-inconsistency errors and everything the mock
    /// cannot do are not caught: catching them would mean the module itself
    /// decides that a gap in the mock is fine, and the error would disappear
    /// from view.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn is_catchable(&self) -> bool {
        !matches!(
            self.root(),
            Self::Raised {
                kind: ExceptionKind::Fatal,
                ..
            } | Self::UnknownFunction { .. }
                | Self::UnknownProcess { .. }
                | Self::UnknownEvent { .. }
                | Self::DuplicateHandler { .. }
                | Self::MisplacedElse
                | Self::UnknownLabel { .. }
                | Self::StepLimitExceeded { .. }
                | Self::Suspended { .. }
                | Self::Unimplemented { .. }
                | Self::UnimplementedSelection { .. }
        )
    }

    /// Checks whether `controller_exception` with the exception type `kind`
    /// catches this error.
    ///
    /// An error raised by [`RuntimeError::Raised`] has a kind of its own, and
    /// that kind decides: `ALL` takes everything catchable, `WARNING` takes
    /// warnings only, `ERROR` takes everything catchable except warnings. A
    /// runtime error of the mock has no kind of its own and counts as an error,
    /// not a warning.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(kind = ?kind))]
    pub fn matches_kind(&self, kind: ExceptionKind) -> bool {
        if !self.is_catchable() {
            return false;
        }
        let raised = match self.root() {
            Self::Raised { kind, .. } => Some(*kind),
            _ => None,
        };
        match kind {
            ExceptionKind::All => true,
            ExceptionKind::Warning => raised == Some(ExceptionKind::Warning),
            ExceptionKind::Error => raised != Some(ExceptionKind::Warning),
            ExceptionKind::Fatal => false,
        }
    }
}

/// The result of a runtime operation.
pub type Result<T> = std::result::Result<T, RuntimeError>;

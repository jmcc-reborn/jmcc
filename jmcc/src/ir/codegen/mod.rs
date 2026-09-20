//! MIR → `Module`: `CodeGen` struct, `generate` entry point, and internal infrastructure.
//!
//! Submodules organize implementation by responsibility: `handlers` for MIR root
//! traversal and handler assembly, `ops` for lowering MIR nodes into `Op`, `values`
//! for building `Value`, and `selectors` for game values and selectors.

use crate::Target;
use crate::ir::mir::Mir;
use egg::{Id, RecExpr};
use jmcdata::generated::{
    ActionArg, ActionDef, ActionId, ArgType, MeasureTimeDuration, get_action_def, get_action_id,
};
use jmcdata::module::{
    Line, LineType, LineValue, Module, Number, Op, Parameter, Selection, TextParsing, TextValue,
    Value, VariableScope,
};
use jmcdata::op_builder::OpBuilder;
use litemap::LiteMap;
use ordered_float::OrderedFloat;
use std::borrow::Cow;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodegenError {
    #[error("Unknown action: {0}::{1}")]
    UnknownAction(String, String),
    #[error("Expected string literal, got {0:?}")]
    InvalidString(Mir),
    #[error("Expected variable, got {0:?}")]
    InvalidVariable(Mir),
    #[error("Expected action node, got {0:?}")]
    InvalidAction(Mir),
    #[error("Invalid event name {0:?}")]
    InvalidEvent(String, #[source] serde_json::Error),
    #[error("Invalid game value {0:?}")]
    InvalidGameValue(String, #[source] serde_json::Error),
    #[error("Invalid game value id: {value}")]
    InvalidGameValueId {
        value: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Unknown constructor: {0}")]
    UnknownConstructor(String),
    #[error("Constructor argument '{argument}' must be a constant {expected}, got: {got}")]
    InvalidConstructorArgument {
        argument: String,
        expected: &'static str,
        got: String,
    },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SNBT error: {0}")]
    Snbt(String),
    #[error("Process call used as a value")]
    ProcessAsValue,
    #[error("Enum requires text, number, or variable value")]
    InvalidEnumValue,
    #[error("Missing enum value at index {0}")]
    MissingEnumValue(usize),
    #[error("Unsupported value: {0:?}")]
    UnsupportedValue(Mir),
    #[error("Expected selector node, got {0:?}")]
    InvalidSelector(Mir),
}

type CgResult<T> = std::result::Result<T, CodegenError>;
type ActionValues = LiteMap<Cow<'static, str>, Value<'static>>;
type EmittedActionArguments = (ActionValues, HashSet<String>);

#[tracing::instrument(skip(e), level = "trace")]
fn extract_str(e: &RecExpr<Mir>, id: Id) -> CgResult<String> {
    match &e[id] {
        Mir::Str(s) => Ok(s.0.to_string()),
        other => Err(CodegenError::InvalidString(other.clone())),
    }
}

const ONE: Value<'static> = Value::Number {
    number: Number::Simple(OrderedFloat(1.0)),
};
const ZERO: Value<'static> = Value::Number {
    number: Number::Simple(OrderedFloat(0.0)),
};

pub struct CodeGen {
    handlers: Vec<Line<'static>>,
    world_start_ops: Vec<Op<'static>>,
    temp_n: usize,
    var_scopes: std::collections::HashMap<String, VariableScope>,
    pub(crate) let_bindings: std::collections::HashMap<egg::Symbol, Id>,
    default_scope: VariableScope,
    pub(crate) edition: u16,
    pub(crate) var_aliases: std::collections::HashMap<String, Value<'static>>,
    pub disable_action_limit: bool,
    func_split_counter: usize,
    idx: usize,
}

impl CodeGen {
    #[must_use]
    #[tracing::instrument(level = "trace")]
    pub fn new(_target: Target, edition: u16) -> Self {
        Self {
            handlers: Vec::new(),
            world_start_ops: Vec::new(),
            temp_n: 0,
            var_scopes: std::collections::HashMap::new(),
            let_bindings: std::collections::HashMap::new(),
            default_scope: if edition >= 2026 {
                VariableScope::Line
            } else {
                VariableScope::Local
            },
            edition,
            var_aliases: std::collections::HashMap::new(),
            disable_action_limit: false,
            func_split_counter: 0,
            idx: 0,
        }
    }

    pub(crate) fn unwrapped_variable_static(e: &RecExpr<Mir>, mut id: Id) -> Id {
        while let Mir::Local(inner) | Mir::Game(inner) | Mir::Save(inner) | Mir::Line(inner) =
            &e[id]
        {
            id = *inner;
        }
        id
    }

    /// Resolves variable aliases across temporary variables in SSA/MIR.
    pub(super) fn resolve_alias(&self, mut val: Value<'static>) -> Value<'static> {
        let mut depth = 0;
        while let Value::Variable { variable, .. } = &val {
            if depth >= 16 {
                tracing::warn!(%variable, "Cycle or excessive depth in var_aliases");
                break;
            }
            if let Some(aliased) = self.var_aliases.get(variable.as_ref()) {
                val = aliased.clone();
                depth += 1;
            } else {
                break;
            }
        }
        val
    }

    /// # Errors
    ///
    /// Returns an error if MIR contains a value that cannot be encoded in the output module.
    #[tracing::instrument(skip(self, expr), level = "trace")]
    pub fn generate(&mut self, expr: &RecExpr<Mir>) -> CgResult<Module<'static>> {
        self.var_aliases.clear();
        let mut let_bindings = std::collections::HashMap::with_capacity(expr.len() / 4);
        for node in expr.as_ref() {
            if let Mir::Let([bound, value, _]) = node {
                let var_id = Self::unwrapped_variable_static(expr, *bound);
                if let Mir::Var(bound_variable) = &expr[var_id] {
                    let_bindings.insert(bound_variable.0, *value);
                }
            }
        }
        self.let_bindings = let_bindings;

        self.emit_root(expr, expr.root())?;

        if !self.world_start_ops.is_empty() {
            tracing::trace!(ops = self.world_start_ops.len(), "Emitting world_start");
            self.handlers.push(Line {
                line_type: LineType::Event,
                position: 1337,
                operations: std::mem::take(&mut self.world_start_ops),
                line_value: LineValue::Event {
                    event: serde_json::from_str("\"world_start\"")
                        .map_err(|e| CodegenError::InvalidEvent("world_start".into(), e))?,
                },
            });
        }

        if !self.disable_action_limit {
            self.split_long_handlers();
        }

        if self.handlers.len() > jmcdata::consts::MAX_HANDLERS as usize {
            let count = self.handlers.len();
            let max = jmcdata::consts::MAX_HANDLERS;
            tracing::warn!(count, max, "Handler count exceeds platform limit");
            #[expect(clippy::print_stderr, reason = "compiler warning output to stderr")]
            {
                match crate::i18n::current_lang() {
                    crate::i18n::Lang::Ru => {
                        eprintln!(
                            "\x1b[1;33mпредупреждение\x1b[0m: количество хендлеров ({count}) превышает максимальное ({max})"
                        );
                    }
                    crate::i18n::Lang::En => {
                        eprintln!(
                            "\x1b[1;33mwarning\x1b[0m: handler count ({count}) exceeds maximum allowed ({max})"
                        );
                    }
                }
            }
        }

        self.handlers.sort_by_key(|l| {
            (
                match l.line_type {
                    LineType::Event => 0,
                    LineType::Function => 1,
                    LineType::Process => 2,
                },
                l.operations.len(),
            )
        });

        for (pos, line) in &mut self.handlers.iter_mut().enumerate() {
            let fmt = match &line.line_value {
                LineValue::Event { event } => format!("{event:?}"),
                LineValue::Fn { name, .. } => name.to_string(),
            };
            let timer = Value::Variable {
                variable: "t".into(),
                scope: VariableScope::Line,
            };
            line.operations = vec![
                Op::controller_measure_time(
                    timer.clone(),
                    Some(MeasureTimeDuration::Microseconds),
                    std::mem::take(&mut line.operations),
                ),
                Op::player_message(
                    Value::Array {
                        values: vec![Some(format!("{fmt} running time:").into()), Some(timer)],
                    },
                    None,
                ),
            ];
            line.position = pos as u16;
        }

        Ok(Module {
            handlers: std::mem::take(&mut self.handlers),
        })
    }

    fn split_long_handlers(&mut self) {
        let max_length = jmcdata::consts::MAX_ACTIONS_PER_LINE as usize;
        let mut i = 0;
        while i < self.handlers.len() {
            if !matches!(self.handlers[i].line_type, LineType::Function) {
                i += 1;
                continue;
            }
            self.idx = 0;
            let mut ops = std::mem::take(&mut self.handlers[i].operations);
            self.walk_operations(&mut ops, max_length);
            self.handlers[i].operations = ops;
            i += 1;
        }
    }

    fn walk_operations(&mut self, acts: &mut Vec<Op<'static>>, mut max_length: usize) {
        if max_length > jmcdata::consts::MAX_ACTIONS_PER_LINE as usize {
            max_length = jmcdata::consts::MAX_ACTIONS_PER_LINE as usize;
        }

        let mut action_idx = 0;
        while action_idx < acts.len() {
            let is_container = acts[action_idx].operations.is_some();
            let action_length = if is_container { 2 } else { 1 };
            let new_idx = self.idx + action_length;
            let has_next = action_idx + 1 < acts.len();
            let has_contents = if is_container {
                usize::from(
                    acts[action_idx]
                        .operations
                        .as_ref()
                        .is_some_and(|ops| !ops.is_empty()),
                )
            } else {
                0
            };

            let mut reserved = 0;
            if has_next {
                reserved += 1;
            }

            if has_next && is_container {
                let next_acti = &acts[action_idx + 1];
                if next_acti.operations.is_some() && next_acti.action == ActionId::Else {
                    reserved += 2;
                    if next_acti
                        .operations
                        .as_ref()
                        .is_some_and(|ops| !ops.is_empty())
                    {
                        reserved += 1;
                    }
                    if action_idx + 2 < acts.len() {
                        reserved += 1;
                    }
                }
            }

            let container_max_length = max_length
                .saturating_sub(reserved)
                .saturating_sub(has_contents);

            if new_idx > container_max_length {
                let func_count = self.func_split_counter;
                self.func_split_counter += 1;

                let mut call_values = LiteMap::new();
                call_values.insert(
                    Cow::Borrowed("function_name"),
                    Value::Text {
                        text: Cow::Owned(format!("jmcc.{func_count}")),
                        parsing: TextParsing::Legacy,
                    },
                );
                let call_func = Op {
                    action: ActionId::CallFunction,
                    values: call_values,
                    operations: None,
                    conditional: None,
                    selection: None,
                    is_inverted: None,
                };

                let remainder = acts.split_off(action_idx);
                acts.push(call_func);

                let desc_text = match crate::i18n::current_lang() {
                    crate::i18n::Lang::Ru => "Функция создана автоматически",
                    crate::i18n::Lang::En => "The function was created automatically",
                };
                let mut func_values = LiteMap::new();
                func_values.insert(
                    Cow::Borrowed("description"),
                    Value::Array {
                        values: vec![Some(Value::Text {
                            text: Cow::Borrowed(desc_text),
                            parsing: TextParsing::Legacy,
                        })],
                    },
                );

                let func = Line {
                    line_type: LineType::Function,
                    position: 1337,
                    operations: remainder,
                    line_value: LineValue::Fn {
                        values: func_values,
                        name: Cow::Owned(format!("jmcc.{func_count}")),
                    },
                };
                self.handlers.push(func);
                self.idx += 1;
                break;
            }

            self.idx = new_idx;
            action_idx += 1;
        }
    }

    #[tracing::instrument(level = "trace")]
    fn is_conditional(obj: &str, name: &str) -> bool {
        get_action_def(obj, name).as_ref().is_some_and(|def| {
            def.action_type == "container" || def.action_type.ends_with("_with_conditional")
        })
    }

    #[tracing::instrument(skip(self, e), fields(node = ?e[id]), level = "trace")]
    fn get_var_name_scope(&self, e: &RecExpr<Mir>, id: Id) -> CgResult<(String, VariableScope)> {
        Ok(match &e[id] {
            Mir::Var(v) => (
                v.0.to_string(),
                self.var_scopes
                    .get(v.0.as_str())
                    .copied()
                    .unwrap_or(self.default_scope),
            ),
            Mir::Str(s) => (
                s.0.to_string(),
                self.var_scopes
                    .get(s.0.as_str())
                    .copied()
                    .unwrap_or(self.default_scope),
            ),
            Mir::Local(i) => (self.get_var_name_scope(e, *i)?.0, VariableScope::Local),
            Mir::Game(i) => (self.get_var_name_scope(e, *i)?.0, VariableScope::Global),
            Mir::Save(i) => (self.get_var_name_scope(e, *i)?.0, VariableScope::Save),
            Mir::Line(i) => (self.get_var_name_scope(e, *i)?.0, VariableScope::Line),
            other => return Err(CodegenError::InvalidVariable(other.clone())),
        })
    }

    #[tracing::instrument(skip(self), level = "trace")]
    fn fresh_temp(&mut self) -> Value<'static> {
        let name = format!("__ct{}", self.temp_n);
        tracing::trace!(name, "fresh temp");
        self.temp_n += 1;
        Value::Variable {
            variable: Cow::Owned(name),
            scope: VariableScope::Line,
        }
    }
}

impl Default for CodeGen {
    #[tracing::instrument(level = "trace")]
    fn default() -> Self {
        Self::new(Target::Justmc, 2026)
    }
}

mod handlers;
mod ops;
mod selectors;
mod values;

//! Loading a module and running its handlers.
//!
//! A module is what the compiler wrote into a `.json`. Here it is parsed through
//! [`jmcdata::module`] (the mock has no data model of its own) and laid out into
//! indices: events, functions, processes. Alongside each handler its frame
//! information is kept — the names of its line variables and its parameter list
//! — collected once at load time.
//!
//! [`Program`] owns the parsed module and hands out handler bodies as
//! `&'a [Op<'a>]` without borrowing [`Runtime`]. That is not a nicety but a
//! condition of working at all: the scheduler takes `&mut self` (for the step
//! counter, the journal and the world) and at the same time runs operations
//! borrowed from the module. They cannot be borrowed from `Runtime` — the body
//! lives in `Program`, which sits next to it and is not touched.
//!
//! Модуль разделён по ответственности: `events` — события и их обработчики,
//! `calls` — вход в обработчик извне планировщика, `scheduler` — кооперативное
//! исполнение потоков и `wait`, `steps` — шаги и журнал, `vars` — переменные
//! программы. Главные типы и их конструкторы остаются здесь.

use std::collections::HashMap;

use jmcdata::generated::{ArgType, EventId};
use jmcdata::module::{Line, LineType, LineValue, Module, Op, Parameter, Value};

use crate::config::Config;
use crate::error::{Candidates, Result, RuntimeError};
use crate::interp::Flow;
use crate::scope::{Shared, line_var_names, scoped_var_names};
use crate::value::Rt;
use crate::world::{Selection, Target, World, default_world};

mod calls;
mod events;
mod scheduler;
mod steps;
mod vars;

pub use events::event_name;
use vars::load_save;

/// The name of the local variable the compiler returns a function's value
/// through: the callee writes it, the caller reads it right after the call.
pub const RETURN_VARIABLE: &str = "ret";

/// A parsed module with its handlers indexed.
///
/// Lives longer than [`Runtime`] and is passed to it by reference: the runtime
/// owns nothing of the program's code, so operations can be run straight out of
/// it.
#[derive(Debug)]
pub struct Program<'a> {
    module: Module<'a>,
    handlers: Handlers,
    frames: Vec<Frame>,
}

/// Handler indices inside [`Module::handlers`].
///
/// Events map to a list because `JustMC` fires every hat: the compiler emits one
/// handler per `event<...>` block, and a project split across files has several
/// blocks for one event. They all run, in the order the module lists them.
#[derive(Debug, Default)]
struct Handlers {
    functions: HashMap<String, usize>,
    processes: HashMap<String, usize>,
    /// The last segment of a mangled name (`module::name` -> `name`) to every
    /// handler that carries it, with the handler's full name. Used only when the
    /// exact name misses.
    function_shorts: HashMap<String, Vec<(String, usize)>>,
    process_shorts: HashMap<String, Vec<(String, usize)>>,
    events: HashMap<EventId, Vec<usize>>,
}

/// Looks a handler up by the name the program used.
///
/// Exact name first: that is what edition 2023 emits, and what a fully qualified
/// name in edition 2026 looks like. Otherwise the mangled names are searched by
/// their last segment. `Err` carries what the message should offer instead — the
/// whole declared list, or the several matches.
#[tracing::instrument(level = "debug", skip(exact, shorts), fields(name = %name))]
fn resolve(
    exact: &HashMap<String, usize>,
    shorts: &HashMap<String, Vec<(String, usize)>>,
    name: &str,
) -> std::result::Result<usize, Candidates> {
    if let Some(&index) = exact.get(name) {
        return Ok(index);
    }
    match shorts.get(name) {
        Some(matches) if matches.len() == 1 => Ok(matches[0].1),
        Some(matches) => Err(Candidates::matches(
            matches.iter().map(|(full, _)| full.clone()).collect(),
        )),
        None => {
            let mut declared: Vec<String> = exact.keys().cloned().collect();
            declared.sort();
            Err(Candidates::declared(declared))
        }
    }
}

/// The frame information of one handler: what is set up on entering a frame.
#[derive(Debug, Default)]
struct Frame {
    /// The names of the line variables occurring in the body. They are declared
    /// on entry: `JustMC` creates line variables at the start of a line, so
    /// reading a variable written on only one of the branches gives an empty
    /// value rather than an error.
    line_vars: Vec<String>,
    /// The parameters: for functions and processes they arrive in their frame's
    /// line variables.
    parameters: Vec<Param>,
}

/// One declared parameter of a function or process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The name the callee sees the argument under.
    pub name: String,
    /// Whether the parameter is the caller's variable itself rather than a copy
    /// of its value. The compiler marks those with `value_type: variable`:
    /// `ref` in `.jc`, and the receiver of a method.
    pub by_ref: bool,
}

/// An argument on its way from a call into the callee's frame.
///
/// The two kinds differ in *when* the caller's frame is read: a plain value is
/// read at the call site, while a reference is not read at all — the callee
/// reads and writes the caller's variable directly.
#[derive(Debug, Clone)]
pub enum Bound<'a> {
    /// A value to place in the callee's frame.
    Value(Rt<'a>),
    /// The caller's variable a `variable`-typed parameter stands for.
    Reference(Shared<'a>, String),
}

impl<'a> Program<'a> {
    /// Parses the JSON `jmcc` emitted.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Json`] if the text does not parse, and
    /// [`RuntimeError::DuplicateHandler`] if the module has two functions or two
    /// processes under one name.
    #[tracing::instrument(level = "debug", skip(text))]
    pub fn parse(text: &'a str) -> Result<Self> {
        let module =
            serde_json::from_str::<Module<'a>>(text).map_err(|source| RuntimeError::Json {
                context: "module".to_owned(),
                source,
            })?;
        Self::from_module(module)
    }

    /// Builds a program from an already parsed module.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::DuplicateHandler`] if the module has two functions
    /// or two processes under one name.
    #[tracing::instrument(level = "debug", skip(module))]
    pub fn from_module(module: Module<'a>) -> Result<Self> {
        let handlers = Handlers::build(&module)?;
        let frames = module
            .handlers
            .iter()
            .map(|line| Frame {
                line_vars: line_var_names(&line.operations),
                parameters: parameters(line),
            })
            .collect();
        Ok(Self {
            module,
            handlers,
            frames,
        })
    }

    /// The module the program was built from.
    #[must_use]
    pub const fn module(&self) -> &Module<'a> {
        &self.module
    }

    /// The module's function names.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.handlers.functions.keys().map(String::as_str)
    }

    /// The module's process names.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn process_names(&self) -> impl Iterator<Item = &str> {
        self.handlers.processes.keys().map(String::as_str)
    }

    /// The events the module handles.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn events(&self) -> impl Iterator<Item = EventId> + '_ {
        self.handlers.events.keys().copied()
    }

    /// Whether the module has a handler for this event.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self), fields(event = ?event))]
    pub fn has_event(&self, event: EventId) -> bool {
        self.handlers.events.contains_key(&event)
    }

    /// The index of a function by name.
    ///
    /// Edition 2026 mangles a module's handlers into `module::name`, while
    /// `code::call_function` gets the name as text — the same text the author
    /// wrote. So an exact match wins and a unique match by the last segment
    /// follows; the compiler's own dead-code pass resolves references the same
    /// way, which is why a function reachable only as text is not dropped.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownFunction`] when there is no such function,
    /// and when the short name matches several of them.
    #[tracing::instrument(level = "debug", skip(self), fields(name = %name))]
    pub(crate) fn function_index(&self, name: &str) -> Result<usize> {
        resolve(
            &self.handlers.functions,
            &self.handlers.function_shorts,
            name,
        )
        .map_err(|candidates| RuntimeError::UnknownFunction {
            name: name.to_owned(),
            candidates,
        })
    }

    /// The index of a process by name. The short-name rule is the one of
    /// [`Self::function_index`].
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownProcess`] when there is no such process,
    /// and when the short name matches several of them.
    #[tracing::instrument(level = "debug", skip(self), fields(name = %name))]
    pub(crate) fn process_index(&self, name: &str) -> Result<usize> {
        resolve(
            &self.handlers.processes,
            &self.handlers.process_shorts,
            name,
        )
        .map_err(|candidates| RuntimeError::UnknownProcess {
            name: name.to_owned(),
            candidates,
        })
    }

    /// The indices of an event's handlers, in the order the module lists them.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self), fields(event = ?event))]
    pub(crate) fn event_indices(&self, event: EventId) -> &[usize] {
        self.handlers
            .events
            .get(&event)
            .map_or(&[][..], Vec::as_slice)
    }

    /// A handler's body.
    ///
    /// Called on `&'a Program<'a>`, so it returns a reference with the module's
    /// own lifetime rather than a borrow.
    #[tracing::instrument(level = "debug", skip(self), fields(index = ?index))]
    pub(crate) fn body(&self, index: usize) -> &[Op<'a>] {
        self.module.handlers[index].operations.as_slice()
    }

    /// A handler's parameters, in declaration order.
    #[tracing::instrument(level = "debug", skip(self), fields(index = ?index))]
    pub(crate) fn parameters(&self, index: usize) -> &[Param] {
        &self.frames[index].parameters
    }

    /// A handler's line variable names.
    #[tracing::instrument(level = "debug", skip(self), fields(index = ?index))]
    pub(crate) fn line_vars(&self, index: usize) -> &[String] {
        &self.frames[index].line_vars
    }
}

impl Handlers {
    #[tracing::instrument(level = "debug", skip(module))]
    fn build(module: &Module<'_>) -> Result<Self> {
        let mut handlers = Self::default();
        for (index, line) in module.handlers.iter().enumerate() {
            match line.line_type {
                LineType::Function => {
                    let name = fn_name(line).to_owned();
                    if handlers.functions.insert(name.clone(), index).is_some() {
                        return Err(RuntimeError::DuplicateHandler { name });
                    }
                    add_short(&mut handlers.function_shorts, name, index);
                }
                LineType::Process => {
                    let name = fn_name(line).to_owned();
                    if handlers.processes.insert(name.clone(), index).is_some() {
                        return Err(RuntimeError::DuplicateHandler { name });
                    }
                    add_short(&mut handlers.process_shorts, name, index);
                }
                LineType::Event => {
                    // An event handler must carry `LineValue::Event`; a
                    // `LineValue::Fn` here would mean the module is corrupt.
                    let LineValue::Event { event } = line.line_value else {
                        continue;
                    };
                    handlers.events.entry(event).or_default().push(index);
                }
            }
        }
        Ok(handlers)
    }
}

/// Records a handler under the last segment of its name.
///
/// The segment is the name itself when it has no `::`, and then it is already
/// reachable through the exact map — harmless, the exact lookup runs first.
#[tracing::instrument(level = "debug", skip(shorts), fields(name = %name, index = ?index))]
fn add_short(shorts: &mut HashMap<String, Vec<(String, usize)>>, name: String, index: usize) {
    let short = name
        .rsplit_once("::")
        .map_or(name.as_str(), |(_, short)| short);
    shorts
        .entry(short.to_owned())
        .or_default()
        .push((name, index));
}

/// The name of a function or process handler. Events have no name: the event's
/// identifier plays that role.
///
/// Both lifetimes are named explicitly: `&Line<'_>` has two — the borrow and the
/// data inside `Line` — and without names it is unclear which one the returned
/// `&str` is tied to.
#[tracing::instrument(level = "debug", skip(line))]
fn fn_name<'a>(line: &'a Line<'a>) -> &'a str {
    match &line.line_value {
        LineValue::Fn { name, .. } => name,
        LineValue::Event { .. } => "",
    }
}

/// The parameters of a function or process, in declaration order.
///
/// `value_type` is what tells the two binding modes apart: `variable` means the
/// parameter *is* the caller's variable, anything else means it is a copy of
/// its value.
#[tracing::instrument(level = "debug", skip(line))]
fn parameters(line: &Line<'_>) -> Vec<Param> {
    let LineValue::Fn { values, .. } = &line.line_value else {
        return Vec::new();
    };
    let Some(Value::Array { values: items }) = values.get("parameters") else {
        return Vec::new();
    };
    items
        .iter()
        .flatten()
        .filter_map(|item| match item {
            Value::Parameter {
                name, param_type, ..
            } => Some(Param {
                name: name.to_string(),
                by_ref: matches!(
                    param_type,
                    Parameter::Singular {
                        value_type: ArgType::Variable,
                        ..
                    } | Parameter::Plural {
                        value_type: ArgType::Variable,
                        ..
                    }
                ),
            }),
            _ => None,
        })
        .collect()
}

/// How a process relates to the caller's local variables.
///
/// Local variables live as long as the process or event and are shared by all
/// frames inside it. A function always shares `local` with its caller — otherwise
/// `ret` would not reach the caller. A process, though, may copy it or start from
/// an empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalVariables {
    /// A new stream starts from an empty `local`.
    #[default]
    DontCopy,
    /// A new stream gets a copy of the caller's `local`; changes are not visible
    /// back.
    Copy,
    /// The stream shares `local` with the caller: this is the function call mode.
    Share,
}

/// Which targets the process being started is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetMode {
    /// The calling stream's target.
    #[default]
    CurrentTarget,
    /// The calling stream's current selection.
    CurrentSelection,
    /// A stream with no target.
    NoTarget,
    /// One stream per target of the selection.
    ForEachInSelection,
}

/// One frame of execution — what `JustMC` calls a stream.
///
/// A frame carries its variable stores, its targets and its selection. The line
/// store is its own per frame and is swapped on a call; the local one is shared
/// by the whole process or event. The global and `save` stores are not part of
/// it: they belong to the runtime entirely and are taken from it by scope rather
/// than from the frame.
#[derive(Debug)]
pub struct Stream<'a> {
    pub(crate) local: Shared<'a>,
    pub(crate) line: Shared<'a>,
    pub(crate) targets: Vec<Target>,
    pub(crate) selection: Selection,
    pub(crate) origin: String,
    pub(crate) chat_message: String,
    pub(crate) event_item: Rt<'a>,
    pub(crate) event_slot: f64,
    pub(crate) inventory_title: String,
}

/// Creates an empty frame for a new stream.
///
/// Not a method on [`Runtime`]: a frame needs nothing from the runtime — not
/// the global variables, not the world. It is handed them later, and it builds
/// its own stores.
#[tracing::instrument(level = "debug", fields(targets = ?targets, origin = %origin))]
pub fn spawn_stream<'a>(targets: Vec<Target>, origin: String) -> Stream<'a> {
    let selection = targets.clone();
    Stream {
        local: Shared::new(),
        line: Shared::new(),
        targets,
        selection,
        origin,
        chat_message: String::new(),
        event_item: None,
        event_slot: 0.0,
        inventory_title: String::new(),
    }
}

impl Stream<'_> {
    /// Where this frame came from — for error messages.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The targets the frame is bound to.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// The frame's current selection.
    #[must_use]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn selection(&self) -> &[Target] {
        &self.selection
    }
}

/// The event's data — what the code sees through game values.
///
/// An event handler has no arguments in JSON: the chat message, the item in the
/// slot and the open menu's title are properties of the event itself rather than
/// values from the module. The mock takes them from here.
#[derive(Debug, Clone, Default)]
pub struct EventData<'a> {
    /// The targets the handler is bound to. An empty list means the world's
    /// default target, [`World::primary_target`].
    pub targets: Vec<Target>,
    /// The chat message's text (`event_chat_message`).
    pub chat_message: Option<String>,
    /// The event's item (`event_item`).
    pub item: Rt<'a>,
    /// The slot number (`event_slot`).
    pub slot: Option<f64>,
    /// The open menu's title (`open_inventory_title`).
    pub inventory_title: Option<String>,
}

/// The mock runtime: a world, settings and global variables around one
/// [`Program`].
#[derive(Debug)]
pub struct Runtime<'a> {
    program: &'a Program<'a>,
    world: World,
    config: Config,
    global: Shared<'a>,
    save: Shared<'a>,
    steps: usize,
    scheduler: scheduler::Scheduler<'a>,
}

impl<'a> Runtime<'a> {
    /// Creates a runtime with default settings.
    ///
    /// # Errors
    ///
    /// Returns an error if [`Config::save_file`] is set and the file exists but
    /// cannot be read.
    #[tracing::instrument(level = "debug", skip(program))]
    pub fn new(program: &'a Program<'a>) -> Result<Self> {
        Self::with_config(program, Config::default())
    }

    /// Creates a runtime with the given settings.
    ///
    /// If the settings have [`Config::save_file`], the `save` variables are read
    /// from it. A missing file is not an error: that is the first run.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::Io`] if the file exists but cannot be read, and
    /// [`RuntimeError::Json`] if its contents do not parse.
    #[tracing::instrument(level = "debug", skip(program, config))]
    pub fn with_config(program: &'a Program<'a>, config: Config) -> Result<Self> {
        let save = match &config.save_file {
            Some(path) => Shared::with_vars(load_save(path)?),
            None => Shared::new(),
        };
        let mut world = if config.default_player {
            default_world()
        } else {
            World::new()
        };
        if let Some(limit) = config.log_limit {
            *world.log_mut() = crate::world::Log::with_limit(limit);
        }
        let global = Shared::new();
        for line in &program.module.handlers {
            for name in scoped_var_names(&line.operations, jmcdata::module::VariableScope::Global) {
                global.declare(&name);
            }
        }
        for line in &program.module.handlers {
            for name in scoped_var_names(&line.operations, jmcdata::module::VariableScope::Save) {
                save.declare(&name);
            }
        }
        Ok(Self {
            program,
            world,
            config,
            global,
            save,
            steps: 0,
            scheduler: scheduler::Scheduler::default(),
        })
    }

    /// The program the runtime executes.
    #[must_use]
    pub const fn program(&self) -> &'a Program<'a> {
        self.program
    }

    /// The settings.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// The world.
    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// The world for modification.
    #[must_use]
    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}

impl Program<'_> {
    /// A handler's name by its index — for error messages.
    #[tracing::instrument(level = "debug", skip(self), fields(index = ?index))]
    pub(crate) fn name(&self, index: usize) -> String {
        let line = &self.module.handlers[index];
        match &line.line_value {
            LineValue::Fn { name, .. } => name.to_string(),
            LineValue::Event { event } => event_name(*event),
        }
    }
}

//! The `JustMC` mock runtime: loads the JSON that `jmcc` emits and runs it.
//!
//! The mock exists so a `.jc` program can be run without a server: to see what
//! it does to the world, and to get an error where the program is broken. The
//! data model is not invented here — it is [`jmcdata::module`], the very same
//! one the compiler describes a program with. All that is original here is the
//! execution: what each action means and what happens between actions.
//!
//! # What is checked at runtime
//!
//! The compiler catches everything visible statically: types, names, argument
//! sets. There is no separate analyser in the mock, and there should not be.
//! But three things are not checked statically, and passing over them silently
//! is not an option:
//!
//! * a variable that is not in its scope at the moment it is read
//!   ([`RuntimeError::UndefinedVariable`]) — it may have been removed, or never
//!   created on this branch;
//! * a function, process or event that the module does not have
//!   ([`RuntimeError::UnknownFunction`] and relatives);
//! * an action whose semantics the mock does not implement
//!   ([`RuntimeError::Unimplemented`]) — if it was supposed to return a value,
//!   skipping it yields a wrong result with no error at all.
//!
//! The behaviour for the third case is configured through [`Config`] and
//! [`Unimplemented`].
//!
//! # Variable lifetimes
//!
//! The four `JustMC` scopes differ in lifetime:
//!
//! | Scope | Lives for |
//! |---|---|
//! | `line` | one call frame; the frame ends and the variables are gone |
//! | `local` | one process or event, together with all nested calls |
//! | `game` | the whole run of the code |
//! | `save` | like `game`, but survives a restart (see [`Config::save_file`]) |
//!
//! A function call rests on that separation. The arguments are copied into the
//! callee's line variables, because every frame has its own line variables, and
//! the return value travels through the local variable `ret`, which the callee
//! writes and the caller reads right after the call — that is how it is done in
//! the compiler's `Mir::lower_func_call`.
//!
//! # Example
//!
//! ```no_run
//! use jmcmock::{Program, Runtime};
//!
//! let text = std::fs::read_to_string("tests/case2.json")?;
//! let program = Program::parse(&text)?;
//! let mut runtime = Runtime::new(&program)?;
//! runtime.fire_event("player_join")?;
//! for entry in runtime.world().log().entries() {
//!     println!("{entry}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Tracing
//!
//! The mock reports what it is doing through `tracing`, so a run can be watched
//! step by step instead of inferred from the journal. The level is chosen with
//! `RUST_LOG`; the split is by altitude rather than by file:
//!
//! | Level | Shows |
//! |---|---|
//! | `jmcmock=debug` | what the played program does: the event, its handlers, task scheduling, `wait`, calls, loops, exceptions, and which action is dispatched |
//! | `jmcmock=trace` | how a single operation is computed: arguments, selections, game values, variable reads and writes, and the body of every action |
//!
//! ```bash
//! RUST_LOG=jmcmock=debug ./target/debug/jmcmock module.json
//! RUST_LOG=jmcmock=trace ./target/debug/jmcmock module.json
//! RUST_LOG=trace       ./target/debug/jmcmock module.json   # everything, `jmcdata` included
//! ```
//!
//! Every instrumented function records the arguments it is given, except the
//! ones that would only repeat the caller's span: `Runtime`, `Stream`, `Args`
//! and `Op` are skipped, because a span that dumps the whole runtime on every
//! action hides the very sequence it was meant to show.

#![warn(missing_docs)]

mod actions;
pub mod config;
pub mod error;
mod interp;
mod run;
pub mod scenario;
mod schema;
pub mod scope;
pub mod text;
pub mod value;
pub mod world;

pub use config::{Config, Unimplemented};
pub use error::{ExceptionKind, Result, RuntimeError};
pub use run::{EventData, LocalVariables, Program, Runtime, Stream, TargetMode, event_name};
pub use scenario::{Scenario, ScenarioStep};
pub use world::{Entity, Log, Player, Position, Selection, Target, World, default_world};

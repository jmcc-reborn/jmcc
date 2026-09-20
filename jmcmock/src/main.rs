//! Running a module from the command line.
//!
//! The program does what the `JustMC` server does: loads a module built by
//! `jmcc` and plays back its event handlers. The entry point is `world_start`,
//! as on the server; another event is chosen with `--event`.
//!
//! The result of a run is the world journal, and it goes to standard output. It
//! is printed even when the run ended in an error: the journal shows what the
//! program managed to do before it broke, and the error explains where exactly.
//! A silent "it ran" with not a single entry would be the worst outcome — an
//! empty journal looks like a sound program that does nothing, while in fact
//! not one line executed.
//!
//! The exit code is `SUCCESS` if all went well and `FAILURE` if the run fell
//! over: a runtime error, an unknown event, an unreadable file.
//!
//! # Tracing
//!
//! Progress is reported through `tracing`, driven by `RUST_LOG`; the two levels
//! the mock uses are described in the crate documentation of `jmcmock`:
//!
//! ```bash
//! RUST_LOG=jmcmock=debug ./target/debug/jmcmock module.json   # what the program does
//! RUST_LOG=jmcmock=trace ./target/debug/jmcmock module.json   # how each operation is computed
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};

use jmcdata::generated::EventId;
use jmcmock::config::Unimplemented;
use jmcmock::error::RuntimeError;
use jmcmock::world::World;
use jmcmock::{Config, EventData, Program, Runtime, event_name};

/// The `JustMC` mock runtime: runs a module built by `jmcc`.
#[derive(Debug, Parser)]
#[command(name = "jmcmock", author, version, about, long_about = None)]
struct Cli {
    /// The module — the JSON that `jmcc --emit json` produced.
    module: PathBuf,

    /// The event to play. The flag can be repeated: the events run in the order
    /// they are given.
    #[arg(short, long, value_name = "NAME")]
    event: Vec<String>,

    /// The event's text (`event_chat_message`).
    #[arg(long, value_name = "TEXT")]
    chat: Option<String>,

    /// The event's slot number (`event_slot`).
    #[arg(long, value_name = "N")]
    slot: Option<f64>,

    /// Add a player to the mock world. The flag can be repeated.
    #[arg(short, long, value_name = "NAME")]
    player: Vec<String>,

    /// Do not create the `Dev` player the mock world starts with.
    #[arg(long)]
    no_default_player: bool,

    /// What to do with actions whose semantics the mock does not know.
    #[arg(long, value_enum, default_value_t = Mode::Error)]
    unimplemented: Mode,

    /// The cap on the number of operations per run. A guard against
    /// `repeat_forever`: the mock has no server to interrupt an endless loop at
    /// the end of a tick.
    #[arg(long, value_name = "N")]
    step_limit: Option<usize>,

    /// The file for `save` variables: read at startup, written at the end.
    #[arg(long, value_name = "FILE")]
    save: Option<PathBuf>,

    /// Show what is in the module and exit.
    #[arg(long)]
    list: bool,
}

/// The behaviour for actions the mock does not implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    /// Stop with an error. The default: silently skipping an action that was
    /// supposed to return a value gives a wrong result without an error.
    Error,
    /// Write the action to the journal and carry on.
    Record,
    /// Skip silently.
    Ignore,
}

impl From<Mode> for Unimplemented {
    #[tracing::instrument(level = "debug", skip(mode))]
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Error => Self::Error,
            Mode::Record => Self::Record,
            Mode::Ignore => Self::Ignore,
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .without_time()
        .with_writer(std::io::stderr)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::ENTER
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("jmcmock=warn")),
        )
        .init();

    let cli = Cli::parse();
    tracing::debug!(module = %cli.module.display(), "jmcmock started");
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error);
            ExitCode::FAILURE
        }
    }
}

/// Loads the module, plays the events and prints the journal.
///
/// # Errors
///
/// Returns [`RuntimeError`] for any failure: the module file cannot be read,
/// the JSON does not parse, an event handler fell over, the `save` file could
/// not be written.
#[tracing::instrument(level = "debug", skip(cli))]
fn run(cli: &Cli) -> Result<(), RuntimeError> {
    let text = read_module(&cli.module)?;
    let program = Program::parse(&text)?;
    if cli.list {
        print_module(&program);
        return Ok(());
    }

    let mut runtime = Runtime::with_config(&program, config(cli))?;
    for player in &cli.player {
        runtime.world_mut().add_player(player.clone());
    }

    let played = play(&mut runtime, cli);
    // Saving and printing happen before the outcome is unwrapped: both must
    // happen however the run ended, a failed one included.
    let saved = runtime.persist_save();
    print_log(runtime.world());
    print_summary(&runtime);

    played?;
    saved
}

/// Plays the module's events.
///
/// Without `--event` it plays `world_start`. A module without such a handler
/// stops the run: a launch that did nothing is not a success but an unknown
/// event, and [`hint_events`] suggests the ones that exist.
///
/// # Errors
///
/// Returns [`RuntimeError::UnknownEvent`] if the event is not in the module,
/// and any runtime error from the handler's body.
#[tracing::instrument(level = "debug", skip(runtime, cli))]
fn play(runtime: &mut Runtime<'_>, cli: &Cli) -> Result<(), RuntimeError> {
    let data = event_data(cli);
    if cli.event.is_empty() {
        return play_event(runtime, &event_name(EventId::WorldStart), data);
    }
    for name in &cli.event {
        play_event(runtime, name, data.clone())?;
    }
    Ok(())
}

/// Plays a single event.
///
/// # Errors
///
/// Returns any runtime error from the handler's body.
#[tracing::instrument(level = "debug", skip(runtime, data), fields(name = %name))]
fn play_event<'a>(
    runtime: &mut Runtime<'a>,
    name: &str,
    data: EventData<'a>,
) -> Result<(), RuntimeError> {
    if let Err(error) = runtime.fire_event_with(name, data) {
        hint_events(runtime.program(), &error);
        return Err(error);
    }
    Ok(())
}

/// What the event brought with it.
///
/// An event handler has no arguments in the module: the chat text, the slot
/// number and the rest are properties of the event itself, not values from the
/// code. The mock takes them from the command-line arguments.
///
/// The caller picks the lifetime: none of these values is borrowed from `cli`,
/// so the `'a` here is the `'a` of the runtime that will receive them.
#[tracing::instrument(level = "debug", skip(cli))]
fn event_data<'a>(cli: &Cli) -> EventData<'a> {
    EventData {
        chat_message: cli.chat.clone(),
        slot: cli.slot,
        ..EventData::default()
    }
}

/// Suggests which events the module has when the requested one is not in it.
///
/// Without the hint the error message sends the reader digging through the
/// module for event names by hand, and `--list` exists for exactly that.
#[tracing::instrument(level = "debug", skip(program, error))]
fn hint_events(program: &Program<'_>, error: &RuntimeError) {
    if !matches!(error.root(), RuntimeError::UnknownEvent { .. }) {
        return;
    }
    let names: Vec<String> = program.events().map(event_name).collect();
    note(&if names.is_empty() {
        "модуль не обрабатывает ни одного события — запускать нечего".to_owned()
    } else {
        format!("модуль обрабатывает события: {}", names.join(", "))
    });
}

/// Builds the runtime settings from the command-line arguments.
#[tracing::instrument(level = "debug", skip(cli))]
fn config(cli: &Cli) -> Config {
    let mut config = Config::default().with_unimplemented(cli.unimplemented.into());
    if let Some(limit) = cli.step_limit {
        config = config.with_step_limit(limit);
    }
    if cli.no_default_player {
        config = config.without_default_player();
    }
    if let Some(path) = &cli.save {
        config = config.with_save_file(path.as_path());
    }
    config
}

/// Reads the module file.
///
/// # Errors
///
/// Returns [`RuntimeError::Io`] if the file cannot be read.
#[tracing::instrument(level = "debug", skip(path))]
fn read_module(path: &Path) -> Result<String, RuntimeError> {
    fs::read_to_string(path).map_err(|source| RuntimeError::Io {
        context: format!("module <- {}", path.display()),
        source,
    })
}

/// Prints what the module consists of.
#[tracing::instrument(level = "debug", skip(program))]
fn print_module(program: &Program<'_>) {
    out("события:");
    for event in program.events() {
        out(&format!("  {}", event_name(event)));
    }
    out("функции:");
    for name in program.function_names() {
        out(&format!("  {name}"));
    }
    out("процессы:");
    for name in program.process_names() {
        out(&format!("  {name}"));
    }
}

/// Prints the world journal.
///
/// The journal is the result of the run, which is why it is printed after an
/// error too: it shows what the program managed to do.
#[tracing::instrument(level = "debug", skip(world))]
fn print_log(world: &World) {
    for entry in world.log().entries() {
        out(entry);
    }
    let dropped = world.log().dropped();
    if dropped > 0 {
        note(&format!(
            "журнал переполнен: {dropped} записей не сохранено"
        ));
    }
}

/// Prints the outcome of the run.
///
/// Threads left asleep past the tick budget are reported: without it a run that
/// stopped in the middle looks like a run that finished, and the leaves that
/// never ran are invisible.
#[tracing::instrument(level = "debug", skip(runtime))]
fn print_summary(runtime: &Runtime<'_>) {
    note(&format!(
        "выполнено {} операций, в журнале {} записей",
        runtime.steps(),
        runtime.world().log().entries().len()
    ));
    let pending = runtime.pending_tasks();
    if pending > 0 {
        note(&format!(
            "{pending} потоков ещё выполняется: их пробуждение лежит за бюджетом в {} тиков",
            runtime.config().tick_limit
        ));
    }
}

/// Prints an error with the whole chain of the places it arose in.
#[tracing::instrument(level = "debug", skip(error))]
fn report(error: &RuntimeError) {
    err(&format!("ошибка: {error}"));
}

/// Prints an explanation rather than a result: the journal goes to standard
/// output while explanations go to standard error, so the two can be told
/// apart.
#[tracing::instrument(level = "debug", fields(line = %line))]
fn note(line: &str) {
    err(&format!("jmcmock: {line}"));
}

/// Prints a line to standard output.
#[expect(
    clippy::print_stdout,
    reason = "printing the journal is this program's job: the output is its result"
)]
fn out(line: &str) {
    println!("{line}");
}

/// Prints a line to standard error.
#[expect(
    clippy::print_stderr,
    reason = "explanations and errors go to a separate stream so as not to mix with the journal"
)]
fn err(line: &str) {
    eprintln!("{line}");
}

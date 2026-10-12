//! Logging and crash reporting for `jmc-analyzer` Language Server.

#![expect(
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
    reason = "Logging and panic hooks use fire-and-forget writes"
)]

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use lsp_server::{Message, Notification};
use lsp_types::{LogMessageParams, MessageType, ShowMessageParams};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, fmt};

static CLIENT_SENDER: Mutex<Option<crossbeam_channel::Sender<Message>>> = Mutex::new(None);
static LOG_FILE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sets the active LSP connection sender to allow sending notifications from logging.
pub fn set_client_sender(sender: crossbeam_channel::Sender<Message>) {
    if let Ok(mut guard) = CLIENT_SENDER.lock() {
        *guard = Some(sender);
    }
}

/// Clears the LSP connection sender on shutdown.
pub fn clear_client_sender() {
    if let Ok(mut guard) = CLIENT_SENDER.lock() {
        *guard = None;
    }
}

/// Returns the path to the current log file if one was configured.
#[must_use]
pub fn current_log_file() -> Option<PathBuf> {
    LOG_FILE_PATH.lock().ok()?.clone()
}

/// Sends a `window/logMessage` notification to the connected LSP client.
pub fn log_to_client(typ: MessageType, message: impl Into<String>) {
    let msg = message.into();
    if let Ok(guard) = CLIENT_SENDER.lock()
        && let Some(sender) = guard.as_ref()
    {
        let not = Notification::new(
            "window/logMessage".to_owned(),
            LogMessageParams { typ, message: msg },
        );
        let _ = sender.send(Message::Notification(not));
    }
}

/// Sends a `window/showMessage` notification to the connected LSP client.
pub fn show_message_to_client(typ: MessageType, message: impl Into<String>) {
    let msg = message.into();
    if let Ok(guard) = CLIENT_SENDER.lock()
        && let Some(sender) = guard.as_ref()
    {
        let not = Notification::new(
            "window/showMessage".to_owned(),
            ShowMessageParams { typ, message: msg },
        );
        let _ = sender.send(Message::Notification(not));
    }
}

/// Initializes tracing and file logging for the language server.
///
/// Returns the path to the active log file, if created.
#[must_use]
pub fn init_logging(cli_log_file: Option<PathBuf>) -> Option<PathBuf> {
    let log_path = cli_log_file
        .or_else(|| {
            std::env::var("JMC_ANALYZER_LOG")
                .or_else(|_| std::env::var("JMCC_LOG_FILE"))
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| std::env::temp_dir().join("jmc-analyzer.log"));

    if let Ok(mut guard) = LOG_FILE_PATH.lock() {
        *guard = Some(log_path.clone());
    }

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,jmc_analyzer=debug,jmcc=info"));

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true);

    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let file_layer = fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .with_target(true);

        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer);

        let _ = subscriber.try_init();
    } else {
        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer);

        let _ = subscriber.try_init();
    }

    install_panic_hook();

    Some(log_path)
}

/// Installs a panic hook that logs detailed crash info to stderr, log file, and LSP client.
pub fn install_lsp_panic_hook() {
    install_panic_hook();
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload().downcast_ref::<&str>().map_or_else(
            || {
                info.payload().downcast_ref::<String>().map_or_else(
                    || "Box<dyn Any> (unknown panic payload)".to_string(),
                    Clone::clone,
                )
            },
            |s| (*s).to_string(),
        );

        let location = info.location().map_or_else(
            || "unknown location".to_string(),
            |loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
        );

        let backtrace = format!("{}", std::backtrace::Backtrace::force_capture());

        let crash_text = format!(
            "\n[jmc-analyzer CRASH] Language Server panicked at '{message}' ({location}):\n{backtrace}\n"
        );

        // Write directly to stderr
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(crash_text.as_bytes());
        let _ = stderr.flush();

        // Write directly to log file
        if let Ok(guard) = LOG_FILE_PATH.lock()
            && let Some(log_path) = guard.as_ref()
            && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path)
        {
            let _ = f.write_all(crash_text.as_bytes());
            let _ = f.flush();
        }

        // Notify LSP client if possible
        log_to_client(
            MessageType::ERROR,
            format!("Language server panicked at {location}: {message}"),
        );
        show_message_to_client(
            MessageType::ERROR,
            format!("JMC Analyzer internal error ({location}): {message}"),
        );

        default_hook(info);
    }));
}

/// Helper function to log crash report directly into file.
pub fn append_to_log_file(text: &str) {
    if let Ok(guard) = LOG_FILE_PATH.lock()
        && let Some(log_path) = guard.as_ref()
        && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path)
    {
        let _ = writeln!(f, "{text}");
    }
}

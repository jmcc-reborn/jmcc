//! Custom panic hook and crash reporter for the JMCC compiler.
//!
//! Captures internal compiler panics (ICE), dumps detailed diagnostic info and backtrace
//! to a crash log file, and presents a user-friendly error message with `E0001` in the
//! user's language directing them to submit an issue at `https://github.com/jmcc-reborn/jmcc/issues`.

use std::backtrace::Backtrace;
use std::fs;
use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diagnostic::style::ColorConfig;
use crate::i18n::{Lang, detect_preferred_lang};

/// Primary URL for reporting compiler bugs.
pub const ISSUES_URL: &str = "https://github.com/jmcc-reborn/jmcc/issues";

/// Data captured from a panic event.
#[derive(Debug, Clone)]
pub struct PanicReport {
    pub message: String,
    pub location: String,
    pub backtrace: String,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub command: String,
    pub working_dir: String,
    pub thread_name: String,
    pub timestamp_secs: u64,
}

impl PanicReport {
    /// Extracts report information from a `PanicHookInfo`.
    #[must_use]
    pub fn from_hook_info(info: &PanicHookInfo<'_>) -> Self {
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

        let backtrace = format!("{}", Backtrace::force_capture());

        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        let command = std::env::args().collect::<Vec<_>>().join(" ");
        let working_dir = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let thread_name = std::thread::current().name().unwrap_or("main").to_string();

        Self {
            message,
            location,
            backtrace,
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            command,
            working_dir,
            thread_name,
            timestamp_secs,
        }
    }

    /// Renders the crash report text content to be saved to disk.
    #[must_use]
    pub fn render_log_content(&self) -> String {
        let time_str = format_utc_timestamp(self.timestamp_secs);
        format!(
            "================================================================================\n\
             JMCC Crash Report (Internal Compiler Error - E0001)\n\
             ================================================================================\n\
             JMCC Version:  {}\n\
             OS / Arch:     {} / {}\n\
             Command:       {}\n\
             Working Dir:   {}\n\
             Thread:        {}\n\
             Timestamp:     {} (UNIX epoch: {})\n\
             Issues URL:    {}\n\
             \n\
             Panic Message:\n\
             {}\n\
             \n\
             Panic Location:\n\
             {}\n\
             \n\
             Backtrace:\n\
             {}\n\
             ================================================================================\n",
            self.version,
            self.os,
            self.arch,
            self.command,
            self.working_dir,
            self.thread_name,
            time_str,
            self.timestamp_secs,
            ISSUES_URL,
            self.message,
            self.location,
            self.backtrace,
        )
    }

    /// Attempts to write the crash report log to disk, returning the saved path.
    #[must_use]
    pub fn save_to_file(&self) -> Option<PathBuf> {
        let file_name = format!(
            "jmcc-crash-{}-{}.log",
            self.timestamp_secs,
            std::process::id()
        );
        let content = self.render_log_content();

        // 1. Try current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let target = cwd.join(&file_name);
            if fs::write(&target, &content).is_ok() {
                return Some(target);
            }
        }

        // 2. Fallback to system temp directory
        let temp_target = std::env::temp_dir().join(&file_name);
        if fs::write(&temp_target, &content).is_ok() {
            return Some(temp_target);
        }

        None
    }

    /// Formats the user-facing diagnostic string in the requested or detected language.
    #[must_use]
    pub fn format_user_message(&self, log_path: Option<&Path>, lang: Lang) -> String {
        let colors = ColorConfig::default();
        let path_display = log_path.map_or_else(
            || match lang {
                Lang::Ru => "(не удалось сохранить файл отчёта на диск)".to_string(),
                Lang::En => "(failed to write crash log to disk)".to_string(),
            },
            |p| p.display().to_string(),
        );

        match lang {
            Lang::Ru => {
                let err_tag = colors.red("ошибка[E0001]");
                let header =
                    colors.bold("внутренняя ошибка компилятора (ICE / Internal Compiler Error)");
                let note_tag = colors.bold("примечание");
                let msg_tag = colors.bold("сообщение");
                let report_tag = colors.bold("отчёт об ошибке");
                let help_tag = colors.cyan("помощь");
                let url_styled = colors.bold(ISSUES_URL);

                format!(
                    "\n\
                    {err_tag}: {header}\n  \
                    --> {}\n   \
                    |\n   \
                    = {note_tag}: компилятор неожиданно завершил работу (panic). Это ошибка (баг) в JMCC.\n   \
                    = {msg_tag}: {}\n   \
                    = {report_tag}: сохранён в {}\n\n\
                    {help_tag}: пожалуйста, сообщите об этой ошибке разработчикам JMCC:\n       \
                    {url_styled}\n       \
                    Прикрепите созданный файл отчёта и минимальный пример исходного кода (.jc),\n       \
                    на котором воспроизводится сбой.\n",
                    self.location, self.message, path_display,
                )
            }
            Lang::En => {
                let err_tag = colors.red("error[E0001]");
                let header = colors.bold("internal compiler error (ICE)");
                let note_tag = colors.bold("note");
                let msg_tag = colors.bold("message");
                let report_tag = colors.bold("crash report");
                let help_tag = colors.cyan("help");
                let url_styled = colors.bold(ISSUES_URL);

                format!(
                    "\n\
                    {err_tag}: {header}\n  \
                    --> {}\n   \
                    |\n   \
                    = {note_tag}: the compiler unexpectedly panicked. This is a bug in JMCC.\n   \
                    = {msg_tag}: {}\n   \
                    = {report_tag}: saved to {}\n\n\
                    {help_tag}: please report this issue to the JMCC developers:\n      \
                    {url_styled}\n      \
                    Attach the crash report file and a minimal reproducing source code (.jc) example.\n",
                    self.location, self.message, path_display,
                )
            }
        }
    }
}

/// Converts seconds since UNIX epoch to a human-readable UTC timestamp string.
#[must_use]
pub fn format_utc_timestamp(secs: u64) -> String {
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let final_y = if m <= 2 { y + 1 } else { y };

    format!("{final_y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}:{seconds:02} UTC")
}

/// Installs the JMCC panic hook to capture internal compiler panics.
#[expect(
    clippy::exit,
    reason = "Terminate process with exit code 101 on fatal ICE panic"
)]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let report = PanicReport::from_hook_info(info);
        let log_path = report.save_to_file();
        let lang = detect_preferred_lang();
        let msg = report.format_user_message(log_path.as_deref(), lang);

        let mut stderr = std::io::stderr().lock();
        drop(stderr.write_all(msg.as_bytes()));
        drop(stderr.flush());

        std::process::exit(101);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_utc_timestamp_epoch() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn test_format_utc_timestamp_known_date() {
        assert_eq!(format_utc_timestamp(946_684_800), "2000-01-01 00:00:00 UTC");
        assert_eq!(
            format_utc_timestamp(1_789_862_400),
            "2026-09-20 00:00:00 UTC"
        );
    }

    #[test]
    fn test_panic_report_render_and_format_russian() {
        let report = PanicReport {
            message: "unexpected node in MIR pass".to_string(),
            location: "src/ir/opt/mir/mod.rs:42:10".to_string(),
            backtrace: "   0: jmcc::ir::opt::mir\n   1: jmcc::compile_file".to_string(),
            version: "0.1.0",
            os: "linux",
            arch: "x86_64",
            command: "jmcc compile test.jc".to_string(),
            working_dir: "/home/user/project".to_string(),
            thread_name: "main".to_string(),
            timestamp_secs: 1_790_000_000,
        };

        let log_content = report.render_log_content();
        assert!(log_content.contains("JMCC Crash Report (Internal Compiler Error - E0001)"));
        assert!(log_content.contains("unexpected node in MIR pass"));
        assert!(log_content.contains("src/ir/opt/mir/mod.rs:42:10"));
        assert!(log_content.contains("https://github.com/jmcc-reborn/jmcc/issues"));

        let dummy_path = Path::new("/tmp/jmcc-crash-test.log");
        let user_msg_ru = report.format_user_message(Some(dummy_path), Lang::Ru);
        assert!(user_msg_ru.contains("ошибка[E0001]"));
        assert!(
            user_msg_ru.contains("внутренняя ошибка компилятора (ICE / Internal Compiler Error)")
        );
        assert!(user_msg_ru.contains("https://github.com/jmcc-reborn/jmcc/issues"));
        assert!(user_msg_ru.contains("/tmp/jmcc-crash-test.log"));
    }

    #[test]
    fn test_panic_report_format_english() {
        let report = PanicReport {
            message: "assertion failed in egg runner".to_string(),
            location: "src/ir/opt/hir/math/mod.rs:99:5".to_string(),
            backtrace: "   0: jmcc::ir::opt::hir::math".to_string(),
            version: "0.1.0",
            os: "linux",
            arch: "x86_64",
            command: "jmcc compile test.jc".to_string(),
            working_dir: "/home/user/project".to_string(),
            thread_name: "main".to_string(),
            timestamp_secs: 1_790_000_000,
        };

        let dummy_path = Path::new("/tmp/jmcc-crash-test.log");
        let user_msg_en = report.format_user_message(Some(dummy_path), Lang::En);
        assert!(user_msg_en.contains("error[E0001]"));
        assert!(user_msg_en.contains("internal compiler error (ICE)"));
        assert!(user_msg_en.contains("https://github.com/jmcc-reborn/jmcc/issues"));
        assert!(user_msg_en.contains("/tmp/jmcc-crash-test.log"));
    }

    #[test]
    fn test_panic_report_save_to_file() {
        let report = PanicReport {
            message: "test panic message".to_string(),
            location: "test.rs:1:1".to_string(),
            backtrace: "dummy backtrace".to_string(),
            version: "0.1.0",
            os: "linux",
            arch: "x86_64",
            command: "test".to_string(),
            working_dir: "/tmp".to_string(),
            thread_name: "test".to_string(),
            timestamp_secs: 9_999_999_999,
        };

        let path = report.save_to_file().expect("save crash log");
        assert!(path.exists());
        let content = fs::read_to_string(&path).expect("read crash log");
        assert!(content.contains("test panic message"));
        fs::remove_file(path).unwrap();
    }
}

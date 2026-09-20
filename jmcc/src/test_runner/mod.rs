//! Built-in test runner for the `JustCode` (`.jc`) language.
//!
//! Discovers and executes unit and integration tests using the `jmcmock` runtime,
//! displaying reports formatted like `cargo test` in English or Russian.

use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal as _;
use std::path::Path;
use std::time::Instant;

use color_eyre::Result;
use jmcdata::module::Module;
use jmcmock::config::Unimplemented;
use jmcmock::{Config, Program, Runtime};

use crate::ast::{Ast, Statement, TestAttribute};
use crate::i18n::{Lang, current_lang};
use crate::project::Project;
use crate::{CompileOptions, Target, compile_file};

/// Configuration options for the test runner.
#[derive(Clone, Debug, Default)]
pub struct TestOptions {
    /// Test name filter string.
    pub filter: Option<String>,
    /// Exact matching of test name rather than substring.
    pub exact: bool,
    /// Run only ignored tests.
    pub ignored: bool,
    /// Do not capture runtime log output.
    pub nocapture: bool,
    /// Language / locale for test report ("ru" or "en").
    pub locale: Option<String>,
    /// Disallow modifying lockfile.
    pub locked: bool,
    /// Disallow network access.
    pub offline: bool,
}

/// Status of an individual test execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestStatus {
    /// Test finished successfully.
    Passed,
    /// Test failed with an error message.
    Failed(String),
    /// Test was skipped.
    Ignored(Option<String>),
}

/// Detailed outcome of a single test.
#[derive(Clone, Debug)]
pub struct TestResult {
    /// Display name of the test.
    pub name: String,
    /// Execution status.
    pub status: TestStatus,
    /// Time spent running the test.
    pub duration: std::time::Duration,
    /// Runtime logs emitted during test.
    pub logs: Vec<String>,
}

/// Aggregated outcome of a test suite run.
#[derive(Clone, Debug, Default)]
pub struct TestSummary {
    /// Total tests discovered.
    pub total: usize,
    /// Total passed.
    pub passed: usize,
    /// Total failed.
    pub failed: usize,
    /// Total ignored.
    pub ignored: usize,
    /// Total filtered out by filter string.
    pub filtered_out: usize,
    /// Total wall duration.
    pub duration: std::time::Duration,
    /// List of failed tests with logs.
    pub failures: Vec<TestResult>,
}

impl TestSummary {
    /// Returns true if all executed tests passed.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// Metadata about an identified test case.
#[derive(Clone, Debug)]
pub struct TestCase {
    /// Full callable name in module.
    pub fn_name: String,
    /// Short name for display.
    pub display_name: String,
    /// Test attributes from decorators.
    pub attr: TestAttribute,
}

/// Runs all tests in a project or a single source file.
///
/// # Errors
/// Returns error if compilation, resolution, or execution fails.
pub fn run_tests(input: Option<&str>, options: &TestOptions) -> Result<TestSummary> {
    if let Some(loc) = &options.locale {
        crate::i18n::set_lang_by_name(loc);
    }

    let mut summary = TestSummary::default();
    let start_all = Instant::now();

    match input {
        None => {
            let curr = std::env::current_dir()?;
            let project = Project::find(&curr)?.ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "No test file specified and no 'jmcc.toml' project found in '{}'",
                    curr.display()
                )
            })?;
            run_project_tests(&project, options, &mut summary)?;
        }
        Some(path_str) => {
            let path = Path::new(path_str);
            if path.is_dir() || path.file_name().and_then(|f| f.to_str()) == Some("jmcc.toml") {
                let project = Project::open(path)?;
                run_project_tests(&project, options, &mut summary)?;
            } else {
                run_file_tests(path, options, &mut summary)?;
            }
        }
    }

    summary.duration = start_all.elapsed();
    print_summary_footer(&summary);

    Ok(summary)
}

fn run_project_tests(
    project: &Project,
    options: &TestOptions,
    summary: &mut TestSummary,
) -> Result<()> {
    // Resolve dependencies with dev dependencies
    let graph = project.resolve_dependencies_ext(true, options.locked, options.offline)?;
    let package_roots = graph.to_package_roots();

    let compile_opts = CompileOptions {
        opt_level: 0,
        target: Target::Justmc,
        edition: project.edition(),
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        output: None,
        locale: options.locale.clone().or_else(|| project.manifest.locale()),
        package_roots,
        passes: Vec::new(),
        disable_passes: Vec::new(),
        locked: options.locked,
        offline: options.offline,
        test_mode: true,
        disable_action_limit: project.manifest.disable_action_limit(),
        upload: false,
        ..Default::default()
    };

    // 1. Run unit tests in library / main entry point
    if let Ok(entry) = project.entry_point() {
        run_file_tests_with_opts(&entry, &compile_opts, options, summary)?;
    }

    // 2. Run integration tests in tests/*.jc directory
    let tests_dir = project.root_dir.join("tests");
    if tests_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&tests_dir)
    {
        let mut test_files = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jc") {
                test_files.push(p);
            }
        }
        test_files.sort();

        for test_file in test_files {
            run_file_tests_with_opts(&test_file, &compile_opts, options, summary)?;
        }
    }

    Ok(())
}

fn run_file_tests(path: &Path, options: &TestOptions, summary: &mut TestSummary) -> Result<()> {
    let compile_opts = CompileOptions {
        opt_level: 0,
        target: Target::Justmc,
        edition: 2026,
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        output: None,
        locale: options.locale.clone(),
        package_roots: HashMap::new(),
        passes: Vec::new(),
        disable_passes: Vec::new(),
        locked: options.locked,
        offline: options.offline,
        test_mode: true,
        disable_action_limit: false,
        upload: false,
        ..Default::default()
    };

    run_file_tests_with_opts(path, &compile_opts, options, summary)
}

fn run_file_tests_with_opts(
    path: &Path,
    compile_opts: &CompileOptions,
    options: &TestOptions,
    summary: &mut TestSummary,
) -> Result<()> {
    // Parse AST to discover test functions and attributes
    let ast = crate::ast::import::parse_file_with_packages(
        path,
        compile_opts.edition,
        compile_opts.package_roots.clone(),
    )?;

    let test_cases = discover_test_cases(&ast, path);
    if test_cases.is_empty() {
        return Ok(());
    }

    // Filter test cases based on filter, exact, and ignored options
    let mut filtered_cases = Vec::new();
    for tc in test_cases {
        if options.ignored && !tc.attr.is_ignore {
            summary.filtered_out += 1;
            continue;
        }
        if let Some(f) = &options.filter {
            let matched = if options.exact {
                tc.display_name == *f || tc.fn_name == *f
            } else {
                tc.display_name.contains(f) || tc.fn_name.contains(f)
            };
            if !matched {
                summary.filtered_out += 1;
                continue;
            }
        }
        filtered_cases.push(tc);
    }

    if filtered_cases.is_empty() {
        return Ok(());
    }

    // Compile file to module
    let module = compile_file(path, compile_opts)?;

    print_running_header(filtered_cases.len());

    for tc in filtered_cases {
        summary.total += 1;

        if !options.ignored && tc.attr.is_ignore {
            summary.ignored += 1;
            print_test_result(
                &tc.display_name,
                &TestStatus::Ignored(tc.attr.ignore_reason.clone()),
                std::time::Duration::ZERO,
            );
            continue;
        }

        let start = Instant::now();
        let (status, logs) = execute_test_case(&module, &tc);
        let elapsed = start.elapsed();

        if options.nocapture {
            for log in &logs {
                #[expect(clippy::print_stdout, reason = "nocapture output")]
                {
                    println!("{log}");
                }
            }
        }

        print_test_result(&tc.display_name, &status, elapsed);

        match &status {
            TestStatus::Passed => {
                summary.passed += 1;
            }
            TestStatus::Failed(_) => {
                summary.failed += 1;
                summary.failures.push(TestResult {
                    name: tc.display_name,
                    status,
                    duration: elapsed,
                    logs,
                });
            }
            TestStatus::Ignored(_) => {
                summary.ignored += 1;
            }
        }
    }

    Ok(())
}

fn is_in_file(ast: &Ast, span: &crate::ast::Span, target_path: &Path) -> bool {
    if ast.file_offsets.is_empty() {
        return true;
    }
    let target_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    for (file_path, start, end) in &ast.file_offsets {
        if span.start >= *start && span.start < *end {
            if file_path == target_path {
                return true;
            }
            if let Ok(c1) = file_path.canonicalize()
                && let Ok(c2) = target_path.canonicalize()
                && c1 == c2
            {
                return true;
            }
            if file_path.file_name().and_then(|n| n.to_str()) == Some(target_name) {
                return true;
            }
            return false;
        }
    }
    false
}

fn discover_test_cases(ast: &Ast, target_file: &Path) -> Vec<TestCase> {
    let mut cases = Vec::new();

    for stmt in &ast.statements {
        if let Statement::Function(f) = stmt {
            if !is_in_file(ast, &f.span, target_file) {
                continue;
            }

            let name = ast.strings.resolve(&f.name);
            let short = name.rsplit_once("::").map_or(name, |(_, s)| s);

            let is_test_by_name = short.starts_with("test_")
                || short.starts_with("тест_")
                || short == "test"
                || short == "тест";

            let is_test_by_attr = f.test_attr.as_ref().is_some_and(|a| a.is_test);

            if is_test_by_name || is_test_by_attr {
                let attr = f.test_attr.clone().unwrap_or(TestAttribute {
                    is_test: true,
                    should_panic: false,
                    expected_panic: None,
                    is_ignore: false,
                    ignore_reason: None,
                });

                cases.push(TestCase {
                    fn_name: name.to_owned(),
                    display_name: short.to_owned(),
                    attr,
                });
            }
        }
    }

    cases
}

fn execute_test_case(module: &Module<'_>, tc: &TestCase) -> (TestStatus, Vec<String>) {
    let Ok(program) = Program::from_module(module.clone()) else {
        return (
            TestStatus::Failed("Failed to initialize mock program from module".to_owned()),
            Vec::new(),
        );
    };

    let config = Config::default()
        .with_unimplemented(Unimplemented::Ignore)
        .with_step_limit(100_000);

    let Ok(mut runtime) = Runtime::with_config(&program, config) else {
        return (
            TestStatus::Failed("Failed to create mock runtime".to_owned()),
            Vec::new(),
        );
    };

    // Fire world_start to initialize global state
    drop(runtime.fire_event("world_start"));

    // Call target test function
    let call_res = runtime.call_function(&tc.fn_name, Vec::new());
    let logs: Vec<String> = runtime.world().log().entries().to_vec();

    let status = if tc.attr.should_panic {
        match call_res {
            Err(e) => {
                let err_str = e.to_string();
                tc.attr.expected_panic.as_deref().map_or(TestStatus::Passed, |exp| {
                    if err_str.contains(exp) {
                        TestStatus::Passed
                    } else {
                        TestStatus::Failed(format!(
                            "Test panicked as expected, but panic message did not contain '{exp}'. Got: '{err_str}'"
                        ))
                    }
                })
            }
            Ok(_) => TestStatus::Failed(
                "Test was expected to panic, but completed successfully without error".to_owned(),
            ),
        }
    } else {
        match call_res {
            Ok(_) => TestStatus::Passed,
            Err(e) => TestStatus::Failed(e.to_string()),
        }
    };

    (status, logs)
}

// ---------------------------------------------------------------------------
// Formatting and output rendering
// ---------------------------------------------------------------------------

#[expect(clippy::print_stdout, reason = "test runner header")]
fn print_running_header(count: usize) {
    let lang = current_lang();
    let text = match lang {
        Lang::Ru => format!("\nзапуск {count} тестов"),
        Lang::En => format!("\nrunning {count} tests"),
    };
    println!("{text}");
}

#[expect(clippy::print_stdout, reason = "test result output")]
fn print_test_result(name: &str, status: &TestStatus, _duration: std::time::Duration) {
    let use_color = std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err();
    let lang = current_lang();

    let (status_text, color_code) = match status {
        TestStatus::Passed => match lang {
            Lang::Ru => ("успешно", "\x1b[32m"),
            Lang::En => ("ok", "\x1b[32m"),
        },
        TestStatus::Failed(_) => match lang {
            Lang::Ru => ("СБОЙ", "\x1b[31m"),
            Lang::En => ("FAILED", "\x1b[31m"),
        },
        TestStatus::Ignored(reason) => {
            let label = match lang {
                Lang::Ru => "пропущен",
                Lang::En => "ignored",
            };
            if let Some(r) = reason {
                return println!("test {name} ... \x1b[33m{label}, {r}\x1b[0m");
            }
            (label, "\x1b[33m")
        }
    };

    let prefix = match lang {
        Lang::Ru => "тест",
        Lang::En => "test",
    };

    if use_color {
        println!("{prefix} {name} ... {color_code}{status_text}\x1b[0m");
    } else {
        println!("{prefix} {name} ... {status_text}");
    }
}

#[expect(clippy::print_stdout, reason = "test failure summary")]
fn print_summary_footer(summary: &TestSummary) {
    let use_color = std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err();
    let lang = current_lang();

    if !summary.failures.is_empty() {
        let failures_header = match lang {
            Lang::Ru => "\nошибки:",
            Lang::En => "\nfailures:",
        };
        println!("{failures_header}");

        for failure in &summary.failures {
            let stdout_header = match lang {
                Lang::Ru => format!("\n---- {} вывод ----", failure.name),
                Lang::En => format!("\n---- {} stdout ----", failure.name),
            };
            println!("{stdout_header}");

            if let TestStatus::Failed(err) = &failure.status {
                println!("{err}");
            }
            for log in &failure.logs {
                println!("{log}");
            }
        }

        println!("{failures_header}");
        for failure in &summary.failures {
            println!("    {}", failure.name);
        }
    }

    let result_str = if summary.is_success() {
        match lang {
            Lang::Ru => {
                if use_color {
                    "\x1b[32mуспешно\x1b[0m"
                } else {
                    "успешно"
                }
            }
            Lang::En => {
                if use_color {
                    "\x1b[32mok\x1b[0m"
                } else {
                    "ok"
                }
            }
        }
    } else {
        match lang {
            Lang::Ru => {
                if use_color {
                    "\x1b[31mСБОЙ\x1b[0m"
                } else {
                    "СБОЙ"
                }
            }
            Lang::En => {
                if use_color {
                    "\x1b[31mFAILED\x1b[0m"
                } else {
                    "FAILED"
                }
            }
        }
    };

    let summary_line = match lang {
        Lang::Ru => format!(
            "\nрезультат тестов: {result_str}. Пройдено: {}; сбоев: {}; пропущено: {}; отфильтровано: {}; завершено за {:.2}с\n",
            summary.passed,
            summary.failed,
            summary.ignored,
            summary.filtered_out,
            summary.duration.as_secs_f64()
        ),
        Lang::En => format!(
            "\ntest result: {result_str}. {} passed; {} failed; {} ignored; {} filtered out; finished in {:.2}s\n",
            summary.passed,
            summary.failed,
            summary.ignored,
            summary.filtered_out,
            summary.duration.as_secs_f64()
        ),
    };

    println!("{summary_line}");
}

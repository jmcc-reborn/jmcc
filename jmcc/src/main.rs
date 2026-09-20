use std::{collections::HashMap, fs, path::Path, process::ExitCode};

use clap::Parser;
use jmcc::{CompileOptions, Target, ast, compile_file, compile_project, project::Project};

#[derive(Parser, Debug)]
#[command(name = "jmcc", author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    Compile {
        /// Source file, project directory, or omitted to build the project in current directory
        input: Option<String>,

        #[arg(short, long)]
        output: Option<String>,

        #[arg(short = 'O', long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=3))]
        opt_level: u8,

        #[arg(long, value_delimiter = ',')]
        passes: Vec<String>,

        #[arg(long, value_delimiter = ',')]
        disable_passes: Vec<String>,

        #[arg(long, default_value = "justmc")]
        target: Target,

        #[arg(long, default_value = "2026")]
        edition: u16,

        #[arg(long, value_delimiter = ',', default_values = ["ast,hir,mir,json"])]
        emit: Vec<String>,

        #[arg(long, aliases = ["lang"])]
        locale: Option<String>,

        /// Compilation profile defined in jmcc.toml (e.g. dev, release)
        #[arg(long)]
        profile: Option<String>,

        /// Shortcut for --profile release
        #[arg(long)]
        release: bool,

        /// Require jmcc.lock and disallow updating it
        #[arg(long)]
        locked: bool,

        /// Run without accessing the network
        #[arg(long)]
        offline: bool,

        /// Build test code and preserve test functions
        #[arg(long = "test")]
        test: bool,

        /// Upload compiled module to server ("official" by default, or "webhook")
        #[arg(
            short = 'u',
            long = "upload",
            num_args = 0..=1,
            default_missing_value = "auto",
            value_name = "TARGET"
        )]
        upload: Option<String>,

        /// Upload target service: official (default) or webhook
        #[arg(long = "upload-target", aliases = ["upload_target", "upload-method", "upload_method"])]
        upload_target: Option<jmcc::UploadTarget>,

        /// Custom webhook URL for unofficial upload target
        #[arg(long = "webhook-url", aliases = ["webhook_url", "webhook"])]
        webhook_url: Option<String>,

        /// Disable action limit per line
        #[arg(long, alias = "disable_action_limit")]
        disable_action_limit: bool,
    },
    Test {
        /// Test filter string or target path
        input: Option<String>,

        /// Exactly match the filter
        #[arg(long)]
        exact: bool,

        /// Run only ignored tests
        #[arg(long)]
        ignored: bool,

        /// Do not capture test output
        #[arg(long)]
        nocapture: bool,

        /// Locale / language for diagnostics and test reports ("ru" or "en")
        #[arg(long, aliases = ["lang"])]
        locale: Option<String>,

        /// Require jmcc.lock and disallow updating it
        #[arg(long)]
        locked: bool,

        /// Run without accessing the network
        #[arg(long)]
        offline: bool,
    },
    Update {
        /// Specific package to update, or omit to update all dependencies
        package: Option<String>,
    },
    Format {
        input: String,

        #[arg(long, default_value_t = false)]
        check: bool,
    },
    /// Create a new `JustCode` package at `<PATH>`
    New {
        /// Path where the package should be created
        path: String,

        /// Initialize a new repository for the given version control system [possible values: git, none]
        #[arg(long, default_value = "git")]
        vcs: Option<String>,

        /// Use a binary (application) template [default]
        #[arg(long, conflicts_with = "lib")]
        bin: bool,

        /// Use a library template
        #[arg(long, conflicts_with = "bin")]
        lib: bool,

        /// Edition to set for the package generated [possible values: 2023, 2026]
        #[arg(long, default_value = "2026")]
        edition: u16,

        /// Set the resulting package name, defaults to the directory name
        #[arg(long)]
        name: Option<String>,
    },
}

fn main() -> color_eyre::Result<ExitCode> {
    color_eyre::install()?;
    jmcc::panic_handler::install_panic_hook();

    tracing_subscriber::fmt()
        .without_time()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,egg=warn")),
        )
        .init();

    tracing::debug!("jmcc started");

    let cli = Cli::parse();

    let failed = match cli.command {
        Commands::Compile {
            input,
            output,
            opt_level,
            passes,
            disable_passes,
            target,
            edition,
            emit,
            locale,
            profile,
            release,
            locked,
            offline,
            test,
            upload,
            upload_target,
            webhook_url,
            disable_action_limit,
        } => {
            run_compile(
                input.as_deref(),
                output.as_deref(),
                opt_level,
                &passes,
                &disable_passes,
                target,
                edition,
                &emit,
                locale.as_deref(),
                profile.as_deref(),
                release,
                locked,
                offline,
                test,
                upload.as_deref(),
                upload_target,
                webhook_url.as_deref(),
                disable_action_limit,
            )?;
            false
        }
        Commands::Test {
            input,
            exact,
            ignored,
            nocapture,
            locale,
            locked,
            offline,
        } => run_test(input, exact, ignored, nocapture, locale, locked, offline)?,
        Commands::Update { package } => {
            run_update(package.as_deref())?;
            false
        }
        Commands::Format { input, check } => run_format(&input, check)?,
        Commands::New {
            path,
            vcs,
            bin: _,
            lib,
            edition,
            name,
        } => {
            let pkg_type = if lib {
                jmcc::project::PackageType::Library
            } else {
                jmcc::project::PackageType::Binary
            };
            let opts = jmcc::project::NewPackageOptions {
                name,
                package_type: pkg_type,
                edition,
                vcs,
            };
            jmcc::project::create_package(Path::new(&path), &opts)?;
            false
        }
    };

    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "cli compile arguments"
)]
fn run_compile(
    input: Option<&str>,
    output: Option<&str>,
    opt_level: u8,
    passes: &[String],
    disable_passes: &[String],
    target: Target,
    edition: u16,
    emit: &[String],
    locale: Option<&str>,
    profile: Option<&str>,
    release: bool,
    locked: bool,
    offline: bool,
    test_mode: bool,
    upload: Option<&str>,
    upload_target: Option<jmcc::UploadTarget>,
    webhook_url: Option<&str>,
    disable_action_limit: bool,
) -> color_eyre::Result<()> {
    if let Some(loc) = locale {
        jmcc::i18n::set_lang_by_name(loc);
    }

    let effective_profile = if release {
        "release"
    } else {
        profile.unwrap_or("dev")
    };

    let (should_upload, cli_target) = if let Some(target_str) = upload {
        let parsed_target = if target_str.eq_ignore_ascii_case("auto") {
            upload_target
        } else if let Some(t) = upload_target {
            Some(t)
        } else {
            Some(
                target_str
                    .parse::<jmcc::UploadTarget>()
                    .map_err(|e| color_eyre::eyre::eyre!(e))?,
            )
        };
        (true, parsed_target)
    } else if let Some(t) = upload_target {
        (true, Some(t))
    } else {
        (false, None)
    };

    let mut cli_options = CompileOptions {
        opt_level,
        passes: passes.to_vec(),
        disable_passes: disable_passes.to_vec(),
        target,
        edition,
        emit_ast: emit.iter().any(|e| e == "ast"),
        emit_hir: emit.iter().any(|e| e == "hir"),
        emit_mir: emit.iter().any(|e| e == "mir"),
        emit_json: emit.iter().any(|e| e == "json"),
        output: output.map(str::to_owned),
        locale: locale.map(str::to_owned),
        package_roots: HashMap::new(),
        locked,
        offline,
        test_mode,
        disable_action_limit,
        upload: should_upload,
        upload_target: cli_target,
        webhook_url: webhook_url.map(str::to_owned),
    };

    match input {
        None => {
            let curr = std::env::current_dir()?;
            let project = Project::find(&curr)?.ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "No input file specified and no 'jmcc.toml' project found in '{}'",
                    curr.display()
                )
            })?;
            compile_project(&project, Some(effective_profile), Some(&cli_options))?;
        }
        Some(in_str) => {
            let path = Path::new(in_str);
            if path.is_dir() || path.file_name().and_then(|f| f.to_str()) == Some("jmcc.toml") {
                let project = Project::open(path)?;
                compile_project(&project, Some(effective_profile), Some(&cli_options))?;
            } else {
                tracing::info!("Compiling file: '{in_str}'");
                tracing::info!("Target platform: {target:?}");
                tracing::info!("Optimization level: -O{opt_level}");

                // If file is inside a project, resolve its dependencies to make them available for imports
                let start_dir = path.parent().unwrap_or_else(|| Path::new("."));
                if let Ok(Some(project)) = Project::find(start_dir) {
                    tracing::info!("Detected project context: '{}'", project.name());
                    if cli_options.locale.is_none() {
                        let prof = project.profile_options(effective_profile);
                        cli_options.locale = prof.locale.or_else(|| project.manifest.locale());
                    }
                    if let Ok(graph) = project.resolve_dependencies_ext(false, locked, offline) {
                        cli_options.package_roots = graph.to_package_roots();
                    }
                }

                compile_file(path, &cli_options)?;
            }
        }
    }

    Ok(())
}

#[expect(clippy::fn_params_excessive_bools, reason = "test cli flag options")]
fn run_test(
    input: Option<String>,
    exact: bool,
    ignored: bool,
    nocapture: bool,
    locale: Option<String>,
    locked: bool,
    offline: bool,
) -> color_eyre::Result<bool> {
    let is_path = input.as_deref().is_some_and(|p| {
        let path = Path::new(p);
        path.is_file() || path.is_dir()
    });

    let (target_path, filter) = if is_path {
        (input.as_deref(), None)
    } else {
        (None, input.clone())
    };

    let options = jmcc::test_runner::TestOptions {
        filter,
        exact,
        ignored,
        nocapture,
        locale,
        locked,
        offline,
    };

    let summary = jmcc::test_runner::run_tests(target_path, &options)?;
    Ok(!summary.is_success())
}

fn run_update(package: Option<&str>) -> color_eyre::Result<()> {
    let curr = std::env::current_dir()?;
    let project = Project::find(&curr)?.ok_or_else(|| {
        color_eyre::eyre::eyre!("No 'jmcc.toml' project found in '{}'", curr.display())
    })?;
    project.update_dependencies(package)?;
    Ok(())
}

/// Returns whether any file failed to format, which the caller turns into a
/// non-zero exit code.
fn run_format(input: &str, check: bool) -> color_eyre::Result<bool> {
    tracing::info!("Formatting: '{input}'");

    let input_path = Path::new(input);

    let had_errors = if input_path.is_dir() {
        let mut had_errors = false;
        format_directory(input_path, check, &mut had_errors)?;
        had_errors
    } else {
        format_file(input_path, check)?;
        false
    };

    tracing::info!("Formatting complete!");
    Ok(had_errors)
}

fn format_directory(dir: &Path, check: bool, had_errors: &mut bool) -> color_eyre::Result<()> {
    // Walk directory entries following symlinks.
    for entry in walkdir::WalkDir::new(dir).follow_links(true) {
        let entry = entry.map_err(|e| {
            color_eyre::eyre::eyre!(
                "Failed to read directory '{}': {}",
                e.path().unwrap_or(dir).display(),
                e
            )
        })?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().is_some_and(|ext| ext == "jc")
            && let Err(e) = format_file(path, check)
        {
            tracing::error!("Failed to format '{}': {}", path.display(), e);
            *had_errors = true;
        }
    }
    Ok(())
}

fn format_file(path: &Path, check: bool) -> color_eyre::Result<()> {
    tracing::info!("Formatting file: '{}'", path.display());

    let source = fs::read_to_string(path)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to read file '{}': {}", path.display(), e))?;

    let ast = ast::parser::parse_string(&source, &path.display().to_string(), 2026, 0)?;

    let formatted = ast::format::format(&ast, &source);

    if check {
        if source != formatted {
            tracing::error!("File '{}' is not formatted correctly", path.display());
            return Err(color_eyre::eyre::eyre!("File is not formatted correctly"));
        }
        tracing::info!("File '{}' is already formatted correctly", path.display());
    } else {
        fs::write(path, &formatted)?;
        tracing::info!("File '{}' formatted successfully", path.display());
    }

    Ok(())
}

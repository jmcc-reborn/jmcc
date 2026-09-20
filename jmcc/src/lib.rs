//! The JMCC compiler library: converts `.jc` source into `JustMC` JSON modules.

use std::fs;
use std::path::Path;

pub mod ast;
pub mod diagnostic;
pub mod error;
pub mod i18n;
pub mod ir;
pub mod panic_handler;
pub mod project;
pub mod test_runner;
pub mod upload;
pub mod utils;

pub use upload::UploadTarget;

use crate::ir::ctx::IrCtx;

/// Target platform for code generation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
#[non_exhaustive]
pub enum Target {
    /// Standard `JustMC` platform.
    #[default]
    Justmc,
}

/// Compilation options controlling optimizations, editions, and file emission.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Optimization level (0..=3).
    pub opt_level: u8,
    /// Enabled passes.
    pub passes: Vec<String>,
    /// Disabled passes.
    pub disable_passes: Vec<String>,
    /// Target platform.
    pub target: Target,
    /// Language edition (e.g. 2023 or 2026).
    pub edition: u16,
    /// Emit AST dump to disk.
    pub emit_ast: bool,
    /// Emit HIR dumps to disk.
    pub emit_hir: bool,
    /// Emit MIR dumps to disk.
    pub emit_mir: bool,
    /// Emit JSON module to disk.
    pub emit_json: bool,
    /// Explicit output file path for JSON.
    pub output: Option<String>,
    /// Language/locale for diagnostic messages ("ru" or "en").
    pub locale: Option<String>,
    /// External package roots (`package_name -> source_root_path`).
    pub package_roots: std::collections::HashMap<String, std::path::PathBuf>,
    /// Require jmcc.lock and disallow updating it.
    pub locked: bool,
    /// Disallow network access.
    pub offline: bool,
    /// Build test code and preserve test functions (false = strip all @test functions).
    pub test_mode: bool,
    /// Disable action line limit splitting.
    pub disable_action_limit: bool,
    /// Upload compiled module to server.
    pub upload: bool,
    /// Target service for module uploading (official `JustMC` or Discord webhook).
    pub upload_target: Option<UploadTarget>,
    /// Custom webhook URL for unofficial upload target.
    pub webhook_url: Option<String>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            opt_level: 2,
            passes: Vec::new(),
            disable_passes: Vec::new(),
            target: Target::Justmc,
            edition: 2026,
            emit_ast: true,
            emit_hir: true,
            emit_mir: true,
            emit_json: true,
            output: None,
            locale: None,
            package_roots: std::collections::HashMap::new(),
            locked: false,
            offline: false,
            test_mode: false,
            disable_action_limit: false,
            upload: false,
            upload_target: None,
            webhook_url: None,
        }
    }
}

/// Compiles a `.jc` file into a `JustMC` module.
///
/// # Errors
///
/// Returns an error if file I/O, parsing, semantic analysis, lowering, or codegen fails.
#[expect(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "compilation driver pipeline"
)]
pub fn compile_file(
    input_path: &Path,
    options: &CompileOptions,
) -> color_eyre::Result<jmcdata::module::Module<'static>> {
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = input_path
        .file_stem()
        .ok_or_else(|| color_eyre::eyre::eyre!("Invalid input filename: {}", input_path.display()))?
        .to_str()
        .ok_or_else(|| color_eyre::eyre::eyre!("Non-UTF8 filename: {}", input_path.display()))?;

    let t_start = std::time::Instant::now();
    let source = fs::read_to_string(input_path).map_err(|e| {
        color_eyre::eyre::eyre!(
            "Failed to read source file '{}': {}",
            input_path.display(),
            e
        )
    })?;

    if let Some(loc) = &options.locale {
        crate::i18n::set_lang_by_name(loc);
    } else {
        match ast::lexer::detect_lexer_kind(&source) {
            ast::lexer::LexerKind::Alternate => {
                crate::i18n::set_current_lang(crate::i18n::Lang::Ru);
            }
            ast::lexer::LexerKind::Modern => crate::i18n::set_current_lang(crate::i18n::Lang::En),
        }
    }

    let t0 = std::time::Instant::now();
    let mut ast = ast::import::parse_file_with_packages(
        input_path,
        options.edition,
        options.package_roots.clone(),
    )?;

    if !options.test_mode {
        ast.statements.retain(|stmt| {
            if let ast::Statement::Function(func) = stmt
                && func.test_attr.as_ref().is_some_and(|t| t.is_test)
            {
                return false;
            }
            true
        });
    }

    tracing::info!(
        "Phase 1 [Parse & Imports]: {:.2}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    if options.emit_ast {
        let path = parent.join(format!("{stem}.ast"));
        fs::write(&path, ast.to_string())?;
        tracing::info!("AST saved to {}", path.display());
    }

    let t1 = std::time::Instant::now();
    let mut ir_ctx = IrCtx::new(&ast);
    let types = ast::analyze(&ast, &source, &ir_ctx, options.edition)?;
    tracing::info!(
        "Phase 2 [IrCtx & Semantic]: {:.2}ms",
        t1.elapsed().as_secs_f64() * 1000.0
    );

    let t2 = std::time::Instant::now();
    let hir = ir::hir::ast_to_hir(&ast, &types, &mut ir_ctx, options.edition)?;
    tracing::info!(
        "Phase 3 [Ast to Hir]: {:.2}ms",
        t2.elapsed().as_secs_f64() * 1000.0
    );
    if options.emit_hir {
        let path = parent.join(format!("{stem}_noopt.hir"));
        fs::write(&path, hir.pretty(80))?;
        tracing::info!("HIR (unoptimized) saved to {}", path.display());
    }

    let opt_config = ir::opt::OptConfig::new(
        options.opt_level,
        options.passes.clone(),
        options.disable_passes.clone(),
    );

    let t3 = std::time::Instant::now();
    tracing::info!("Running HIR optimizations...");
    let hir = ir::opt::optimize(
        &hir,
        &opt_config,
        ir::opt::hir::all_module_passes(options.opt_level, options.test_mode),
        ir::opt::hir::all_function_passes(),
    );
    tracing::info!(
        "Phase 4 [HIR Opt]: {:.2}ms",
        t3.elapsed().as_secs_f64() * 1000.0
    );

    if options.emit_hir {
        let path = parent.join(format!("{stem}_opt_preoverloads.hir"));
        fs::write(&path, hir.pretty(80))?;
        tracing::info!("HIR (optimized, pre-overload) saved to {}", path.display());
    }

    let t4 = std::time::Instant::now();
    tracing::info!("Expanding operator overloads...");
    let hir = ir::hir_expand::expand_overloads(&ast, &types, &mut ir_ctx, &hir)
        .map_err(|e| color_eyre::eyre::eyre!("Overload expansion failed: {e}"))?;
    tracing::info!(
        "Phase 5 [Overloads Expand]: {:.2}ms",
        t4.elapsed().as_secs_f64() * 1000.0
    );

    if options.emit_hir {
        let path = parent.join(format!("{stem}.hir"));
        fs::write(&path, hir.pretty(80))?;
        tracing::info!("HIR (optimized + overloads) saved to {}", path.display());
    }

    let t5 = std::time::Instant::now();
    let mir = ir::mir::lower_to_mir(&hir, &ir_ctx, options.edition)?;
    tracing::info!(
        "Phase 6 [Lower to MIR]: {:.2}ms",
        t5.elapsed().as_secs_f64() * 1000.0
    );
    if options.emit_mir {
        let path = parent.join(format!("{stem}_noopt.mir"));
        fs::write(&path, mir.pretty(80))?;
        tracing::info!("MIR (unoptimized) saved to {}", path.display());
    }

    let t6 = std::time::Instant::now();
    tracing::info!("Running MIR optimizations...");
    let mir = ir::opt::optimize(
        &mir,
        &opt_config,
        ir::opt::mir::all_module_passes(),
        ir::opt::mir::all_function_passes(options.edition),
    );
    tracing::info!(
        "Phase 7 [MIR Opt]: {:.2}ms",
        t6.elapsed().as_secs_f64() * 1000.0
    );

    if options.emit_mir {
        let path = parent.join(format!("{stem}.mir"));
        fs::write(&path, mir.pretty(80))?;
        tracing::info!("MIR (optimized) saved to {}", path.display());
    }

    let t7 = std::time::Instant::now();
    let mut cg = ir::codegen::CodeGen::new(options.target, options.edition);
    cg.disable_action_limit = options.disable_action_limit;
    let module = cg.generate(&mir)?;
    tracing::info!(
        "Phase 8 [Codegen]: {:.2}ms",
        t7.elapsed().as_secs_f64() * 1000.0
    );

    let t8 = std::time::Instant::now();
    let json_str = serde_json::to_string_pretty(&module)?;
    if options.emit_json {
        let json_path = options.output.as_ref().map_or_else(
            || parent.join(format!("{stem}.json")),
            |explicit| Path::new(explicit).to_path_buf(),
        );
        fs::write(&json_path, &json_str)?;
        tracing::info!("Compiled JSON saved to {}", json_path.display());
    } else {
        tracing::info!("JSON generation skipped (according to --emit flag)");
    }
    tracing::info!(
        "Phase 9 [JSON Emit]: {:.2}ms",
        t8.elapsed().as_secs_f64() * 1000.0
    );

    if options.upload {
        let target = options
            .upload_target
            .unwrap_or(upload::UploadTarget::Official);
        upload::upload_module(&json_str, target, options.webhook_url.as_deref())?;
    }

    tracing::info!(
        "Total Compilation Time: {:.2}ms",
        t_start.elapsed().as_secs_f64() * 1000.0
    );

    tracing::info!("Compilation successfully finished!");
    Ok(module)
}

/// Compiles a `JustCode` project defined by `Project`.
///
/// Resolves dependencies, applies profile configurations, locates entry point,
/// and invokes `compile_file`.
///
/// # Errors
/// Returns error if project resolution, dependency fetching, or compilation fails.
pub fn compile_project(
    project: &project::Project,
    profile_name: Option<&str>,
    cli_overrides: Option<&CompileOptions>,
) -> color_eyre::Result<jmcdata::module::Module<'static>> {
    let profile = profile_name.unwrap_or("dev");
    tracing::info!(
        "Compiling project '{}' with profile '{}'",
        project.name(),
        profile
    );

    let mut options = CompileOptions {
        edition: project.edition(),
        locale: project.manifest.locale(),
        disable_action_limit: project.manifest.disable_action_limit(),
        upload: project.manifest.auto_upload(),
        upload_target: project.manifest.upload_target(),
        webhook_url: project.manifest.webhook_url(),
        ..Default::default()
    };

    let profile_cfg = project.profile_options(profile);
    profile_cfg.apply_to(&mut options);

    // Apply explicit CLI overrides if any
    if let Some(overrides) = cli_overrides {
        options.locked = overrides.locked;
        options.offline = overrides.offline;
        if overrides.opt_level != 2 {
            options.opt_level = overrides.opt_level;
        }
        if !overrides.passes.is_empty() {
            options.passes = overrides.passes.clone();
        }
        if !overrides.disable_passes.is_empty() {
            options.disable_passes = overrides.disable_passes.clone();
        }
        if overrides.output.is_some() {
            options.output = overrides.output.clone();
        }
        if overrides.locale.is_some() {
            options.locale = overrides.locale.clone();
        }
        if overrides.disable_action_limit {
            options.disable_action_limit = true;
        }
        if overrides.upload {
            options.upload = true;
        }
        if overrides.upload_target.is_some() {
            options.upload_target = overrides.upload_target;
        }
        if overrides.webhook_url.is_some() {
            options.webhook_url = overrides.webhook_url.clone();
        }
    }

    // Resolve dependencies
    let graph = project.resolve_dependencies_ext(false, options.locked, options.offline)?;
    options.package_roots = graph.to_package_roots();

    let entry_point = project.entry_point()?;
    tracing::info!("Project entry point: '{}'", entry_point.display());
    compile_file(&entry_point, &options)
}

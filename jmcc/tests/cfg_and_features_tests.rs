//! Integration tests for `@cfg(feature = "...")` conditional compilation and manifest features.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use jmcc::ast::Statement;
use jmcc::ast::parser::parse_string;
use jmcc::project::Manifest;
use jmcc::{CompileOptions, compile_file, compile_project};

#[test]
fn test_cfg_parser_english_and_russian() {
    let code = r#"
@cfg(feature = "alpha")
function alpha_only() {
    var x = 10;
}

@cfg(фича = "beta")
function beta_only() {
    var y = 20;
}

@cfg(not(feature = "gamma"))
function not_gamma() {
    var z = 30;
}

@cfg(все(фича = "alpha", фича = "beta"))
function all_ab() {}

@cfg(any(feature = "alpha", feature = "beta"))
function any_ab() {}
"#;

    let ast = parse_string(code, "test.jc", 2026, 0).expect("parse string with @cfg");
    assert_eq!(ast.statements.len(), 5);

    for stmt in &ast.statements {
        assert!(
            matches!(stmt, Statement::Cfg(_, _)),
            "Each statement should be wrapped in Statement::Cfg"
        );
    }
}

#[test]
fn test_cfg_filtering_statements_and_blocks() {
    let code = r#"
@cfg(feature = "extra")
function extra_function() {
    var a = 1;
}

function common_function() {
    var base = 100;
    @cfg(feature = "extra")
    var extra_val = 200;
    var finish = base;
}
"#;

    let mut ast_without = parse_string(code, "test.jc", 2026, 0).expect("parse");
    let enabled_empty = HashSet::new();
    ast_without.filter_cfg(&enabled_empty, None);

    // extra_function must be completely removed
    assert_eq!(ast_without.statements.len(), 1);
    if let Statement::Function(f) = &ast_without.statements[0] {
        assert_eq!(ast_without.strings.resolve(&f.name), "common_function");
        // inside common_function: extra_val should be eliminated, remaining only base and finish
        assert_eq!(f.body.len(), 2);
    } else {
        panic!("Expected Function statement");
    }

    let mut ast_with = parse_string(code, "test.jc", 2026, 0).expect("parse");
    let mut enabled_extra = HashSet::new();
    enabled_extra.insert("extra".to_string());
    ast_with.filter_cfg(&enabled_extra, None);

    // extra_function must be present and unwrapped
    assert_eq!(ast_with.statements.len(), 2);
    assert!(
        matches!(&ast_with.statements[0], Statement::Function(f) if ast_with.strings.resolve(&f.name) == "extra_function")
    );
    if let Statement::Function(f) = &ast_with.statements[1] {
        assert_eq!(ast_with.strings.resolve(&f.name), "common_function");
        // inside common_function: base, extra_val, and finish
        assert_eq!(f.body.len(), 3);
        assert!(
            !matches!(&f.body[1], Statement::Cfg(_, _)),
            "Statement should be unwrapped"
        );
    } else {
        panic!("Expected Function statement");
    }
}

#[test]
fn test_cfg_short_circuit_import() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_cfg_test_import");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let main_jc = dir.join("main.jc");
    // Reference a non-existent file behind an inactive @cfg.
    // If it is filtered early, no error will occur when resolving imports!
    fs::write(
        &main_jc,
        r#"
@cfg(feature = "nonexistent_feature")
import "this_file_does_not_exist.jc"

function main() {
    var ok = 42;
}
"#,
    )
    .unwrap();

    let opts = CompileOptions {
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        features: vec![],
        ..Default::default()
    };

    let result = compile_file(&main_jc, &opts);
    assert!(
        result.is_ok(),
        "Compilation should succeed because inactive import is eliminated before disk read: {:?}",
        result.err()
    );
}

#[test]
fn test_cfg_check_typo_validation() {
    let code = r#"
@cfg(feature = "typo_feature")
function test_function() {}
"#;

    let mut ast = parse_string(code, "test.jc", 2026, 0).expect("parse");
    let enabled = HashSet::new();
    let mut known = HashSet::new();
    known.insert("correct_feature".to_string());

    // filter_cfg with known_features logs a warning for typo_feature
    ast.filter_cfg(&enabled, Some(&known));
    // Since typo_feature is not enabled, test_function is filtered out
    assert_eq!(ast.statements.len(), 0);
}

#[test]
fn test_manifest_features_and_optional_dependencies() {
    let manifest_toml = r#"
[project]
name = "my_app"
version = "1.0.0"
edition = 2026

[dependencies]
base_lib = { path = "../base_lib" }
opt_lib = { path = "../opt_lib", optional = true }

[features]
default = ["logging"]
logging = ["dep:opt_lib"]
fast = []
"#;

    let manifest =
        Manifest::from_str(manifest_toml, Path::new("jmcc.toml")).expect("parse manifest");
    assert_eq!(manifest.default_features(), vec!["logging"]);
    let all = manifest.all_features();
    assert!(all.contains(&"logging".to_string()));
    assert!(all.contains(&"fast".to_string()));

    // Test resolving features transitively
    let (enabled, activated_deps, _) = manifest.resolve_features(&["default".to_string()]);
    assert!(enabled.contains("default"));
    assert!(enabled.contains("logging"));
    assert!(activated_deps.contains("opt_lib"));

    // Test resolving with empty features (no default)
    let (enabled_empty, activated_empty, _) = manifest.resolve_features(&[]);
    assert!(enabled_empty.is_empty());
    assert!(activated_empty.is_empty());
}

#[test]
fn test_manifest_invalid_dependency_in_feature_detected() {
    let manifest_toml = r#"
[project]
name = "bad_app"
version = "1.0.0"
edition = 2026

[dependencies]
real_dep = { path = "../real_dep" }

[features]
broken = ["dep:ghost_dep"]
"#;

    let manifest =
        Manifest::from_str(manifest_toml, Path::new("bad_app/jmcc.toml")).expect("parse manifest");
    let val_res = manifest.validate(Path::new("bad_app/jmcc.toml"));
    assert!(
        val_res.is_err(),
        "Manifest validation should fail on unknown dep in dep:ghost_dep"
    );
    let err_str = val_res.err().unwrap().to_string();
    assert!(err_str.contains("ghost_dep"));
}

#[test]
fn test_project_compilation_with_features() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_project_features");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let lib_dir = base.join("extra_lib");
    fs::create_dir_all(lib_dir.join("src")).unwrap();
    fs::write(
        lib_dir.join("jmcc.toml"),
        r#"
[project]
name = "extra_lib"
version = "0.1.0"
edition = 2026
"#,
    )
    .unwrap();
    fs::write(
        lib_dir.join("src").join("lib.jc"),
        r#"
export function helper_calculation() -> number {
    return 12345;
}
"#,
    )
    .unwrap();

    let app_dir = base.join("app");
    fs::create_dir_all(app_dir.join("src")).unwrap();
    fs::write(
        app_dir.join("jmcc.toml"),
        r#"
[project]
name = "app"
version = "0.1.0"
edition = 2026

[dependencies]
extra_lib = { path = "../extra_lib", optional = true }

[features]
default = ["with_extra"]
with_extra = ["dep:extra_lib"]
"#,
    )
    .unwrap();

    fs::write(
        app_dir.join("src").join("main.jc"),
        r#"
@cfg(feature = "with_extra")
import "extra_lib";

function main() {
    @cfg(feature = "with_extra")
    var _x = helper_calculation();
    var _y = 1;
}
"#,
    )
    .unwrap();

    let project = jmcc::project::Project::open(&app_dir).expect("open project");

    // 1. Compile with default features (with_extra enabled -> extra_lib activated and imported)
    let opts = CompileOptions {
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        ..Default::default()
    };
    let res = compile_project(&project, None, Some(&opts));
    assert!(
        res.is_ok(),
        "Compilation with default features should succeed: {:?}",
        res.err()
    );

    // 2. Compile with --no-default-features (with_extra disabled -> extra_lib not resolved, import eliminated)
    let opts_no_default = CompileOptions {
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        no_default_features: true,
        ..Default::default()
    };
    let res_no_default = compile_project(&project, None, Some(&opts_no_default));
    assert!(
        res_no_default.is_ok(),
        "Compilation with --no-default-features should succeed: {:?}",
        res_no_default.err()
    );
}

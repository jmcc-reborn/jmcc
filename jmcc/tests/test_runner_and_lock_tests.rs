//! Integration tests for `jmcc.lock` lockfile management and the built-in test runner.

use std::fs;
use std::path::{Path, PathBuf};

use jmcc::project::Project;
use jmcc::project::error::ProjectError;
use jmcc::project::lockfile::Lockfile;
use jmcc::test_runner::{TestOptions, run_tests};

fn setup_test_workspace(base_dir: &Path) {
    drop(fs::remove_dir_all(base_dir));
    fs::create_dir_all(base_dir).unwrap();

    // 1. Dependency "utils"
    let utils_dir = base_dir.join("utils");
    fs::create_dir_all(utils_dir.join("src")).unwrap();
    let utils_manifest = r#"
        [package]
        name = "utils"
        version = "0.2.0"
        edition = 2026
    "#;
    fs::write(utils_dir.join("jmcc.toml"), utils_manifest).unwrap();
    let utils_code = r#"
        export function compute_val(x: number) -> number {
            return x * 10;
        }
    "#;
    fs::write(utils_dir.join("src").join("lib.jc"), utils_code).unwrap();

    // 2. Main package "core_app"
    let app_dir = base_dir.join("core_app");
    fs::create_dir_all(app_dir.join("src")).unwrap();
    fs::create_dir_all(app_dir.join("tests")).unwrap();
    let app_manifest = r#"
        [project]
        name = "core_app"
        version = "1.0.0"
        edition = 2026

        [dependencies]
        utils = { path = "../utils" }
    "#;
    fs::write(app_dir.join("jmcc.toml"), app_manifest).unwrap();

    let app_code = r#"
        import "utils";

        export function add(a: number, b: number) -> number {
            return a + b;
        }

        @test
        function test_addition() {
            var res = add(2, 3);
        }

        @test
        @should_panic
        function test_expect_panic() {
            throw ERROR "expected failure";
        }

        @test
        @ignore("work in progress")
        function test_pending_feature() {
            var z = 999;
        }

        function test_convention_name() {
            var val = compute_val(5);
        }
    "#;
    fs::write(app_dir.join("src").join("main.jc"), app_code).unwrap();

    // Integration test in tests/
    let itest_code = r#"
        import "utils";

        @test
        function test_integration_flow() {
            var num = compute_val(4);
        }
    "#;
    fs::write(app_dir.join("tests").join("itest.jc"), itest_code).unwrap();
}

#[test]
fn test_lockfile_generation_and_validation() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_lock_test_1");
    setup_test_workspace(&base);

    let app_dir = base.join("core_app");
    let project = Project::open(&app_dir).expect("open core_app project");

    // Initially, no jmcc.lock
    assert!(!project.lockfile_path().exists());

    // Resolve dependencies - should generate jmcc.lock
    let graph = project
        .resolve_dependencies(false)
        .expect("resolve dependencies");
    assert_eq!(graph.packages.len(), 1);
    assert!(
        project.lockfile_path().exists(),
        "jmcc.lock must be created"
    );

    // Read and verify lockfile contents
    let lock = Lockfile::from_file(&project.lockfile_path()).expect("parse generated lockfile");
    assert_eq!(lock.version, 1);
    assert_eq!(lock.packages.len(), 2);

    let app_pkg = lock.packages.iter().find(|p| p.name == "core_app").unwrap();
    assert_eq!(app_pkg.version, "1.0.0");
    assert!(app_pkg.source.is_none());

    let utils_pkg = lock.packages.iter().find(|p| p.name == "utils").unwrap();
    assert_eq!(utils_pkg.version, "0.2.0");
    assert!(utils_pkg.source.as_ref().unwrap().starts_with("path+"));
    assert!(utils_pkg.checksum.as_ref().unwrap().starts_with("sha256:"));
}

#[test]
fn test_lockfile_locked_flag_enforcement() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_lock_test_2");
    setup_test_workspace(&base);

    let app_dir = base.join("core_app");
    let project = Project::open(&app_dir).expect("open core_app project");

    // 1. With --locked when lockfile doesn't exist -> Err(LockfileNotFound)
    let res_no_lock = project.resolve_dependencies_ext(false, true, false);
    match res_no_lock {
        Err(ProjectError::LockfileNotFound { .. }) => {}
        other => panic!("expected LockfileNotFound, got {other:?}"),
    }

    // 2. Normal resolution creates the lockfile
    project
        .resolve_dependencies(false)
        .expect("resolve to create lockfile");
    assert!(project.lockfile_path().exists());

    // 3. With --locked when lockfile is in sync -> Ok
    let res_locked_ok = project.resolve_dependencies_ext(false, true, false);
    assert!(
        res_locked_ok.is_ok(),
        "resolve with valid lockfile must succeed under --locked"
    );

    // 4. Modify dependency version in jmcc.toml -> --locked must fail with LockfileOutOfDate
    let modified_manifest = r#"
        [project]
        name = "core_app"
        version = "1.0.0"
        edition = 2026

        [dependencies]
        utils = { path = "../utils", version = "9.9.9" }
    "#;
    fs::write(app_dir.join("jmcc.toml"), modified_manifest).unwrap();
    let project_modified = Project::open(&app_dir).expect("reopen modified project");
    let res_out_of_date = project_modified.resolve_dependencies_ext(false, true, false);
    match res_out_of_date {
        Err(ProjectError::LockfileOutOfDate { reason, .. }) => {
            assert!(reason.contains("version"));
        }
        other => panic!("expected LockfileOutOfDate, got {other:?}"),
    }
}

#[test]
fn test_test_runner_discovery_and_execution() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_test_runner_1");
    setup_test_workspace(&base);

    let app_dir = base.join("core_app");

    let opts = TestOptions {
        filter: None,
        exact: false,
        ignored: false,
        nocapture: true,
        locale: Some("en".to_string()),
        locked: false,
        offline: false,
    };

    let summary = run_tests(app_dir.to_str(), &opts).expect("run project tests");

    // In core_app/src/main.jc:
    // 1. test_addition -> passes
    // 2. test_expect_panic -> passes (it panics as expected)
    // 3. test_pending_feature -> ignored
    // 4. test_convention_name -> passes
    // In core_app/tests/itest.jc:
    // 5. test_integration_flow -> passes
    assert_eq!(summary.passed, 4, "4 tests should pass");
    assert_eq!(summary.failed, 0, "0 tests should fail");
    assert_eq!(summary.ignored, 1, "1 test should be ignored");
    assert!(summary.is_success());
}

#[test]
fn test_test_runner_filtering_and_ignored() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_test_runner_2");
    setup_test_workspace(&base);

    let app_dir = base.join("core_app");

    // 1. Filter by "addition" with exact = true
    let filter_opts = TestOptions {
        filter: Some("test_addition".to_string()),
        exact: true,
        ignored: false,
        nocapture: true,
        locale: Some("ru".to_string()),
        locked: false,
        offline: false,
    };
    let summary = run_tests(app_dir.to_str(), &filter_opts).expect("run filtered tests");
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.filtered_out, 4);

    // 2. Run only ignored tests
    let ignored_opts = TestOptions {
        filter: None,
        exact: false,
        ignored: true,
        nocapture: true,
        locale: Some("en".to_string()),
        locked: false,
        offline: false,
    };
    let ignored_summary = run_tests(app_dir.to_str(), &ignored_opts).expect("run ignored tests");
    // Only test_pending_feature was ignored, running it should pass
    assert_eq!(ignored_summary.passed, 1);
    assert_eq!(ignored_summary.ignored, 0);
}

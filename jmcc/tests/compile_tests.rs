//! Integration tests for the JMCC compiler: verifies compilation of `.jc` programs across editions and optimization levels.

use std::path::{Path, PathBuf};

use jmcc::{CompileOptions, compile_file};
use jmcdata::generated::ArgType;
use jmcdata::module::{LineValue, Value};

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(relative)
}

fn test_options(edition: u16, opt_level: u8) -> CompileOptions {
    CompileOptions {
        opt_level,
        edition,
        emit_ast: false,
        emit_hir: false,
        emit_mir: false,
        emit_json: false,
        ..Default::default()
    }
}

fn run_large_test<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .expect("failed to spawn large stack test thread")
        .join()
        .expect("test thread panicked");
}

#[test]
fn test_compile_case1() {
    let path = fixture_path("case1.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("Failed to compile case1.jc");
    assert!(
        !module.handlers.is_empty(),
        "case1 module should have handlers"
    );
}

#[test]
fn test_compile_case2() {
    let path = fixture_path("case2.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("Failed to compile case2.jc");
    assert!(
        !module.handlers.is_empty(),
        "case2 module should have handlers"
    );
}

#[test]
fn test_compile_multi_assign() {
    let path = fixture_path("multi_assign.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile multi_assign.jc");
    assert!(
        !module.handlers.is_empty(),
        "multi_assign module should have handlers"
    );
}

#[test]
fn test_compile_interface() {
    let path = fixture_path("interface_test.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile interface_test.jc");
    assert!(
        !module.handlers.is_empty(),
        "interface_test module should have handlers"
    );
}

#[test]
fn test_compile_dce_start_process() {
    let path = fixture_path("dce_start_process.jc");
    let module = compile_file(&path, &test_options(2026, 2))
        .expect("Failed to compile dce_start_process.jc");
    assert!(
        !module.handlers.is_empty(),
        "dce_start_process module should have handlers"
    );
}

#[test]
fn test_compile_raw_action_lint() {
    let path = fixture_path("raw_action_lint.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile raw_action_lint.jc");
    assert!(
        !module.handlers.is_empty(),
        "raw_action_lint module should have handlers"
    );
}

#[test]
fn test_compile_bootstrap_classes() {
    let path = fixture_path("case2.jc");
    // 1. Normal mode: test_mode = false -> all @test functions must be stripped
    let mut opts_normal = test_options(2026, 2);
    opts_normal.test_mode = false;
    let module_normal = compile_file(&path, &opts_normal).expect("compile case2 without test mode");
    for handler in &module_normal.handlers {
        if let LineValue::Fn { name, .. } = &handler.line_value {
            assert!(
                !name.contains("test_") && !name.contains("тест_"),
                "Tests must be stripped in normal compilation: {name}"
            );
        }
    }

    // 2. Test mode: test_mode = true -> preserves @test functions
    let mut opts_test = test_options(2026, 2);
    opts_test.test_mode = true;
    let module_test = compile_file(&path, &opts_test).expect("compile case2 in test mode");
    let has_tests = module_test.handlers.iter().any(|h| {
        if let LineValue::Fn { name, .. } = &h.line_value {
            name.contains("test_") || name.contains("тест_")
        } else {
            false
        }
    });
    assert!(
        has_tests,
        "Test functions must be preserved when test_mode is enabled"
    );
}

#[test]
fn test_compile_generator() {
    let path = fixture_path("generator.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile generator.jc");
    assert!(
        !module.handlers.is_empty(),
        "generator module should have handlers"
    );
}

#[test]
fn test_compile_universaltest() {
    let path = fixture_path("universaltest.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile universaltest.jc");
    assert!(
        !module.handlers.is_empty(),
        "universaltest module should have handlers"
    );
}

#[test]
fn test_compile_nn() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("std")
        .join("ai")
        .join("nn.jc");
    let mut opts = test_options(2026, 2);
    opts.test_mode = true;
    let module = compile_file(&path, &opts).expect("Failed to compile std/ai/nn.jc");
    let has_nn_test = module.handlers.iter().any(|h| {
        if let LineValue::Fn { name, .. } = &h.line_value {
            name.contains("test_neural_network")
        } else {
            false
        }
    });
    assert!(
        has_nn_test,
        "std/ai/nn.jc must preserve test_neural_network in test mode"
    );
}

#[test]
fn test_compile_compass_2023() {
    let path = fixture_path("compass/compass.jc");
    let module = compile_file(&path, &test_options(2023, 2))
        .expect("Failed to compile compass.jc with edition 2023");
    assert!(
        !module.handlers.is_empty(),
        "compass module should have handlers"
    );
}

#[test]
fn test_compile_pvp_2023() {
    run_large_test(|| {
        let path = fixture_path("pvp/main.jc");
        let module = compile_file(&path, &test_options(2023, 2))
            .expect("Failed to compile pvp/main.jc with edition 2023");
        assert!(
            !module.handlers.is_empty(),
            "pvp module should have handlers"
        );
    });
}

#[test]
fn test_compile_nwo_2023() {
    let path = fixture_path("nwo/new-world-order.jc");
    let module = compile_file(&path, &test_options(2023, 2))
        .expect("Failed to compile nwo/new-world-order.jc with edition 2023");
    assert!(
        !module.handlers.is_empty(),
        "nwo module should have handlers"
    );
}

#[test]
fn test_compile_cubed_2023() {
    run_large_test(|| {
        let path = fixture_path("cubed.jc");
        let module = compile_file(&path, &test_options(2023, 2))
            .expect("Failed to compile cubed.jc with edition 2023");
        assert!(
            !module.handlers.is_empty(),
            "cubed module should have handlers"
        );
    });
}

#[test]
fn test_opt_levels() {
    let path = fixture_path("case1.jc");
    for opt in 0..=3 {
        let module = compile_file(&path, &test_options(2026, opt))
            .unwrap_or_else(|e| panic!("Failed to compile case1.jc at -O{opt}: {e}"));
        assert!(
            !module.handlers.is_empty(),
            "module at -O{opt} should have handlers"
        );
    }
}

#[test]
fn test_edition_2026_return_ret_param() {
    let path = fixture_path("raw_action_lint.jc");
    let module =
        compile_file(&path, &test_options(2026, 0)).expect("Failed to compile raw_action_lint.jc");

    // In raw_action_lint.jc, find the function `helper`:
    let helper_fn = module.handlers.iter().find(
        |h| matches!(&h.line_value, LineValue::Fn { name, .. } if name.ends_with("::helper")),
    );
    assert!(
        helper_fn.is_some(),
        "function 'helper' should exist in module"
    );

    if let Some(h) = helper_fn
        && let LineValue::Fn { ref values, .. } = h.line_value
    {
        // In edition 2026, the first parameter in "parameters" array should be `ret` with type `ArgType::Variable`
        let params_val = values
            .get("parameters")
            .expect("helper must have parameters");
        if let Value::Array { values: param_list } = params_val {
            let first = param_list
                .first()
                .and_then(|p| p.as_ref())
                .expect("helper must have at least one param");
            if let Value::Parameter {
                name, param_type, ..
            } = first
            {
                assert_eq!(name.as_ref(), "ret", "first param name must be 'ret'");
                let val_type = match param_type {
                    jmcdata::module::Parameter::Singular { value_type, .. }
                    | jmcdata::module::Parameter::Plural { value_type, .. } => *value_type,
                    jmcdata::module::Parameter::Enum { .. } => ArgType::Enum,
                };
                assert_eq!(
                    val_type,
                    ArgType::Variable,
                    "ret parameter must have type variable (by-ref)"
                );
            } else {
                panic!("expected Value::Parameter for first param");
            }
        } else {
            panic!("expected Value::Array for parameters");
        }
    }
}

#[test]
fn test_edition_2023_return_protocol() {
    let path = fixture_path("case2.jc");
    let module =
        compile_file(&path, &test_options(2023, 0)).expect("Failed to compile case2.jc in 2023");

    // In edition 2023, functions should not have synthetic `ret` parameter
    for h in &module.handlers {
        if let LineValue::Fn { ref values, .. } = h.line_value
            && let Some(Value::Array { values: param_list }) = values.get("parameters")
        {
            for param in param_list.iter().flatten() {
                if let Value::Parameter { name, .. } = param {
                    assert_ne!(
                        name.as_ref(),
                        "ret",
                        "edition 2023 must not prepend 'ret' parameter"
                    );
                }
            }
        }
    }
}

fn example_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest has parent")
        .join("examples")
        .join(relative)
}

#[test]
fn test_compile_example_calculator() {
    let path = example_path("calculator.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile calculator.jc");
    assert!(
        !module.handlers.is_empty(),
        "calculator module should have handlers"
    );
}

#[test]
fn test_compile_example_fibonacci() {
    let path = example_path("fibonacci.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile fibonacci.jc");
    assert!(
        !module.handlers.is_empty(),
        "fibonacci module should have handlers"
    );
}

#[test]
fn test_compile_example_vector3d() {
    let path = example_path("vector3d.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile vector3d.jc");
    assert!(
        !module.handlers.is_empty(),
        "vector3d module should have handlers"
    );
}

#[test]
fn test_compile_example_quest_system() {
    let path = example_path("quest_system.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile quest_system.jc");
    assert!(
        !module.handlers.is_empty(),
        "quest_system module should have handlers"
    );
}

#[test]
fn test_compile_match_try() {
    let path = fixture_path("match_try.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile match_try.jc");
    assert!(
        !module.handlers.is_empty(),
        "match_try module should have handlers"
    );
}

#[test]
fn test_compile_example_match_try() {
    let path = example_path("match_try.jc");
    let module = compile_file(&path, &test_options(2026, 2))
        .expect("Failed to compile examples/match_try.jc");
    assert!(
        !module.handlers.is_empty(),
        "example match_try should have handlers"
    );
}

#[test]
fn test_compile_loops() {
    let path = fixture_path("loops_test.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile loops_test.jc");
    assert!(
        !module.handlers.is_empty(),
        "loops_test module should have handlers"
    );
}

#[test]
fn test_compile_example_loops() {
    let path = example_path("loops.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile examples/loops.jc");
    assert!(
        !module.handlers.is_empty(),
        "example loops should have handlers"
    );
}

#[test]
fn test_compile_lambda() {
    let path = fixture_path("lambda.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("Failed to compile lambda.jc");
    assert!(
        !module.handlers.is_empty(),
        "lambda module should have handlers"
    );
}

#[test]
fn test_compile_dialog() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("std")
        .join("effects")
        .join("text")
        .join("dialog.jc");
    let mut opts = test_options(2026, 2);
    opts.test_mode = true;
    let module = compile_file(&path, &opts).expect("Failed to compile std/effects/text/dialog.jc");
    let has_dialog_test = module.handlers.iter().any(|h| {
        if let LineValue::Fn { name, .. } = &h.line_value {
            name.contains("test_dialog_creation")
        } else {
            false
        }
    });
    assert!(
        has_dialog_test,
        "std/effects/text/dialog.jc must preserve test_dialog_creation in test mode"
    );
}

#[test]
fn test_diagnostics_unknown_action_suggestion() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_diag_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test_action.jc");
    std::fs::write(&file, "function test() { player::play_sond(); }").unwrap();
    let err = compile_file(&file, &test_options(2026, 2))
        .unwrap_err()
        .to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        err.contains("Unknown action 'player::play_sond'"),
        "Error should report unknown action: {err}"
    );
    assert!(
        err.contains("did you mean 'play_sound'?"),
        "Error should suggest 'play_sound': {err}"
    );
}

#[test]
fn test_diagnostics_not_iterable() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_diag_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test_for.jc");
    std::fs::write(&file, "function test() { for x in 123 { } }").unwrap();
    let err = compile_file(&file, &test_options(2026, 2))
        .unwrap_err()
        .to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        err.contains("is not iterable in 'for' loop"),
        "Error should report not iterable: {err}"
    );
}

#[test]
fn test_diagnostics_unknown_method_suggestion() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_diag_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test_method.jc");
    std::fs::write(
        &file,
        "function test() { var arr: array<number> = [1, 2, 3]; var l = arr.lengh(); }",
    )
    .unwrap();
    let err = compile_file(&file, &test_options(2026, 2))
        .unwrap_err()
        .to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        err.contains("Unknown method 'lengh'"),
        "Error should report unknown method: {err}"
    );
    assert!(
        err.contains("did you mean 'len'?"),
        "Error should suggest 'len': {err}"
    );
}

#[test]
fn test_compile_range() {
    let path = fixture_path("range_test.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile range_test.jc");
    assert!(
        !module.handlers.is_empty(),
        "range module should have handlers"
    );
}

#[test]
fn test_compile_compound_assign() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_compound_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test_compound.jc");
    std::fs::write(
        &file,
        r#"
        event<player_join> {
            var s = "hello";
            s += " world";
            var arr = [1, 2];
            arr += [3, 4];
            var n = 10;
            n += 5;
            n -= 2;
            n *= 3;
            n /= 2;
        }
        "#,
    )
    .unwrap();
    let module = compile_file(&file, &test_options(2026, 2))
        .expect("Failed to compile compound assign with text, array, and number");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!module.handlers.is_empty(), "module should have handlers");
}

#[test]
fn test_action_limit_splitting_long_function() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_split_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("long_func.jc");

    // Generate a function with 60 statements (> MAX_ACTIONS_PER_LINE = 43)
    let mut code = String::from("function long_action_func() {\n");
    for i in 0..60 {
        code.push_str(&format!("    local var v{i} = {i};\n"));
    }
    code.push_str("}\n");
    std::fs::write(&file, &code).unwrap();

    let mut options = test_options(2026, 0);
    options.disable_action_limit = false;
    let module = compile_file(&file, &options).expect("Compile long function with splitting");

    let has_jmcc_split = module.handlers.iter().any(|h| {
        if let LineValue::Fn { name, .. } = &h.line_value {
            name.starts_with("jmcc.")
        } else {
            false
        }
    });
    assert!(
        has_jmcc_split,
        "Expected split handler jmcc.N to be generated"
    );

    // Test with disable_action_limit = true
    options.disable_action_limit = true;
    let module_no_split =
        compile_file(&file, &options).expect("Compile long function without splitting");
    let has_jmcc_split_when_disabled = module_no_split.handlers.iter().any(|h| {
        if let LineValue::Fn { name, .. } = &h.line_value {
            name.starts_with("jmcc.")
        } else {
            false
        }
    });
    assert!(
        !has_jmcc_split_when_disabled,
        "Expected NO split handlers when disable_action_limit is true"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_max_handlers_limit_warning() {
    let file = fixture_path("cubed.jc");
    let options = test_options(2023, 2);
    let module = compile_file(&file, &options)
        .expect("Compilation must succeed with warning on max handlers");
    assert!(
        module.handlers.len() > jmcdata::consts::MAX_HANDLERS as usize,
        "cubed.jc has > 345 handlers"
    );
}

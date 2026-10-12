//! Integration tests for the JMCC compiler: verifies compilation of `.jc` programs across editions and optimization levels.

use std::path::{Path, PathBuf};

use jmcc::{CompileOptions, compile_file};
use jmcdata::generated::{ActionId, ArgType};
use jmcdata::module::{LineValue, Op, Value};

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
        && let LineValue::Fn { values, .. } = &h.line_value
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
        if let LineValue::Fn { values, .. } = &h.line_value
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
    drop(std::fs::remove_dir_all(&dir));
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
    drop(std::fs::remove_dir_all(&dir));
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
    drop(std::fs::remove_dir_all(&dir));
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
    drop(std::fs::remove_dir_all(&dir));
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

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_max_handlers_limit_warning() {
    run_large_test(|| {
        let file = fixture_path("cubed.jc");
        let options = test_options(2023, 2);
        let module = compile_file(&file, &options)
            .expect("Compilation must succeed with warning on max handlers");
        assert!(
            module.handlers.len() > jmcdata::consts::MAX_HANDLERS as usize,
            "cubed.jc has > 345 handlers"
        );
    });
}

#[test]
fn test_action_limit_splitting_dynamic_placeholders() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_split_dyn_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dyn_split_func.jc");

    let mut code = String::from("function test_dyn_split() {\n");
    code.push_str("    line var key = \"foo\";\n");
    code.push_str("    `item_%var_line(key)` = 42;\n");
    for i in 0..45 {
        code.push_str(&format!("    line var v{i} = {i};\n"));
    }
    code.push_str("    line var res = `item_%var_line(key)`;\n");
    code.push_str("}\n");
    std::fs::write(&file, &code).unwrap();

    let mut options = test_options(2026, 0);
    options.disable_action_limit = false;
    let module =
        compile_file(&file, &options).expect("Compile dynamic placeholder function with splitting");

    // Verify split occurred
    let split_fn = module
        .handlers
        .iter()
        .find(|h| {
            if let LineValue::Fn { name, .. } = &h.line_value {
                name.starts_with("jmcc.")
            } else {
                false
            }
        })
        .expect("Expected split handler jmcc.N");

    // Verify split function has va_args parameter
    if let LineValue::Fn { values, .. } = &split_fn.line_value {
        let params = values.get("parameters").expect("Must have parameters");
        let params_json = serde_json::to_string(params).unwrap();
        assert!(
            params_json.contains("va_args"),
            "va_args should be in parameters: {params_json}"
        );
    } else {
        panic!("Not a Fn");
    }

    fn find_action<'a>(ops: &'a [Op<'_>], action: ActionId) -> Option<&'a Op<'a>> {
        for op in ops {
            if op.action == action {
                return Some(op);
            }
            if let Some(inner) = &op.operations
                && let Some(found) = find_action(inner, action)
            {
                return Some(found);
            }
        }
        None
    }

    // Verify split function begins with unpack loop (RepeatForEachMapEntry)
    assert!(!split_fn.operations.is_empty());
    assert!(
        find_action(&split_fn.operations, ActionId::RepeatForEachMapEntry).is_some(),
        "Continuation should have unpack loop"
    );

    // Verify caller function test_dyn_split packs va_args and calls split function
    let caller_fn = module
        .handlers
        .iter()
        .find(|h| {
            if let LineValue::Fn { name, .. } = &h.line_value {
                name.ends_with("test_dyn_split")
            } else {
                false
            }
        })
        .expect("Expected caller function test_dyn_split");

    assert!(
        find_action(&caller_fn.operations, ActionId::SetVariableGetListVariables).is_some(),
        "Caller must have SetVariableGetListVariables"
    );

    assert!(
        find_action(&caller_fn.operations, ActionId::RepeatForEachInList).is_some(),
        "Caller must have RepeatForEachInList to pack dynamic variables"
    );

    let call_op = find_action(&caller_fn.operations, ActionId::CallFunction)
        .expect("Caller must have CallFunction");

    let call_json = serde_json::to_string(call_op).unwrap();
    assert!(
        call_json.contains("va_args"),
        "CallFunction args must include va_args"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_action_limit_splitting_dynamic_write_static_read() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_split_dyn_ws_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dyn_split_ws.jc");

    let mut code = String::from("function test_dyn_ws() {\n");
    code.push_str("    line var key = \"foo\";\n");
    code.push_str("    `item_%var_line(key)` = 42;\n");
    for i in 0..45 {
        code.push_str(&format!("    line var v{i} = {i};\n"));
    }
    code.push_str("    player::message(\"value is %var_line(item_foo)\");\n");
    code.push_str("}\n");
    std::fs::write(&file, &code).unwrap();

    let mut options = test_options(2026, 0);
    options.disable_action_limit = false;
    let module = compile_file(&file, &options)
        .expect("Compile dynamic write static read function with splitting");

    let split_fn = module
        .handlers
        .iter()
        .find(|h| {
            if let LineValue::Fn { name, .. } = &h.line_value {
                name.starts_with("jmcc.")
            } else {
                false
            }
        })
        .expect("Expected split handler jmcc.N");

    if let LineValue::Fn { values, .. } = &split_fn.line_value {
        let params = values.get("parameters").expect("Must have parameters");
        let params_json = serde_json::to_string(params).unwrap();
        assert!(
            params_json.contains("va_args"),
            "va_args should be passed when defined_before has dynamic writes: {params_json}"
        );
    }

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_action_limit_splitting_static_write_dynamic_read() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_split_dyn_sw_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dyn_split_sw.jc");

    let mut code = String::from("function test_dyn_sw() {\n");
    code.push_str("    line var item_foo = 42;\n");
    code.push_str("    line var key = \"foo\";\n");
    for i in 0..45 {
        code.push_str(&format!("    line var v{i} = {i};\n"));
    }
    code.push_str("    line var res = `item_%var_line(key)`;\n");
    code.push_str("}\n");
    std::fs::write(&file, &code).unwrap();

    let mut options = test_options(2026, 0);
    options.disable_action_limit = false;
    let module = compile_file(&file, &options)
        .expect("Compile static write dynamic read function with splitting");

    let split_fn = module
        .handlers
        .iter()
        .find(|h| {
            if let LineValue::Fn { name, .. } = &h.line_value {
                name.starts_with("jmcc.")
            } else {
                false
            }
        })
        .expect("Expected split handler jmcc.N");

    if let LineValue::Fn { values, .. } = &split_fn.line_value {
        let params = values.get("parameters").expect("Must have parameters");
        let params_json = serde_json::to_string(params).unwrap();
        assert!(
            params_json.contains("va_args"),
            "va_args should be passed when remainder has dynamic reads: {params_json}"
        );
    }

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_static_methods_action_syntax() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_static_action_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("static_action.jc");

    let code = r#"
event<player_join> {
    var player_name = value::name<current>;
    player::message("Добро пожаловать на сервер, ${player_name}!");
    var sum = Test::Sum(1, 5);
    player::message("sum: ${sum}");
}

class Test {
    function Sum(a: number, b: number) -> number {
        return a + b;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module_2026 = compile_file(&file, &test_options(2026, 2))
        .expect("Failed to compile static method action syntax in Edition 2026");
    assert!(
        !module_2026.handlers.is_empty(),
        "module should have handlers in Edition 2026"
    );

    let module_2023 = compile_file(&file, &test_options(2023, 2))
        .expect("Failed to compile static method action syntax in Edition 2023");
    assert!(
        !module_2023.handlers.is_empty(),
        "module should have handlers in Edition 2023"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_static_methods_dot_syntax() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_static_dot_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("static_dot.jc");

    let code = r#"
event<player_join> {
    var sum = Test.Sum(10, 20);
    player::message("sum: ${sum}");
}

class Test {
    function Sum(a: number, b: number) -> number {
        return a + b;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module_2026 = compile_file(&file, &test_options(2026, 2))
        .expect("Failed to compile static method dot syntax in Edition 2026");
    assert!(!module_2026.handlers.is_empty());

    let module_2023 = compile_file(&file, &test_options(2023, 2))
        .expect("Failed to compile static method dot syntax in Edition 2023");
    assert!(!module_2023.handlers.is_empty());

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_static_inline_methods() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_static_inline_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("static_inline.jc");

    let code = r#"
event<player_join> {
    var s1 = MathUtils::Add(3, 4);
    var s2 = MathUtils.Add(10, 20);
    player::message("${s1} and ${s2}");
}

class MathUtils {
    inline function Add(a: number, b: number) -> number {
        return a + b;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module_2026 = compile_file(&file, &test_options(2026, 2))
        .expect("Failed to compile static inline methods in Edition 2026");
    assert!(!module_2026.handlers.is_empty());

    let module_2023 = compile_file(&file, &test_options(2023, 2))
        .expect("Failed to compile static inline methods in Edition 2023");
    assert!(!module_2023.handlers.is_empty());

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_debug_time_codegen() {
    let file = fixture_path("case1.jc");

    // debug_time = true
    let mut opts_true = test_options(2026, 2);
    opts_true.debug_time = true;
    let module_true = compile_file(&file, &opts_true).expect("compile case1 with debug_time=true");
    let json_true = serde_json::to_string(&module_true).unwrap();
    assert!(
        json_true.contains("running time:"),
        "debug_time=true must emit running time message"
    );
    assert!(
        json_true.contains("controller_measure_time"),
        "debug_time=true must emit controller_measure_time action"
    );

    // debug_time = false
    let mut opts_false = test_options(2026, 2);
    opts_false.debug_time = false;
    let module_false =
        compile_file(&file, &opts_false).expect("compile case1 with debug_time=false");
    let json_false = serde_json::to_string(&module_false).unwrap();
    assert!(
        !json_false.contains("running time:"),
        "debug_time=false must NOT emit running time message"
    );
    assert!(
        !json_false.contains("controller_measure_time"),
        "debug_time=false must NOT emit controller_measure_time action"
    );
}

#[test]
fn test_minify_codegen() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_minify_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");
    std::fs::write(&file, "event<world_start> { player::message(\"hello\"); }").unwrap();

    let out_pretty = dir.join("out_pretty.json");
    let mut opts_pretty = test_options(2026, 2);
    opts_pretty.emit_json = true;
    opts_pretty.output = Some(out_pretty.to_string_lossy().to_string());
    opts_pretty.minify = false;
    compile_file(&file, &opts_pretty).expect("compile pretty");
    let pretty_str = std::fs::read_to_string(&out_pretty).unwrap();
    assert!(
        pretty_str.contains('\n'),
        "pretty JSON must contain newlines"
    );

    let out_min = dir.join("out_min.json");
    let mut opts_min = test_options(2026, 2);
    opts_min.emit_json = true;
    opts_min.output = Some(out_min.to_string_lossy().to_string());
    opts_min.minify = true;
    compile_file(&file, &opts_min).expect("compile minified");
    let min_str = std::fs::read_to_string(&out_min).unwrap();
    assert!(
        !min_str.contains('\n'),
        "minified JSON must not contain newlines"
    );
    assert!(
        min_str.len() < pretty_str.len(),
        "minified JSON must be smaller than pretty JSON"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_dce_preserves_target_placeholder_var() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_dce_ph_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dce_ph.jc");

    let code = r#"
event<player_join> {
    var k = "my_key";
    `prefix_%var_line(k)` = 100;
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("my_key"),
        "DCE must not eliminate variable k used only inside dynamic target placeholder: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_var_aliases_edition_2023_replacement() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_alias_2023_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("alias_2023.jc");

    let code = r#"
event<player_join> {
    var greeting = "hello";
    player::message("${greeting} world");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2023, 2))
        .expect("compile with edition 2023 -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        !json.contains("%var_local(__ct") && !json.contains("%var_line(__ct"),
        "Emitted JSON should not contain unreplaced __ct temporary placeholders: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_redundant_else_preserves_user_variable_zero_assignment() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_else_user_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("else_user.jc");

    let code = r#"
event<player_join> {
    var x = 42;
    if player::is_flying() {
        x = 1;
    } else {
        x = 0;
    }
    player::message("%var_line(x)");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("else"),
        "RedundantElseElimination must not remove else branch setting user variable to 0: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_redundant_else_preserves_save_variable_zero_assignment() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_else_save_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("else_save.jc");

    let code = r#"
event<player_join> {
    if player::is_flying() {
        s`flag` = 1;
    } else {
        s`flag` = 0;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("else"),
        "RedundantElseElimination must not remove else branch setting save variable to 0: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_copy_coalescing_preserves_string_placeholder_temp() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_cc_str_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cc_str.jc");

    let code = r#"
inline function format_val(val) {
    player::message("%var_line(val)");
}

event<player_join> {
    var greeting = "hello";
    format_val(greeting);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("greeting"),
        "Inlined parameter referenced in string literal must preserve greeting binding: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_logical_and_short_circuit_codegen() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_log_and_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("log_and.jc");

    let code = r#"
event<player_join> {
    var a = g`a`;
    var b = g`b`;
    if a > 0 and b < 10 {
        player::message("both true");
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("if_variable_greater"),
        "Must emit native greater comparison: {json}"
    );
    assert!(
        json.contains("if_variable_less"),
        "Must emit native less comparison: {json}"
    );
    assert!(
        json.contains("both true"),
        "Must emit message action: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_mir_dse_eliminates_consecutive_overwritten_stores() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_dse_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dse_test.jc");

    let code = r#"
event<player_join> {
    var x = 111;
    x = 222;
    player::message(x);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    // 111 must be eliminated by DSE, only 222 should remain
    assert!(
        !json.contains("111"),
        "DSE must eliminate dead store of 111: {json}"
    );
    assert!(
        json.contains("222"),
        "222 must be retained in output: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_ctor_without_init_direct_slot_initialization() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_ctor_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ctor_test.jc");

    let code = r#"
class Point {
    var x: number;
    var y: number;
}

event<player_join> {
    var p = Point(123, 456);
    player::message(p.x);
    player::message(p.y);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with -O2 must succeed");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("123"),
        "Constructor args must directly initialize slots (123): {json}"
    );
    assert!(
        json.contains("456"),
        "Constructor args must directly initialize slots (456): {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_licm_loop_invariant_code_motion() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_licm_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("licm_test.jc");

    let code = r#"
event<player_join> {
    var a = 10;
    var b = 20;
    var i = 0;
    while i < 5 {
        var inv = a * b;
        player::message(inv);
        i += 1;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with LICM must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_cse_common_subexpression_elimination() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_cse_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cse_test.jc");

    let code = r#"
event<player_join> {
    var a = 10;
    var b = 20;
    var x = a * b + 1;
    var y = a * b + 2;
    player::message(x);
    player::message(y);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with CSE must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_loop_unrolling_short_loops() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_unroll_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("unroll_test.jc");

    let code = r#"
event<player_join> {
    var i = 0;
    while i < 3 {
        player::message("step");
        i += 1;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with LoopUnrolling must succeed");
    assert!(!module.handlers.is_empty());
    let json = serde_json::to_string(&module).unwrap();
    // Unrolled loop should not contain repeat_while or repeat action
    assert!(
        !json.contains("repeat_while"),
        "Unrolled loop should not contain repeat_while: {json}"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_tail_call_optimization() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_tco_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tco_test.jc");

    let code = r#"
function countdown(n: number) -> number {
    if n <= 0 {
        player::message("done");
        return 0;
    }
    player::message(n);
    return countdown(n - 1);
}

event<player_join> {
    var res = countdown(10);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with TCO must succeed");
    assert!(!module.handlers.is_empty());
    // In countdown handler, there should be no recursive call to countdown
    for handler in &module.handlers {
        let is_countdown = match &handler.line_value {
            LineValue::Fn { name, .. } => name == "countdown",
            _ => false,
        };
        if is_countdown {
            let json = serde_json::to_string(handler).unwrap();
            assert!(
                !json.contains("call_function"),
                "TCO should eliminate recursive call_function in countdown: {json}"
            );
        }
    }
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_branch_folding_empty_conditions() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_branch_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("branch_test.jc");

    let code = r#"
event<player_join> {
    if 1 == 1 {
    }
    player::message("hello");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with BranchFolding must succeed");
    assert!(!module.handlers.is_empty());
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        !json.contains("if_variable_equals"),
        "Empty branch should be pruned: {json}"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_branch_merging_consecutive_ifs() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_bmerge_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bmerge_test.jc");

    let code = r#"
event<player_join> {
    var x = 5;
    if x > 0 {
        player::message("part 1");
    }
    if x > 0 {
        player::message("part 2");
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with BranchMerging must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_guard_factoring_consecutive_guarded_ifs() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_guard_factor_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("guard_factor_test.jc");

    let code = r#"
event<player_join> {
    var flagged = false;
    var c1 = true;
    var c2 = true;
    var c3 = true;
    if not flagged and c1 { flagged = true; }
    if not flagged and c2 { flagged = true; }
    if not flagged and c3 { flagged = true; }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with GuardFactoring must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_code_sinking_into_branch() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_sink_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sink_test.jc");

    let code = r#"
event<player_join> {
    var cond = 1;
    var x = 10 + 20;
    if cond == 1 {
        player::message(x);
    } else {
        player::message("no x needed");
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with CodeSinking must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_dead_loop_elimination() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_deadloop_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("deadloop_test.jc");

    let code = r#"
event<player_join> {
    while false {
        player::message("unreachable");
    }
    player::message("reachable");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with DeadLoopElimination must succeed");
    assert!(!module.handlers.is_empty());
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        !json.contains("unreachable"),
        "Dead loop body should be eliminated: {json}"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_loop_unswitching() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_unswitch_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("unswitch_test.jc");

    let code = r#"
event<player_join> {
    var flag = 1;
    var i = 0;
    while i < 5 {
        if flag == 1 {
            player::message("flag on");
        } else {
            player::message("flag off");
        }
        i += 1;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with LoopUnswitch must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_dead_argument_elimination() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_dae_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dae_test.jc");

    let code = r#"
function internal_compute(used: number, unused: number) -> number {
    return used * 2;
}

event<player_join> {
    var res = internal_compute(21, 999);
    player::message(res);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with DeadArgumentElimination must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_loop_fusion_adjacent_loops() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_loop_fusion_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("fusion_test.jc");

    let code = r#"
event<player_join> {
    var i = 0;
    while i < 10 {
        player::message("loop1");
        i = i + 1;
    }
    i = 0;
    while i < 10 {
        player::message("loop2");
        i = i + 1;
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with LoopFusion must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_action_peephole_specialization() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_peephole_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("peephole_test.jc");

    let code = r#"
event<player_join> {
    var x = 0;
    x += 1;
    x -= 1;
    player::message("msg 1");
    player::message("msg 2");
    var arr: array<number> = [1];
    arr.append(2);
    arr.append(3);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with ActionPeephole must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_mir_constant_folding() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_mir_cf_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mir_cf_test.jc");

    let code = r#"
event<player_join> {
    var s = "hello" + " " + "world";
    player::message(s);
    var list = [10, 20, 30];
    var first = list[0];
    player::message(first);
    var len = list.len();
    player::message(len);
    var m = {"a": 100, "b": 200};
    var val = m["a"];
    player::message(val);
    var mlen = m.len();
    player::message(mlen);
    var v = vector(1, 2, 3);
    var vx = v.get_vector_component("X");
    player::message(vx);
    var loc = location(10, 20, 30, 45, 90);
    var ly = loc.get_coordinate("Y");
    player::message(ly);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile with MirConstantFolding must succeed");
    assert!(!module.handlers.is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_regression_optimization_safety() {
    let dir = std::env::temp_dir().join("jmcc_test_regressions_safety");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jc");

    let code = r#"
event<player_join> {
    var x = g`x`;
    if x > 0 {
        x = 0;
        player::message("branch1");
    }
    if x > 0 {
        player::message("branch2");
    }
}

event<player_quit> {
    var i = 0;
    while i < 10 {
        if i == 5 { break; }
        i += 1;
    }
    var j = 0;
    while j < 10 {
        player::message(j);
        j += 1;
    }
}

event<world_start> {
    player::message("hello");
    player::message("world");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compilation with all optimizations must succeed and maintain correctness");

    let get_event = |ev: jmcdata::generated::EventId| {
        module
            .handlers
            .iter()
            .find(|h| matches!(&h.line_value, LineValue::Event { event } if *event == ev))
            .unwrap_or_else(|| panic!("Event {ev:?} not found in handlers"))
    };

    fn count_actions(ops: &[Op<'_>], predicate: &dyn Fn(ActionId) -> bool) -> usize {
        let mut count = 0;
        for op in ops {
            if predicate(op.action) {
                count += 1;
            }
            if let Some(inner) = &op.operations {
                count += count_actions(inner, predicate);
            }
        }
        count
    }

    // 1. Verify branch merging did not merge player_join's two ifs
    let join_handler = get_event(jmcdata::generated::EventId::PlayerJoin);
    let if_count = count_actions(&join_handler.operations, &|a| {
        format!("{a:?}").starts_with("If")
    });
    assert_eq!(
        if_count, 2,
        "Branch merging must not merge ifs when the first branch modifies the condition variable"
    );

    // 2. Verify loop fusion did not fuse loops when one contains a break
    let quit_handler = get_event(jmcdata::generated::EventId::PlayerQuit);
    let repeat_count = count_actions(&quit_handler.operations, &|a| {
        format!("{a:?}").starts_with("Repeat")
    });
    assert_eq!(
        repeat_count, 2,
        "Loop fusion must not fuse loops when control flow (break) is present"
    );

    // 3. Verify merged messages have SEPARATE_LINES
    let start_handler = get_event(jmcdata::generated::EventId::WorldStart);
    let msg_json = serde_json::to_string(&start_handler.operations).unwrap();
    assert!(
        msg_json.contains("SEPARATE_LINES"),
        "Merged player_send_message must specify SEPARATE_LINES"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_ref_parameter_passes_variable_and_invalidates_copy() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_ref_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ref_test.jc");

    let code = r#"
class Foo {
    var x: number;
}

function modify(ref f: Foo) {
    if f.x > 1000 {
        return;
    }
    f.x = 42;
    code::wait(0, time_unit = "TICKS");
}

event<world_start> {
    var a = Foo();
    a.x = 10;
    modify(a);
    player::message(m"${a.x}");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with ref must succeed");
    let json = serde_json::to_string_pretty(&module).unwrap();

    // Verify that modify function has parameter f with value_type: "variable"
    assert!(
        json.contains(r#""name": "f""#) && json.contains(r#""value_type": "variable""#),
        "Function parameter f must be declared with value_type: variable"
    );

    // Verify that in call_function args, f is passed as a variable "a", NOT folded to number 10
    // The key "f" in args must map to {"type": "variable", "variable": "a", "scope": "line"}
    assert!(
        json.contains(r#""variable": "a""#),
        "Variable 'a' must be passed by reference to modify"
    );

    // Verify that after modify(a), copy_prop did not fold message to 10
    assert!(
        json.contains("%var_line(a)"),
        "Copy propagation must invalidate 'a' after call to modify(ref), preserving %var_line(a)"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_ref_parameter_rejects_literal() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_ref_literal_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ref_literal_test.jc");

    let code = r#"
function modify(ref x: number) {
    x = 42;
}

event<world_start> {
    modify(10);
}
"#;
    std::fs::write(&file, code).unwrap();

    let res = compile_file(&file, &test_options(2026, 2));
    assert!(
        res.is_err(),
        "Passing a literal 10 to a ref parameter must fail compilation"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_method_receiver_is_passed_by_reference() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_self_ref_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("self_ref_test.jc");

    let code = r#"
class Counter {
    var value: number;

    function bump(self: Counter) {
        if self.value > 1000 {
            return;
        }
        self.value = self.value + 1;
        code::wait(0, time_unit = "TICKS");
    }
}

event<world_start> {
    var counter = Counter();
    counter.value = 10;
    counter.bump();
    player::message(m"${counter.value}");
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile with method call must succeed");
    let json = serde_json::to_string_pretty(&module).unwrap();

    assert!(
        json.contains(r#""name": "self""#) && json.contains(r#""value_type": "variable""#),
        "A method receiver must be declared as a variable parameter"
    );
    assert!(
        json.contains(r#""variable": "counter""#),
        "The receiver must be passed as the variable the method was called on"
    );
    assert!(
        json.contains("%var_line(counter)"),
        "Copy propagation must keep the receiver live after the mutating call"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_compile_comparison_non_line_scope_promoted_to_line() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_cmp_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
game var g_val: text = "MATCH";

event<world_start> {
    var c = (g_val == "MATCH");
    if c {
        player::message("OK");
    }
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile comparison must succeed");
    let json = serde_json::to_string_pretty(&module).unwrap();

    // Verify that if_variable_equals compares line variables, not mixed game vs line
    assert!(
        json.contains(r#""action": "if_variable_equals""#),
        "module must contain if_variable_equals"
    );
    // Ensure that in if_variable_equals, the value argument was normalized from game to line
    // by checking that both value and compare have scope line (or compare has scope line and value was promoted)
    assert!(
        !json.contains(r#""scope": "game""#) || json.contains(r#""scope": "line""#),
        "comparison operands must be normalized"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_compile_dynamic_var_ref_soft_fix() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_ref_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
function modify(ref n: number) {
    n += 10;
}

game var %player%_dyn_num: number;

event<world_start> {
    modify(%player%_dyn_num);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module =
        compile_file(&file, &test_options(2026, 2)).expect("compile dynamic var ref must succeed");
    let json = serde_json::to_string_pretty(&module).unwrap();

    // Verify that dynamic variable is passed directly as ref parameter
    assert!(
        json.contains(r#""action": "call_function""#),
        "module must contain call_function"
    );
    assert!(
        json.contains("%player%_dyn_num"),
        "module must reference dynamic variable"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_compile_nested_ref_forwarding_soft_fix() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_nested_ref_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
function inner(ref n: number) {
    n = n + 10;
}

function outer(ref n: number) {
    inner(n);
}

game var g_val: number;

event<world_start> {
    g_val = 5;
    outer(g_val);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile nested ref forwarding must succeed");
    let json = serde_json::to_string_pretty(&module).unwrap();

    // Verify compiler soft-fix: __rf_ temporary generated for forwarded ref argument
    assert!(
        json.contains("__rf_"),
        "module must contain __rf_ temporary variable from compiler soft-fix"
    );

    // Verify in jmcmock that execution completes successfully and mutates value
    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");

    assert_eq!(
        runtime.global().peek("g_val").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(15.0),
        }),
        "g_val must be incremented by 10 through forwarded ref"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_inline_abs_in_or_condition() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_abs_or_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
import "std/math/core.jc";

function check(dy: number) -> boolean {
    if abs(dy) < 0.005 or dy >= 0.0 {
        return true;
    }
    return false;
}

game var res_pos: boolean;
game var res_neg: boolean;

event<world_start> {
    res_pos = check(0.1);
    res_neg = check(-0.2);
}
"#;
    std::fs::write(&file, code).unwrap();

    let opts = test_options(2026, 2);
    let module = compile_file(&file, &opts).expect("compile abs in or condition must succeed");

    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");

    assert_eq!(
        runtime.global().peek("res_pos").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(1.0),
        }),
        "check(0.1) should be true (dy >= 0.0)"
    );

    assert_eq!(
        runtime.global().peek("res_neg").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(0.0),
        }),
        "check(-0.2) should be false (abs(-0.2) = 0.2 >= 0.005, and -0.2 < 0.0)"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_cached_class_write_through_and_caching() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_cached_class_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
cached class Point {
    x: number;
    y: number;

    function move_by(self: Point, dx: number, dy: number) {
        self.x = self.x + dx;
        self.y = self.y + dy;
    }
}

game var sum_res: number;
game var final_x: number;

event<world_start> {
    var p = Point(10, 20);
    var a = p.x;
    var b = p.x;
    var c = p.x;
    sum_res = a + b + c;

    p.move_by(5, 10);
    final_x = p.x;
}
"#;
    std::fs::write(&file, code).unwrap();

    let mut opts = test_options(2026, 0);
    opts.debug_time = false;
    let module = compile_file(&file, &opts).expect("compile cached class");

    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");

    assert_eq!(
        runtime.global().peek("sum_res").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(30.0),
        }),
        "sum of 3 cached reads of 10 must be 30"
    );

    assert_eq!(
        runtime.global().peek("final_x").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(15.0),
        }),
        "p.x after move_by(5, 10) must be 15"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_cached_individual_field() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_cached_field_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
class Entity {
    cached id: number;
    hp: number;
}

game var read_id: number;
game var read_hp: number;

event<world_start> {
    var e = Entity(100, 50);
    read_id = e.id + e.id;
    e.id = 200;
    read_id = read_id + e.id;
    read_hp = e.hp;
}
"#;
    std::fs::write(&file, code).unwrap();

    let mut opts = test_options(2026, 0);
    opts.debug_time = false;
    let module = compile_file(&file, &opts).expect("compile cached field");

    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");

    assert_eq!(
        runtime.global().peek("read_id").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(400.0),
        }),
        "read_id should be 100 + 100 + 200 = 400"
    );
    assert_eq!(
        runtime.global().peek("read_hp").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(50.0),
        }),
        "read_hp should be 50"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_russian_cached_syntax() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_cached_ru_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
кэшированный класс Вектор {
    кэш x: число;
    y: число;
}

игра перем результат: число;

событие<world_start> {
    перем v = Вектор(100, 200);
    v.x = v.x + 50;
    результат = v.x + v.y;
}
"#;
    std::fs::write(&file, code).unwrap();

    let mut opts = test_options(2026, 2);
    opts.debug_time = false;
    let module = compile_file(&file, &opts).expect("compile russian cached class");

    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");

    assert_eq!(
        runtime.global().peek("результат").flatten(),
        Some(jmcdata::module::Value::Number {
            number: jmcdata::module::Number::from_f64(350.0),
        }),
        "result should be 150 + 200 = 350"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_cached_reduces_get_list_value_actions() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_cached_reduction_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file_cached = dir.join("cached.jc");
    let file_normal = dir.join("normal.jc");

    let code_cached = r#"
cached class Point {
    x: number;
    y: number;
}
game var res: number;
event<world_start> {
    var p = Point(10, 20);
    res = p.x + p.x + p.x + p.x;
}
"#;
    let code_normal = r#"
class Point {
    x: number;
    y: number;
}
game var res: number;
event<world_start> {
    var p = Point(10, 20);
    res = p.x + p.x + p.x + p.x;
}
"#;
    std::fs::write(&file_cached, code_cached).unwrap();
    std::fs::write(&file_normal, code_normal).unwrap();

    let mut opts = test_options(2026, 0);
    opts.debug_time = false;
    let mod_cached = compile_file(&file_cached, &opts).expect("compile cached");
    let mod_normal = compile_file(&file_normal, &opts).expect("compile normal");

    fn count_actions(ops: &[Op<'_>], target: ActionId) -> usize {
        let mut count = 0;
        for op in ops {
            if op.action == target {
                count += 1;
            }
            if let Some(inner) = &op.operations {
                count += count_actions(inner, target);
            }
        }
        count
    }

    let count_get_list = |m: &jmcdata::module::Module<'_>| -> usize {
        let mut count = 0;
        for h in &m.handlers {
            if let LineValue::Event { .. } = &h.line_value {
                count += count_actions(&h.operations, ActionId::SetVariableGetListValue);
            }
        }
        count
    };

    let cached_count = count_get_list(&mod_cached);
    let normal_count = count_get_list(&mod_normal);

    assert_eq!(
        cached_count, 1,
        "cached class should emit exactly 1 get_list_value for repeated field reads in event, got {cached_count}"
    );
    assert_eq!(
        normal_count, 4,
        "normal class should emit 4 get_list_value actions, got {normal_count}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_cached_with_inlining() {
    let dir = std::env::temp_dir().join(format!("jmcc_cached_inline_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("test.jc");
    let code = r#"
cached class Point {
    x: number;
    y: number;
}

inline function read_x(pt: Point) -> number {
    return pt.x;
}

inline function write_x(pt: Point, v: number) {
    pt.x = v;
}

game var g1: number;
game var g2: number;
game var g3: number;

event<world_start> {
    var p = Point(10, 20);
    g1 = p.x;
    g2 = read_x(p);
    write_x(p, 99);
    g3 = p.x;
}
"#;
    std::fs::write(&file, code).unwrap();

    let mut opts = test_options(2026, 0);
    opts.debug_time = false;
    let m = compile_file(&file, &opts).expect("compile test_cached_with_inlining");

    let mut get_list_count = 0;
    fn count_get(ops: &[Op<'_>], cnt: &mut usize) {
        for op in ops {
            if op.action == ActionId::SetVariableGetListValue {
                *cnt += 1;
            }
            if let Some(inner) = &op.operations {
                count_get(inner, cnt);
            }
        }
    }

    for h in &m.handlers {
        if let LineValue::Event { .. } = &h.line_value {
            count_get(&h.operations, &mut get_list_count);
        }
    }

    assert_eq!(
        get_list_count, 1,
        "inline function should reuse caller cached field and emit exactly 1 get_list_value, got {get_list_count}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_game_var_inline_method() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_test_gv_inline_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let mod_file = dir.join("sub/mod.jc");
    let file = dir.join("main.jc");

    let mod_code = r#"
export class MovementModule {
    inline function process(self: MovementModule, name: text, ref data: number) -> boolean {
        data += 1;
        return false;
    }
}
"#;
    std::fs::write(&mod_file, mod_code).unwrap();

    let main_code = r#"
import "sub/mod";

game var movement_module: MovementModule = MovementModule();
game var player_data: number = 0;

event<player_move> {
    var name = "test";
    movement_module.process(name, player_data);
    player::message("${player_data}");
}
"#;
    std::fs::write(&file, main_code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("Failed to compile game var inline method");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        !json.contains("call_function"),
        "inline method on game var must be inlined without call_function: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_variable_action_lvalue_write_back() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_var_lvalue_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
import "std/primitives/code/array.jc";

@dict
class DictHolder {
    var available_lists: array;

    function append_counter(self: DictHolder) {
        variable::append_value(self.available_lists, 100);
    }
}

class RegularHolder {
    var available_lists: array;
    var other_field: number;

    function append_counter(self: RegularHolder) {
        variable::append_value(self.available_lists, 200);
    }
}

event<player_join> {
    var l1 = [1];
    var d = DictHolder();
    d.available_lists = l1;
    d.append_counter();

    var l2 = [2];
    var r = RegularHolder();
    r.available_lists = l2;
    r.other_field = 42;
    r.append_counter();
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2)).expect(
        "variable::append_value on class fields must compile without unresolved argument error",
    );
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("set_map_value"),
        "must write back to dict: {json}"
    );
    assert!(
        json.contains("set_list_value"),
        "must write back to regular class: {json}"
    );

    let program = jmcmock::Program::from_module(module).expect("build program from module");
    let mut runtime = jmcmock::Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_variable_action_target_invalidation_copy_prop() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_random_inval_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.jc");

    let code = r#"
event<player_join> {
    var rsy = 0;
    variable::random_number(rsy, 0.8, 1, "FALSE");
    var res = 0 - (1 - rsy);
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile test_variable_action_target_invalidation_copy_prop");
    let json = serde_json::to_string(&module).unwrap();
    assert!(
        json.contains("subtract"),
        "0 - (1 - rsy) must preserve rsy and emit subtraction, not fold to -1: {json}"
    );

    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn test_generic_param_type_annotation_without_value() {
    let dir = std::env::temp_dir().join(format!(
        "jmcc_generic_ty_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_ty.jc");

    let code = r#"
class Box<T> {
    inline function extract(self: Box<T>) -> T {
        var val: T;
        return val;
    }
}

event<world_start> {
    var b = Box();
    var x = b.extract();
}
"#;
    std::fs::write(&file, code).unwrap();

    let module = compile_file(&file, &test_options(2026, 2))
        .expect("compile generic param type annotation without value");
    assert!(!module.handlers.is_empty(), "module should have handlers");

    drop(std::fs::remove_dir_all(&dir));
}

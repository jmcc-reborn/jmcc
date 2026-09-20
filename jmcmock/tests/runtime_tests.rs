//! Integration runtime tests for JMCC compiled modules running inside `jmcmock`.

use std::path::{Path, PathBuf};

use jmcc::{CompileOptions, compile_file};
use jmcmock::config::Unimplemented;
use jmcmock::value::as_number;
use jmcmock::{Config, Program, Runtime};

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest has parent")
        .join("jmcc")
        .join("tests")
        .join(relative)
}

fn std_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest has parent")
        .join("jmcc")
        .join("std")
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

#[test]
fn test_runtime_case1() {
    let path = fixture_path("case1.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile case1.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default().with_unimplemented(Unimplemented::Ignore);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");
    assert!(runtime.steps() > 0, "should execute operations");
}

#[test]
fn test_runtime_case2() {
    let path = fixture_path("case2.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile case2.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let has_message = runtime
        .world()
        .log()
        .entries()
        .iter()
        .any(|entry| entry.contains("test"));
    assert!(has_message, "world log must contain player message 'test'");
}

#[test]
fn test_runtime_interface() {
    let path = fixture_path("interface_test.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile interface_test.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let has_msg1 = runtime
        .world()
        .log()
        .entries()
        .iter()
        .any(|entry| entry.contains("World"));
    let has_msg2 = runtime
        .world()
        .log()
        .entries()
        .iter()
        .any(|entry| entry.contains("Dev"));
    let has_msg3 = runtime
        .world()
        .log()
        .entries()
        .iter()
        .any(|entry| entry.contains("JMCC"));

    assert!(has_msg1, "should emit msg1 'World'");
    assert!(has_msg2, "should emit msg2 'Dev'");
    assert!(has_msg3, "should emit msg3 'JMCC'");
}

#[test]
fn test_runtime_multi_assign() {
    let path = fixture_path("multi_assign.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile multi_assign.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");
    assert!(runtime.steps() > 0, "multi_assign must execute steps");
}

#[test]
fn test_runtime_dce_start_process() {
    let path = fixture_path("dce_start_process.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile dce_start_process.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("world_start").expect("fire world_start");
    assert!(runtime.steps() > 0, "dce_start_process must execute steps");
}

#[test]
fn test_runtime_universaltest() {
    let path = fixture_path("universaltest.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile universaltest.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default().with_unimplemented(Unimplemented::Ignore);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    runtime.fire_event("world_start").expect("fire world_start");
    runtime.fire_event("player_join").expect("fire player_join");

    let has_vec_log = runtime
        .world()
        .log()
        .entries()
        .iter()
        .any(|entry| entry.contains("Vec length:5"));
    assert!(has_vec_log, "universaltest should output 'Vec length:5'");
}

#[test]
fn test_runtime_generator() {
    let path = fixture_path("generator.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile generator.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default()
        .with_unimplemented(Unimplemented::Ignore)
        .with_step_limit(500);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    // Start generator process:
    let proc_name = program
        .process_names()
        .find(|n| n.ends_with("::generate") || *n == "generate")
        .expect("generate process must exist");
    let _ = runtime.start_process(proc_name, Vec::new());
    assert!(runtime.steps() > 0, "generator process must execute steps");
}

#[test]
fn test_runtime_nn() {
    let path = std_path("ai/nn.jc");
    let mut opts = test_options(2026, 2);
    opts.test_mode = true;
    let module = compile_file(&path, &opts).expect("compile nn.jc");
    let program = Program::from_module(module).expect("build program from module");

    // Limit steps to 3000 so the test runs in milliseconds
    let config = Config::default()
        .with_unimplemented(Unimplemented::Ignore)
        .with_step_limit(3000);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    let test_fn = program
        .function_names()
        .find(|n| n.ends_with("::test_neural_network") || *n == "test_neural_network")
        .expect("test_neural_network function must exist");
    let _ = runtime.call_function(test_fn, Vec::new());
    assert!(runtime.steps() > 0, "nn training should execute steps");
}

#[test]
fn test_runtime_function_return_helper() {
    let path = fixture_path("raw_action_lint.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile raw_action_lint.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    let helper_fn = program
        .function_names()
        .find(|n| n.ends_with("::helper") || *n == "helper")
        .expect("helper function must exist");

    let result = runtime
        .call_function(helper_fn, Vec::new())
        .expect("call_function helper");
    let num = result.as_ref().and_then(as_number);
    assert_eq!(
        num,
        Some(1.0),
        "helper function must return 1.0 via ret parameter"
    );
}

#[test]
fn test_runtime_compass_2023() {
    let path = fixture_path("compass/compass.jc");
    let module = compile_file(&path, &test_options(2023, 2)).expect("compile compass.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default()
        .with_unimplemented(Unimplemented::Ignore)
        .with_step_limit(300);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    // repeat::forever runs until step limit:
    let _ = runtime.fire_event("player_join");
    assert!(runtime.steps() >= 100, "compass repeat loop executed steps");
}

fn example_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest has parent")
        .join("examples")
        .join(relative)
}

#[test]
fn test_runtime_calculator() {
    let path = example_path("calculator.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile calculator.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter().any(|e| e.contains("10 + 5 = 15")),
        "log must contain addition result 15, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("10 * 3 = 30")),
        "log must contain multiplication result 30, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("20 / 4 = 5")),
        "log must contain division result 5, got: {log:?}"
    );
}

#[test]
fn test_runtime_fibonacci() {
    let path = example_path("fibonacci.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile fibonacci.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    // F(0)=0, F(1)=1, F(2)=1, F(3)=2, F(4)=3, F(5)=5, F(6)=8
    assert!(
        log.iter().any(|e| e.contains("F(5) =") && e.contains("5")),
        "log must contain F(5) = 5, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("F(6) =") && e.contains("8")),
        "log must contain F(6) = 8, got: {log:?}"
    );
}

#[test]
fn test_runtime_vector3d() {
    let path = example_path("vector3d.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile vector3d.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter()
            .any(|e| e.contains("(5, 7, 9)")
                || (e.contains("5") && e.contains("7") && e.contains("9"))),
        "log must contain vector sum (5, 7, 9), got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("155")),
        "log must contain vector length_sq (5^2+7^2+9^2 = 25+49+81 = 155), got: {log:?}"
    );
}

#[test]
fn test_runtime_quest_system() {
    let path = example_path("quest_system.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile quest_system.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter()
            .any(|e| e.contains("Неплохо") || e.contains("25")),
        "log must contain quest progress status, got: {log:?}"
    );
}

#[test]
fn test_runtime_match_try() {
    let path = fixture_path("match_try.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile match_try.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter().any(|e| e.contains("small")),
        "log must contain match result 'small', got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("caught")),
        "log must contain catch message, got: {log:?}"
    );
    assert!(
        log.iter().all(|e| !e.contains("unreachable")),
        "throw must skip remaining try body, got: {log:?}"
    );
}

#[test]
fn test_runtime_loops() {
    let path = fixture_path("loops_test.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile loops_test.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    // Expected logs: "5", "0", "6", "30", "0"
    assert!(
        log.iter().any(|e| e.contains("5")),
        "log must contain '5', got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("6")),
        "log must contain '6' (list sum), got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("30")),
        "log must contain '30' (map sum), got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("0")),
        "log must contain '0', got: {log:?}"
    );
}

#[test]
fn test_runtime_example_loops() {
    let path = example_path("loops.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile examples/loops.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter().any(|e| e.contains("яблоко")),
        "log must contain 'яблоко', got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("Alice")),
        "log must contain 'Alice', got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("Найден банан")),
        "log must contain 'Найден банан', got: {log:?}"
    );
}

#[test]
fn test_runtime_lambda() {
    let path = fixture_path("lambda.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile lambda.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let log = runtime.world().log().entries();
    assert!(
        log.iter().any(|e| e.contains("11")),
        "log must contain 11, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("50")),
        "log must contain 50, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("10")),
        "log must contain 10, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("14")),
        "log must contain 14, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("150")),
        "log must contain 150, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("225")),
        "log must contain 225, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("310")),
        "log must contain 310, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("42")),
        "log must contain 42, got: {log:?}"
    );
}

#[test]
fn test_runtime_dialog() {
    let path = std_path("effects/text/dialog.jc");
    let mut opts = test_options(2026, 2);
    opts.test_mode = true;
    let module = compile_file(&path, &opts).expect("compile dialog.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    let test_fn = program
        .function_names()
        .find(|n| n.ends_with("::test_dialog_creation") || *n == "test_dialog_creation")
        .expect("test_dialog_creation function must exist");
    let _ = runtime
        .call_function(test_fn, Vec::new())
        .expect("call_function test_dialog_creation");
    assert!(runtime.steps() > 0, "dialog test should execute steps");
}

#[test]
fn test_runtime_ranges() {
    let path = fixture_path("range_test.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile range_test.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default().with_unimplemented(Unimplemented::Ignore);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");
    assert!(runtime.steps() > 0, "range test should execute steps");
}

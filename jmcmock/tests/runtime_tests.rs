//! Integration runtime tests for JMCC compiled modules running inside `jmcmock`.

use std::path::{Path, PathBuf};

use jmcc::{CompileOptions, compile_file};
use jmcdata::generated::ActionId;
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

    let entries = runtime.world().log().entries();
    let has_vec_log = entries.iter().any(|entry| entry.contains("Vec length:5"));
    assert!(has_vec_log, "universaltest should output 'Vec length:5'");

    let has_vec_desc = entries
        .iter()
        .any(|entry| entry.contains("Vector2DEx(3, 4)"));
    assert!(
        has_vec_desc,
        "universaltest should output 'Vector2DEx(3, 4)'"
    );

    let has_add_txt = entries.iter().any(|entry| entry.contains("Add text:12"));
    assert!(has_add_txt, "universaltest should output 'Add text:12'");

    let has_gen = entries
        .iter()
        .any(|entry| entry.contains("generic_identity_works"));
    assert!(
        has_gen,
        "universaltest should output generic identity result"
    );

    let has_item_var = entries
        .iter()
        .any(|entry| entry.contains("Variable 'item' method calls work properly"));
    assert!(has_item_var, "universaltest should output item var success");

    let has_absent = entries
        .iter()
        .any(|entry| entry.contains("Absent map key equals 0"));
    assert!(
        has_absent,
        "universaltest should output absent key equals 0"
    );

    let has_lambda = entries
        .iter()
        .any(|entry| entry.contains("Lambda result:40"));
    assert!(has_lambda, "universaltest should output lambda result");

    let has_block_lambda = entries
        .iter()
        .any(|entry| entry.contains("Block lambda result:31"));
    assert!(
        has_block_lambda,
        "universaltest should output block lambda result"
    );

    let has_tern_bin = entries
        .iter()
        .any(|entry| entry.contains("Ternary with binary condition:100"));
    assert!(
        has_tern_bin,
        "universaltest should output ternary binary condition result"
    );

    let has_match = entries
        .iter()
        .any(|entry| entry.contains("Match results:smallhuge"));
    assert!(has_match, "universaltest should output match result");

    let has_ru = entries
        .iter()
        .any(|entry| entry.contains("Russian syntax result:положительное"));
    assert!(has_ru, "universaltest should output Russian syntax result");

    let has_err = entries
        .iter()
        .any(|entry| entry.contains("custom_error_message"));
    assert!(has_err, "universaltest should catch error");

    let has_split_success = entries
        .iter()
        .any(|entry| entry.contains("LINE_SPLIT_SUCCESS:999"));
    assert!(
        has_split_success,
        "universaltest should preserve line context across jmcc.n split (>50 actions)"
    );

    assert!(
        entries.iter().any(|e| e.contains("IN_ARRAY_SUCCESS")),
        "universaltest should evaluate 'in' for arrays"
    );
    assert!(
        entries.iter().any(|e| e.contains("IN_MAP_SUCCESS")),
        "universaltest should evaluate 'in' for maps"
    );
    assert!(
        entries.iter().any(|e| e.contains("IN_TEXT_SUCCESS")),
        "universaltest should evaluate 'in' for text"
    );
    assert!(
        entries.iter().any(|e| e.contains("IN_RANGE_SUCCESS")),
        "universaltest should evaluate 'in' for ranges"
    );
    assert!(
        entries.iter().any(|e| e.contains("IN_RANGE_INC_SUCCESS")),
        "universaltest should evaluate 'in' for inclusive ranges"
    );
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
    drop(runtime.start_process(proc_name, Vec::new()));
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
    drop(runtime.call_function(test_fn, Vec::new()));
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
    drop(runtime.fire_event("player_join"));
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
        log.iter().any(|e| e.contains("F(5) =") && e.contains('5')),
        "log must contain F(5) = 5, got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("F(6) =") && e.contains('8')),
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
                || (e.contains('5') && e.contains('7') && e.contains('9'))),
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
        log.iter().any(|e| e.contains('5')),
        "log must contain '5', got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains('6')),
        "log must contain '6' (list sum), got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains("30")),
        "log must contain '30' (map sum), got: {log:?}"
    );
    assert!(
        log.iter().any(|e| e.contains('0')),
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
    drop(
        runtime
            .call_function(test_fn, Vec::new())
            .expect("call_function test_dialog_creation"),
    );
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

#[test]
fn test_runtime_scenario_multiplayer() {
    let path = fixture_path("universaltest.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile universaltest.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default().with_unimplemented(Unimplemented::Ignore);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    let scenario_json = r#"[
        { "type": "event", "event": "world_start" },
        { "type": "add_player", "name": "Alice", "x": 10.0, "y": 64.0, "z": 20.0 },
        { "type": "event", "event": "player_join", "player": "Alice" },
        { "type": "event", "event": "player_chat", "player": "Alice", "chat": "@run" },
        { "type": "add_player", "name": "Bob", "x": 5.0, "y": 64.0, "z": 5.0 },
        { "type": "event", "event": "player_join", "player": "Bob" },
        { "type": "assert_log", "contains": "Добро пожаловать!" },
        { "type": "clear_log" },
        { "type": "event", "event": "player_click_inventory", "player": "Bob", "slot": 0.0, "title": "TestGUI" }
    ]"#;

    let scenario = jmcmock::Scenario::parse(scenario_json).expect("parse scenario");
    scenario.run(&mut runtime).expect("execute scenario");

    assert_eq!(runtime.world().players().len(), 3); // Dev + Alice + Bob
    assert!(runtime.steps() > 0);
}

#[test]
fn test_runtime_scenario_assertion_failure() {
    let path = fixture_path("universaltest.jc");
    let module = compile_file(&path, &test_options(2026, 2)).expect("compile universaltest.jc");
    let program = Program::from_module(module).expect("build program from module");

    let config = Config::default().with_unimplemented(Unimplemented::Ignore);
    let mut runtime = Runtime::with_config(&program, config).expect("build runtime");

    let scenario_json = r#"[
        { "type": "assert_log", "contains": "NON_EXISTENT_LOG_STRING" }
    ]"#;

    let scenario = jmcmock::Scenario::parse(scenario_json).expect("parse scenario");
    let err = scenario.run(&mut runtime).unwrap_err();
    assert!(matches!(
        err,
        jmcmock::RuntimeError::AssertionFailed { step: 1, .. }
    ));
}

#[test]
fn test_runtime_basic_world_and_player_actions() {
    let mut world = jmcmock::World::new();
    let _player = world.add_player("Alice");
    assert_eq!(world.players().len(), 1);
    assert_eq!(world.players()[0].name, "Alice");
    assert_eq!(world.players()[0].health, 20.0);
    assert_eq!(world.players()[0].game_mode, "SURVIVAL");

    let player = world.player_mut(0).unwrap();
    player.damage(5.0);
    assert_eq!(player.health, 15.0);
    player.heal(10.0);
    assert_eq!(player.health, 20.0);

    assert_eq!(world.block_at(10, 64, 20), None);
    world.set_block(10, 64, 20, "diamond_block");
    assert_eq!(world.block_at(10, 64, 20), Some("diamond_block"));
    let broken = world.break_block(10, 64, 20);
    assert_eq!(broken, Some("diamond_block".to_owned()));
    assert_eq!(world.block_at(10, 64, 20), None);

    world.set_world_time(6000.0);
    assert_eq!(world.world_time(), 6000.0);
    world.set_weather("RAIN");
    assert_eq!(world.weather(), "RAIN");

    let _entity = world.add_entity(
        "minecraft:creeper",
        jmcmock::Position::coords(0.0, 64.0, 0.0),
    );
    assert_eq!(world.entities().len(), 1);
    assert_eq!(world.entities()[0].kind, "minecraft:creeper");
    let entity = world.entity_mut(0).unwrap();
    entity.damage(8.0);
    assert_eq!(entity.health, 12.0);
    entity.heal(3.0);
    assert_eq!(entity.health, 15.0);
    assert!(world.remove_entity(0));
    assert_eq!(world.entities().len(), 0);
}

#[test]
fn test_runtime_compound_subscript_and_property_assignment() {
    let temp_dir = std::env::temp_dir().join("jmcc_compound_subscript_test");
    drop(std::fs::create_dir_all(&temp_dir));
    let fixture_file = temp_dir.join("test_compound.jc");

    let source = r#"
class Holder {
    var inner: number;

    @getter
    inline function data(self: Holder) -> number {
        return self.inner;
    }

    @setter
    inline function data(self: Holder, v: number) {
        self.inner = v;
    }
}

class CustomBox {
    var v: number;

    @getter
    inline function __subscript__(self: CustomBox, idx: number) -> number {
        return self.v + idx;
    }
}

event<player_join> {
    var arr = [10, 20, 30];
    arr[0] += 5;
    arr[1] *= 3;
    arr[2] -= 10;
    player::message("&aARR0:" + arr[0]);
    player::message("&aARR1:" + arr[1]);
    player::message("&aARR2:" + arr[2]);

    var m = {"val": 100};
    m["val"] += 25;
    player::message("&aMAP:" + m["val"]);

    var h = Holder(50);
    h.data += 15;
    player::message("&aHOLDER:" + h.data);

    var b = CustomBox(40);
    var read_val = b[2];
    player::message("&aBOX:" + read_val);
}
"#;
    std::fs::write(&fixture_file, source).expect("write test fixture");

    let module =
        compile_file(&fixture_file, &test_options(2026, 2)).expect("compile test_compound.jc");
    let program = Program::from_module(module).expect("build program from module");

    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let entries = runtime.world().log().entries();
    let has_arr0 = entries.iter().any(|e| e.contains("ARR0:15"));
    let has_arr1 = entries.iter().any(|e| e.contains("ARR1:60"));
    let has_arr2 = entries.iter().any(|e| e.contains("ARR2:20"));
    let has_map = entries.iter().any(|e| e.contains("MAP:125"));
    let has_holder = entries.iter().any(|e| e.contains("HOLDER:65"));
    let has_box = entries.iter().any(|e| e.contains("BOX:42"));

    assert!(has_arr0, "arr[0] += 5 must be 15, actual logs: {entries:?}");
    assert!(has_arr1, "arr[1] *= 3 must be 60, actual logs: {entries:?}");
    assert!(
        has_arr2,
        "arr[2] -= 10 must be 20, actual logs: {entries:?}"
    );
    assert!(
        has_map,
        "m['val'] += 25 must be 125, actual logs: {entries:?}"
    );
    assert!(
        has_holder,
        "h.data += 15 must be 65, actual logs: {entries:?}"
    );
    assert!(
        has_box,
        "b[2] via @getter __subscript__ method must be 42, actual logs: {entries:?}"
    );

    drop(std::fs::remove_file(fixture_file));
}

#[test]
fn test_runtime_single_field_class_optimization() {
    let temp_dir = std::env::temp_dir().join("jmcc_single_field_opt_test");
    drop(std::fs::create_dir_all(&temp_dir));
    let fixture_file = temp_dir.join("test_single_field.jc");

    let source = r#"
class SingleFieldHolder {
    var val: number;

    inline function get(self: SingleFieldHolder) -> number {
        return self.val;
    }

    inline function calc(self: SingleFieldHolder, factor: number) -> number {
        return self.val * factor;
    }
}

event<player_join> {
    var s = SingleFieldHolder(10);
    player::message("&aINIT:" + s.val);
    s.val = 42;
    player::message("&aSET:" + s.val);
    s.val += 8;
    player::message("&aADD:" + s.val);
    s.val *= 2;
    player::message("&aMUL:" + s.val);
    player::message("&aGET:" + s.get());
    var calculated = s.calc(3);
    player::message("&aCALC:" + calculated);
}
"#;
    std::fs::write(&fixture_file, source).expect("write test fixture");

    let module =
        compile_file(&fixture_file, &test_options(2026, 2)).expect("compile test_single_field.jc");

    // Verify that single-field class does not generate any list operations
    for line in &module.handlers {
        for op in &line.operations {
            assert_ne!(
                op.action,
                ActionId::SetVariableCreateList,
                "single-field class should not use SetVariableCreateList"
            );
            assert_ne!(
                op.action,
                ActionId::SetVariableGetListValue,
                "single-field class should not use SetVariableGetListValue"
            );
            assert_ne!(
                op.action,
                ActionId::SetVariableSetListValue,
                "single-field class should not use SetVariableSetListValue"
            );
        }
    }

    let program = Program::from_module(module).expect("build program from module");
    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let entries = runtime.world().log().entries();
    assert!(
        entries.iter().any(|e| e.contains("INIT:10")),
        "initial value must be 10, got {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("SET:42")),
        "set value must be 42, got {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("ADD:50")),
        "compound add value must be 50, got {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("MUL:100")),
        "compound mul value must be 100, got {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("GET:100")),
        "get method call result must be 100, got {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("CALC:300")),
        "calc method call result must be 300, got {entries:?}"
    );

    drop(std::fs::remove_file(fixture_file));
}

#[test]
fn test_runtime_action_limit_splitting_dynamic_placeholders() {
    let temp_dir = std::env::temp_dir().join(format!(
        "jmcc_split_dyn_rt_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let fixture_file = temp_dir.join("test_split_dyn.jc");

    let mut source = String::from(
        r#"
function run_worker() {
    line var key = "foo";
    `item_%var_line(key)` = 12345;
"#,
    );
    for i in 0..45 {
        source.push_str(&format!("    line var dummy_{i} = {i};\n"));
    }
    source.push_str(
        r#"
    line var retrieved = `item_%var_line(key)`;
    player::message("RESULT:" + retrieved);
}

event<player_join> {
    run_worker();
}
"#,
    );

    std::fs::write(&fixture_file, source).expect("write test fixture");

    let mut options = test_options(2026, 0);
    options.disable_action_limit = false;
    let module = compile_file(&fixture_file, &options).expect("compile test_split_dyn.jc");

    assert!(
        module.handlers.iter().any(|h| {
            if let jmcdata::module::LineValue::Fn { name, .. } = &h.line_value {
                name.starts_with("jmcc.")
            } else {
                false
            }
        }),
        "split handler jmcc.N must be present"
    );

    let program = Program::from_module(module).expect("build program from module");
    let mut runtime = Runtime::new(&program).expect("build runtime");
    runtime.fire_event("player_join").expect("fire player_join");

    let entries = runtime.world().log().entries();
    assert!(
        entries.iter().any(|e| e.contains("RESULT:12345")),
        "retrieved dynamic line variable must be 12345 across split, actual logs: {entries:?}"
    );

    drop(std::fs::remove_dir_all(temp_dir));
}

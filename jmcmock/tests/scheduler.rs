//! Регрессии кооперативного планировщика на JSON, без зависимости от компилятора.

use jmcdata::module::VariableScope;
use jmcmock::{Program, Runtime, value};
use serde_json::{Value, json};

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text, "parsing": "plain"})
}

fn number(number: u64) -> Value {
    json!({"type": "number", "number": number})
}

fn variable(scope: &str, name: &str) -> Value {
    json!({"type": "variable", "scope": scope, "variable": name})
}

fn op(action: &str, args: &[(&str, Value)]) -> Value {
    let values: Vec<_> = args
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    json!({"action": action, "values": values})
}

fn block(action: &str, args: &[(&str, Value)], body: Vec<Value>) -> Value {
    let mut result = op(action, args);
    result["operations"] = json!(body);
    result
}

fn wait(ticks: u64) -> Value {
    op("control_wait", &[("duration", number(ticks))])
}

fn set(scope: &str, name: &str, value: Value) -> Value {
    op(
        "set_variable_value",
        &[("variable", variable(scope, name)), ("value", value)],
    )
}

fn message(message: &str) -> Value {
    op(
        "player_send_message",
        &[(
            "messages",
            json!({"type": "array", "values": [text(message)]}),
        )],
    )
}

fn call(name: &str) -> Value {
    op("call_function", &[("function_name", text(name))])
}

fn event(body: Vec<Value>) -> Value {
    json!({"type": "event", "event": "world_start", "position": 0, "operations": body})
}

fn named(kind: &str, name: &str, body: Vec<Value>) -> Value {
    json!({"type": kind, "name": name, "values": [], "position": 0, "operations": body})
}

fn module(handlers: Vec<Value>) -> String {
    json!({"handlers": handlers}).to_string()
}

fn assert_global(runtime: &Runtime<'_>, name: &str, expected: u64) {
    let actual = runtime
        .global()
        .read(name, VariableScope::Global)
        .expect("global exists");
    assert_eq!(
        value::display(&actual),
        expected.to_string(),
        "global {name}"
    );
}

fn assert_messages(runtime: &Runtime<'_>, expected: &[&str]) {
    let entries: Vec<_> = runtime
        .world()
        .log()
        .entries()
        .iter()
        .filter(|entry| entry.contains("player_send_message"))
        .collect();
    assert_eq!(
        entries.len(),
        expected.len(),
        "log: {:?}",
        runtime.world().log().entries()
    );
    for (entry, expected) in entries.iter().zip(expected) {
        assert!(
            entry.contains(expected),
            "expected {expected:?}, got {entry:?}"
        );
    }
}

#[test]
fn same_event_handlers_yield_independently() {
    let source = module(vec![
        event(vec![message("first-start"), wait(3), message("first-end")]),
        event(vec![
            message("second-start"),
            wait(1),
            message("second-end"),
        ]),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("event completes");
    assert_messages(
        &runtime,
        &["first-start", "second-start", "second-end", "first-end"],
    );
    assert_eq!(runtime.world().tick(), 3);
}

#[test]
fn process_wait_does_not_block_parent() {
    let source = module(vec![
        event(vec![
            op("start_process", &[("process_name", text("worker"))]),
            set("game", "parent_finished", number(1)),
            message("parent-end"),
        ]),
        named(
            "process",
            "worker",
            vec![
                wait(2),
                set(
                    "game",
                    "child_observed_parent",
                    variable("game", "parent_finished"),
                ),
                message("child-end"),
            ],
        ),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("event completes");
    assert_global(&runtime, "child_observed_parent", 1);
    assert_messages(&runtime, &["parent-end", "child-end"]);
    assert_eq!(runtime.world().tick(), 2);
}

#[test]
fn nested_function_wait_preserves_each_line_frame_and_shared_local() {
    let source = module(vec![
        event(vec![
            set("line", "slot", number(10)),
            set("local", "shared", number(1)),
            call("outer"),
            set("game", "caller_line", variable("line", "slot")),
            set("game", "caller_local", variable("local", "shared")),
        ]),
        event(vec![
            set("line", "slot", number(90)),
            set("local", "shared", number(99)),
            wait(1),
            set("game", "other_local", variable("local", "shared")),
        ]),
        named(
            "function",
            "outer",
            vec![
                set("line", "slot", number(20)),
                call("inner"),
                set("game", "outer_line", variable("line", "slot")),
            ],
        ),
        named(
            "function",
            "inner",
            vec![
                set("line", "slot", number(30)),
                wait(2),
                set("game", "inner_line", variable("line", "slot")),
                set("game", "inner_local", variable("local", "shared")),
                set("local", "shared", number(2)),
            ],
        ),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("event completes");
    for (name, expected) in [
        ("caller_line", 10),
        ("outer_line", 20),
        ("inner_line", 30),
        ("inner_local", 1),
        ("caller_local", 2),
        ("other_local", 99),
    ] {
        assert_global(&runtime, name, expected);
    }
    assert_eq!(runtime.world().tick(), 2);
}

#[test]
fn repeat_resumes_after_wait_without_restarting_body() {
    let source = module(vec![
        event(vec![
            block(
                "repeat_on_range",
                &[
                    ("variable", variable("line", "index")),
                    ("start", number(1)),
                    ("end", number(3)),
                ],
                vec![
                    message("iteration-before"),
                    wait(2),
                    message("iteration-after"),
                    set("game", "last_index", variable("line", "index")),
                ],
            ),
            message("loop-end"),
        ]),
        event(vec![wait(1), message("peer-ran")]),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("event completes");
    assert_messages(
        &runtime,
        &[
            "iteration-before",
            "peer-ran",
            "iteration-after",
            "iteration-before",
            "iteration-after",
            "iteration-before",
            "iteration-after",
            "loop-end",
        ],
    );
    assert_global(&runtime, "last_index", 3);
    assert_eq!(runtime.world().tick(), 6);
}

#[test]
fn measure_time_spans_wait_but_not_another_streams_later_wakeup() {
    let source = module(vec![
        event(vec![block(
            "controller_measure_time",
            &[("variable", variable("game", "elapsed"))],
            vec![wait(3), message("measurement-end")],
        )]),
        event(vec![wait(10), message("later-peer")]),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("event completes");
    assert_global(&runtime, "elapsed", 150);
    assert_messages(&runtime, &["measurement-end", "later-peer"]);
    assert_eq!(runtime.world().tick(), 10);
}

#[test]
fn exception_catcher_survives_a_nested_function_wait() {
    let source = module(vec![
        event(vec![
            block(
                "controller_exception",
                &[("variable", variable("game", "caught"))],
                vec![call("failing"), message("unreachable-catcher-tail")],
            ),
            message("caught-and-continued"),
        ]),
        event(vec![wait(1), message("peer-before-error")]),
        named(
            "function",
            "failing",
            vec![
                wait(2),
                op(
                    "control_call_exception",
                    &[("message", text("failure-after-wait"))],
                ),
                message("unreachable-function-tail"),
            ],
        ),
    ]);
    let program = Program::parse(&source).expect("valid module");
    let mut runtime = Runtime::new(&program).expect("runtime");
    runtime.fire_event("world_start").expect("exception caught");
    let caught = runtime
        .global()
        .read("caught", VariableScope::Global)
        .expect("caught variable")
        .expect("exception text");
    assert!(value::display(&Some(caught)).contains("failure-after-wait"));
    assert_messages(&runtime, &["peer-before-error", "caught-and-continued"]);
    assert_eq!(runtime.world().tick(), 2);
}

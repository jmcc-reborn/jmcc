//! Integration tests for the JMCC code formatter.

use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn check_idempotent(source: &str, edition: u16, path_str: &str) -> String {
    let ast1 = jmcc::ast::parser::parse_string(source, path_str, edition, 0)
        .unwrap_or_else(|e| panic!("Failed to parse initial source of {path_str}: {e:?}"));
    let formatted1 = jmcc::ast::format::format(&ast1, source);

    let ast2 = jmcc::ast::parser::parse_string(&formatted1, path_str, edition, 0)
        .unwrap_or_else(|e| {
            let err_str = format!("{e:?}");
            let snippet = if let Some(idx) = err_str.find("..").and_then(|pos| {
                let start_num = err_str[..pos].rsplit(|c: char| !c.is_ascii_digit()).next()?;
                start_num.parse::<usize>().ok()
            }) {
                let start = idx.saturating_sub(100);
                let end = (idx + 100).min(formatted1.len());
                &formatted1[start..end]
            } else {
                "unable to extract range"
            };
            panic!("Failed to parse formatted source (round 1) of {path_str}: {e:?}\nSnippet around error:\n>>>\n{snippet}\n<<<\n");
        });
    let formatted2 = jmcc::ast::format::format(&ast2, &formatted1);

    if formatted1 != formatted2 {
        let lines1: Vec<&str> = formatted1.lines().collect();
        let lines2: Vec<&str> = formatted2.lines().collect();
        let mut diff_line = 0;
        for (i, (l1, l2)) in lines1.iter().zip(lines2.iter()).enumerate() {
            if l1 != l2 {
                diff_line = i;
                break;
            }
        }
        if diff_line == 0 && lines1.len() != lines2.len() {
            diff_line = lines1.len().min(lines2.len());
        }
        let start1 = diff_line.saturating_sub(5);
        let end1 = (diff_line + 5).min(lines1.len());
        let context1 = lines1[start1..end1].join("\n");

        let start2 = diff_line.saturating_sub(5);
        let end2 = (diff_line + 5).min(lines2.len());
        let context2 = lines2[start2..end2].join("\n");

        panic!(
            "Formatter is not idempotent for {path_str} at line {}!\n--- Formatted 1 (around diff) ---\n{context1}\n--- Formatted 2 (around diff) ---\n{context2}",
            diff_line + 1
        );
    }

    formatted1
}

#[test]
fn test_format_loops_and_labels() {
    let src = r#"function test_loops() {
    var i = 0;
    while i < 10 {
        i += 1;
        if i == 5 {
            break;
        }
    }
    player::message(i);

    while not (i == 0) {
        i -= 1;
    }
    player::message(i);

    var list = [1, 2, 3];
    var sum = 0;
    for item in list {
        sum += item;
    }
    player::message(sum);

    var m = {"a": 10, "b": 20};
    var map_sum = 0;
    for k, v in m {
        map_sum += v;
    }
    player::message(map_sum);

    var reached_after = 0;
    'outer: while true {
        'inner: for item in list {
            if item == 2 {
                break 'outer;
            }
        }
        reached_after = 1;
    }
    player::message(reached_after);
}
"#;
    let formatted = check_idempotent(src, 2026, "loops");
    assert!(
        formatted.contains("for item in list"),
        "Expected 'for item in list', got: {formatted}"
    );
    assert!(
        formatted.contains("for k, v in m"),
        "Expected 'for k, v in m', got: {formatted}"
    );
    assert!(
        formatted.contains("break 'outer;"),
        "Expected 'break 'outer;', got: {formatted}"
    );
    assert!(
        formatted.contains("'outer: while true"),
        "Expected ''outer: while true', got: {formatted}"
    );
    assert!(
        formatted.contains("'inner: for item in list"),
        "Expected ''inner: for item in list', got: {formatted}"
    );
}

#[test]
fn test_format_decorators_and_generics() {
    let src = r#"@lang_item
@dict
class map<K, V> {
    @getter
    function get_first(self: map<K, V>) -> V {
        return self.get(0);
    }

    @setter
    function set_first(self: map<K, V>, val: V) {
        self.put(0, val);
    }
}
"#;
    let formatted = check_idempotent(src, 2026, "decorators");
    assert!(
        formatted.contains("@lang_item"),
        "Missing @lang_item: {formatted}"
    );
    assert!(formatted.contains("@dict"), "Missing @dict: {formatted}");
    assert!(
        formatted.contains("class map<K, V>"),
        "Missing generics: {formatted}"
    );
    assert!(
        formatted.contains("@getter"),
        "Missing @getter: {formatted}"
    );
    assert!(
        formatted.contains("@setter"),
        "Missing @setter: {formatted}"
    );
}

#[test]
fn test_format_interface() {
    let src = r#"interface Greeter {
    function greet(self: Greeter, name: text) -> text;
}

interface DetailedGreeter extends Greeter {
    function greet_detailed(self: DetailedGreeter, name: text, title: text) -> text;
}
"#;
    let formatted = check_idempotent(src, 2026, "interface");
    assert!(
        formatted.contains("function greet(self: Greeter, name: text) -> text;"),
        "Interface method must end with semicolon: {formatted}"
    );
}

#[test]
fn test_format_lambdas() {
    let src = r#"function test() {
    var f1 = (x, y) => x + y;
    var f2 = (x: number) -> number => x * 2;
    var f3 = (x) => {
        var y = x * 2;
        return y;
    };
}
"#;
    let formatted = check_idempotent(src, 2026, "lambdas");
    assert!(
        formatted.contains("(x, y) => x + y"),
        "Failed lambda 1: {formatted}"
    );
    assert!(
        formatted.contains("(x: number) -> number => x * 2"),
        "Failed lambda 2: {formatted}"
    );
}

#[test]
fn test_format_preserves_comments_in_empty_blocks() {
    // `@lang_item` constructors keep their implementation note in the body. The
    // formatter must not collapse such a block to `{}`, which would delete the
    // comment and leave the constructor semantically empty.
    let src = r#"@lang_item
export class block {
    var id: text;

    inline function __init__(id: text) -> block {
        /* compiler built-in */
    }

    inline function truly_empty() {}
}
"#;
    let formatted = check_idempotent(src, 2026, "empty_block_comments");
    assert!(
        formatted.contains("/* compiler built-in */"),
        "Comment inside an empty block was dropped: {formatted}"
    );
    assert!(
        !formatted.contains("-> block {}"),
        "Comment-bearing constructor body collapsed to an empty block: {formatted}"
    );
    assert!(
        formatted.contains("function truly_empty() {}"),
        "A genuinely empty block should stay on one line: {formatted}"
    );
}

#[test]
fn test_format_all_fixtures() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut tested = 0;
    for entry in WalkDir::new(&tests_dir).follow_links(true) {
        let entry = entry.expect("failed to read entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jc") {
            let path_lossy = path.to_string_lossy();
            if path_lossy.contains("/template/") {
                continue;
            }
            let src = fs::read_to_string(path).expect("failed to read .jc file");
            let edition = if path_lossy.contains("2023")
                || path_lossy.contains("/nwo/")
                || path_lossy.contains("/pvp/")
                || path_lossy.ends_with("cubed.jc")
                || path_lossy.ends_with("compass.jc")
            {
                2023
            } else {
                2026
            };
            let path_str = path.display().to_string();
            let _ = check_idempotent(&src, edition, &path_str);
            tested += 1;
        }
    }
    assert!(
        tested > 10,
        "Expected to test at least 10 fixtures, tested: {tested}"
    );
}

#[test]
fn test_format_matrix() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("std/math/matrix.jc");
    let src = fs::read_to_string(&path).expect("failed to read matrix.jc");
    let ast1 = jmcc::ast::parser::parse_string(&src, "matrix.jc", 2026, 0)
        .expect("Failed to parse initial source");
    let formatted = jmcc::ast::format::format(&ast1, &src);
    match jmcc::ast::parser::parse_string(&formatted, "matrix.jc", 2026, 0) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Parse error: {:?}", e);
            let lines: Vec<&str> = formatted.lines().collect();
            for (idx, line) in lines.iter().enumerate() {
                eprintln!("{:4}: {}", idx + 1, line);
            }
            panic!("Failed to parse formatted matrix: {:?}", e);
        }
    }
}

#[test]
fn test_format_all_std() {
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("std");
    let mut tested = 0;
    for entry in WalkDir::new(&std_dir).follow_links(true) {
        let entry = entry.expect("failed to read entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jc") {
            let src = fs::read_to_string(path).expect("failed to read std file");
            let edition = if path.to_string_lossy().contains("2023") {
                2023
            } else {
                2026
            };
            let path_str = path.display().to_string();
            let _ = check_idempotent(&src, edition, &path_str);
            tested += 1;
        }
    }
    assert!(
        tested > 10,
        "Expected to test at least 10 std files, tested: {tested}"
    );
}

#[test]
fn test_format_all_examples() {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent dir")
        .join("examples");
    let mut tested = 0;
    for entry in WalkDir::new(&examples_dir).follow_links(true) {
        let entry = entry.expect("failed to read entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jc") {
            let src = fs::read_to_string(path).expect("failed to read example file");
            let path_str = path.display().to_string();
            let _ = check_idempotent(&src, 2026, &path_str);
            tested += 1;
        }
    }
    assert!(
        tested >= 5,
        "Expected to test at least 5 examples, tested: {tested}"
    );
}

#[test]
fn test_format_match_preserves_comments_and_style() {
    let src = r#"function test_match() {
    var x = 1;
    match x {
        // Comment before first arm
        1 => {
            player::message("one");
        }
        // Comment between arms
        2 => player::message("two"),
        // Comment before default
        _ => {
            player::message("other");
        }
        // Comment before closing brace
    }
}
"#;
    let formatted = check_idempotent(src, 2026, "match_comments");
    assert!(formatted.contains("// Comment before first arm"));
    assert!(formatted.contains("// Comment between arms"));
    assert!(formatted.contains("// Comment before default"));
    assert!(formatted.contains("// Comment before closing brace"));
    assert!(
        !formatted.contains("},\n        //"),
        "Should not have trailing comma after block arm"
    );
}

use jmcc::CompileOptions;
use jmcc::ast::parser::parse_string;
use jmcc::compile_file;
use jmcc::diagnostic::{Diagnostic, DiagnosticLabel, DiagnosticRenderer};
use std::fs;

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
fn test_pretty_diagnostic_render_direct() {
    let source = "function main() {\n    player::send_msg(\"Hello\");\n}\n";
    let diag = Diagnostic::error("unknown action 'player::send_msg'")
        .with_code("E0002")
        .with_file("src/main.jc".into())
        .with_label(DiagnosticLabel::primary(
            22..38,
            Some("unknown action".into()),
        ))
        .with_help("did you mean 'send_message'?");

    let renderer = DiagnosticRenderer::new().with_color(false);
    let output = renderer.render_diagnostic(&diag, source, "src/main.jc");

    assert!(output.contains("error[E0002]: unknown action 'player::send_msg'"));
    assert!(output.contains("src/main.jc"));
    assert!(output.contains("player::send_msg(\"Hello\");"));
    assert!(output.contains("^^^^^^^^^^^^^^^^"));
    assert!(output.contains("help: did you mean 'send_message'?"));
}

#[test]
fn test_pretty_parser_syntax_error() {
    let bad_source = "function test( { }\n";
    let err = parse_string(bad_source, "bad_syntax.jc", 2026, 0).unwrap_err();
    let err_str = err.to_string();

    assert!(err_str.contains("error[S0002]") || err_str.contains("ошибка[S0002]"));
    assert!(err_str.contains("bad_syntax.jc"));
    assert!(err_str.contains("function test( { }"));
    assert!(err_str.contains('^'));
}

#[test]
fn test_pretty_semantic_error_unknown_action() {
    let temp_dir = std::env::temp_dir().join("jmcc_pretty_test_action");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_action.jc");
    fs::write(
        &file_path,
        "function main() {\n    player::send_msg(\"Hi\");\n}\n",
    )
    .expect("write test file");

    let opts = test_options(2026, 2);
    let err = compile_file(&file_path, &opts).unwrap_err();
    let rendered = err.to_string();

    assert!(rendered.contains("error[E0002]"));
    assert!(rendered.contains("test_action.jc"));
    assert!(rendered.contains("player::send_msg"));
    assert!(rendered.contains("^^^^^^^^^^^^^^^^"));
    assert!(rendered.contains("aborting due to") || rendered.contains("прервано из-за"));

    drop(fs::remove_file(file_path));
}

#[test]
fn test_pretty_semantic_error_duplicate_declaration() {
    let temp_dir = std::env::temp_dir().join("jmcc_pretty_test_dup");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_dup.jc");
    fs::write(&file_path, "function foo() {}\n\nfunction foo() {}\n").expect("write test file");

    let opts = test_options(2026, 2);
    let err = compile_file(&file_path, &opts).unwrap_err();
    let rendered = err.to_string();

    assert!(rendered.contains("error[E0009]"));
    assert!(rendered.contains("test_dup.jc"));
    assert!(rendered.contains("function foo() {}"));
    assert!(rendered.contains("aborting due to") || rendered.contains("прервано из-за"));

    drop(fs::remove_file(file_path));
}

#[test]
fn test_pretty_multiple_errors_rendering() {
    let temp_dir = std::env::temp_dir().join("jmcc_pretty_test_multi");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_multi.jc");
    fs::write(
        &file_path,
        "function test() {\n    break;\n    return 123;\n}\n",
    )
    .expect("write test file");

    let opts = test_options(2026, 2);
    let err = compile_file(&file_path, &opts).unwrap_err();
    let rendered = err.to_string();

    assert!(rendered.contains("error[E0011]"));
    assert!(rendered.contains("test_multi.jc"));
    assert!(rendered.contains("break;"));
    assert!(rendered.contains("aborting due to") || rendered.contains("прервано из-за"));

    drop(fs::remove_file(file_path));
}

#[test]
fn test_pretty_diagnostic_render_russian() {
    let source = "function main() {\n    player::send_msg(\"Hello\");\n}\n";
    let diag = Diagnostic::error("неизвестное действие 'player::send_msg'")
        .with_code("E0002")
        .with_file("src/main.jc".into())
        .with_label(DiagnosticLabel::primary(
            22..38,
            Some("неизвестное действие".into()),
        ))
        .with_help("возможно, вы имели в виду 'send_message'?")
        .with_note("действие JustMC должно быть объявлено");

    let renderer = DiagnosticRenderer::new()
        .with_color(false)
        .with_lang(jmcc::i18n::Lang::Ru);
    let output = renderer.render_diagnostic(&diag, source, "src/main.jc");

    assert!(output.contains("ошибка[E0002]: неизвестное действие 'player::send_msg'"));
    assert!(output.contains("src/main.jc"));
    assert!(output.contains("player::send_msg(\"Hello\");"));
    assert!(output.contains("^^^^^^^^^^^^^^^^"));
    assert!(output.contains("помощь: возможно, вы имели в виду 'send_message'?"));
    assert!(output.contains("примечание: действие JustMC должно быть объявлено"));
}

#[test]
fn test_pretty_diagnostic_render_english() {
    let source = "function main() {\n    player::send_msg(\"Hello\");\n}\n";
    let diag = Diagnostic::error("unknown action 'player::send_msg'")
        .with_code("E0002")
        .with_file("src/main.jc".into())
        .with_label(DiagnosticLabel::primary(
            22..38,
            Some("unknown action".into()),
        ))
        .with_help("did you mean 'send_message'?")
        .with_note("action must be declared");

    let renderer = DiagnosticRenderer::new()
        .with_color(false)
        .with_lang(jmcc::i18n::Lang::En);
    let output = renderer.render_diagnostic(&diag, source, "src/main.jc");

    assert!(output.contains("error[E0002]: unknown action 'player::send_msg'"));
    assert!(output.contains("src/main.jc"));
    assert!(output.contains("player::send_msg(\"Hello\");"));
    assert!(output.contains("^^^^^^^^^^^^^^^^"));
    assert!(output.contains("help: did you mean 'send_message'?"));
    assert!(output.contains("note: action must be declared"));
}

#[test]
fn test_pretty_diagnostic_render_warning_both_languages() {
    let source = "function main() {\n    var x = 1;\n}\n";
    let diag_ru = Diagnostic::warning("неиспользуемая переменная 'x'")
        .with_file("src/main.jc".into())
        .with_label(DiagnosticLabel::primary(
            26..27,
            Some("не используется".into()),
        ));
    let renderer_ru = DiagnosticRenderer::new()
        .with_color(false)
        .with_lang(jmcc::i18n::Lang::Ru);
    let out_ru = renderer_ru.render_diagnostic(&diag_ru, source, "src/main.jc");
    assert!(out_ru.contains("предупреждение: неиспользуемая переменная 'x'"));

    let diag_en = Diagnostic::warning("unused variable 'x'")
        .with_file("src/main.jc".into())
        .with_label(DiagnosticLabel::primary(26..27, Some("unused".into())));
    let renderer_en = DiagnosticRenderer::new()
        .with_color(false)
        .with_lang(jmcc::i18n::Lang::En);
    let out_en = renderer_en.render_diagnostic(&diag_en, source, "src/main.jc");
    assert!(out_en.contains("warning: unused variable 'x'"));
}

#[test]
fn test_compile_file_with_russian_locale_flag() {
    let temp_dir = std::env::temp_dir().join("jmcc_pretty_test_ru_flag");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_ru.jc");
    fs::write(
        &file_path,
        "function main() {\n    player::play_sond(\"Hi\");\n}\n",
    )
    .expect("write test file");

    let mut opts = test_options(2026, 2);
    opts.locale = Some("ru".into());
    let err = compile_file(&file_path, &opts).unwrap_err();
    let rendered = err.to_string();

    assert!(
        rendered.contains("ошибка[E0002]"),
        "expected 'ошибка[E0002]', got:\n{rendered}"
    );
    assert!(
        rendered.contains("помощь") && rendered.contains("play_sound"),
        "expected 'помощь' with 'play_sound', got:\n{rendered}"
    );
    assert!(
        rendered.contains("прервано из-за"),
        "expected summary in Russian, got:\n{rendered}"
    );

    drop(fs::remove_file(file_path));
}

#[test]
fn test_compile_file_with_english_locale_flag() {
    let temp_dir = std::env::temp_dir().join("jmcc_pretty_test_en_flag");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_en.jc");
    fs::write(
        &file_path,
        "function main() {\n    player::play_sond(\"Hi\");\n}\n",
    )
    .expect("write test file");

    let mut opts = test_options(2026, 2);
    opts.locale = Some("en".into());
    let err = compile_file(&file_path, &opts).unwrap_err();
    let rendered = err.to_string();

    assert!(
        rendered.contains("error[E0002]"),
        "expected 'error[E0002]', got:\n{rendered}"
    );
    assert!(
        rendered.contains("help") && rendered.contains("play_sound"),
        "expected 'help' with 'play_sound', got:\n{rendered}"
    );
    assert!(
        rendered.contains("aborting due to"),
        "expected summary in English, got:\n{rendered}"
    );

    drop(fs::remove_file(file_path));
}

#[test]
fn test_manifest_locale_configuration() {
    use jmcc::project::Manifest;
    use std::path::Path;

    let toml_str = r#"
        [package]
        name = "localized_app"
        version = "0.1.0"
        locale = "ru"

        [profile.release]
        locale = "en"
    "#;
    let manifest = Manifest::from_str(toml_str, Path::new("jmcc.toml")).unwrap();
    assert_eq!(manifest.locale(), Some("ru".to_owned()));

    let rel_profile = &manifest.profile["release"];
    assert_eq!(rel_profile.locale, Some("en".to_owned()));
}

#[test]
fn test_pretty_diagnostic_ice_compiler_error_code() {
    let err = jmcc::error::JmccError::InternalCompilerError("assertion failed in pass".to_owned());
    let pretty_err = err.with_source_context(
        "function foo() {}",
        std::path::Path::new("test_ice.jc"),
        0..8,
        jmcc::i18n::Lang::En,
    );
    let rendered = pretty_err.to_string();
    assert!(
        rendered.contains("error[E0001]"),
        "expected 'error[E0001]', got:\n{rendered}"
    );
    assert!(rendered.contains("internal compiler error (ICE): assertion failed in pass"));
}

#[test]
fn test_pretty_diagnostic_ice_parser_error_code() {
    let err = jmcc::error::JmccError::InternalParserError("unexpected lexer state".to_owned());
    let pretty_err = err.with_source_context(
        "function foo() {}",
        std::path::Path::new("test_parser_ice.jc"),
        0..8,
        jmcc::i18n::Lang::En,
    );
    let rendered = pretty_err.to_string();
    assert!(
        rendered.contains("error[S0001]"),
        "expected 'error[S0001]', got:\n{rendered}"
    );
    assert!(rendered.contains("internal parser error (ICE): unexpected lexer state"));
}

#[test]
fn test_subscript_missing_getter_hint() {
    let temp_dir = std::env::temp_dir().join("jmcc_test_subscript_missing_getter");
    drop(fs::create_dir_all(&temp_dir));
    let file_path = temp_dir.join("test_no_getter.jc");
    fs::write(
        &file_path,
        r#"
class Box {
    var v: number;
    function __subscript__(self: Box, idx: number) -> number {
        return self.v + idx;
    }
}
function main() {
    var b = Box(10);
    var x = b[0];
}
"#,
    )
    .unwrap();

    let err = compile_file(&file_path, &test_options(2026, 2)).unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("E0039"),
        "expected error E0039, got: {err_str}"
    );
    assert!(
        err_str.contains("__subscript__") && err_str.contains("@getter"),
        "expected hint mentioning '@getter', got: {err_str}"
    );

    drop(fs::remove_file(file_path));
}

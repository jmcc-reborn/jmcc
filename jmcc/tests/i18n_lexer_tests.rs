//! Integration tests for i18n, alternate (Russian) lexer, and alias decorators.

use jmcc::{
    CompileOptions, compile_file,
    i18n::{Lang, plural_errors, plural_params},
};
use std::path::{Path, PathBuf};

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

#[test]
fn test_compile_russian_code() {
    let path = fixture_path("russian_code.jc");
    let module = compile_file(&path, &test_options(2026, 2))
        .expect("Failed to compile russian_code.jc with alternate lexer");
    assert!(
        !module.handlers.is_empty(),
        "russian_code should generate handlers"
    );
}

#[test]
fn test_compile_modern_alias() {
    let path = fixture_path("modern_alias.jc");
    let module =
        compile_file(&path, &test_options(2026, 2)).expect("Failed to compile modern_alias.jc");
    assert!(
        !module.handlers.is_empty(),
        "modern_alias should generate handlers"
    );
}

#[test]
fn test_icu_plurals_russian_and_english() {
    assert_eq!(plural_errors(1, Lang::Ru), "1 семантическая ошибка");
    assert_eq!(plural_errors(2, Lang::Ru), "2 семантические ошибки");
    assert_eq!(plural_errors(5, Lang::Ru), "5 семантических ошибок");
    assert_eq!(plural_errors(11, Lang::Ru), "11 семантических ошибок");
    assert_eq!(plural_errors(21, Lang::Ru), "21 семантическая ошибка");

    assert_eq!(plural_errors(1, Lang::En), "1 semantic error");
    assert_eq!(plural_errors(2, Lang::En), "2 semantic errors");
    assert_eq!(plural_errors(5, Lang::En), "5 semantic errors");

    assert_eq!(plural_params(1, Lang::Ru), "1 параметр");
    assert_eq!(plural_params(2, Lang::Ru), "2 параметра");
    assert_eq!(plural_params(5, Lang::Ru), "5 параметров");
    assert_eq!(plural_params(21, Lang::Ru), "21 параметр");

    assert_eq!(plural_params(1, Lang::En), "1 parameter");
    assert_eq!(plural_params(2, Lang::En), "2 parameters");
}

#[test]
fn test_localized_diagnostic_output() {
    let temp_dir = std::env::temp_dir().join("jmcc_test_i18n_err");
    drop(std::fs::create_dir_all(&temp_dir));
    let bad_file = temp_dir.join("bad.jc");
    std::fs::write(&bad_file, "function test() { var x = unknown_func_xyz(); }")
        .expect("write bad file");

    let mut opts_ru = test_options(2026, 2);
    opts_ru.locale = Some("ru".into());

    let err_ru = compile_file(&bad_file, &opts_ru).unwrap_err();
    let err_ru_str = err_ru.to_string();
    assert!(
        err_ru_str.contains("семантическ") && err_ru_str.contains("необъявленной переменной"),
        "Russian error should be localized, got: {err_ru_str}"
    );

    let mut opts_en = test_options(2026, 2);
    opts_en.locale = Some("en".into());

    let err_en = compile_file(&bad_file, &opts_en).unwrap_err();
    let err_en_str = err_en.to_string();
    assert!(
        err_en_str.contains("semantic error") && err_en_str.contains("undeclared variable"),
        "English error should be localized, got: {err_en_str}"
    );

    drop(std::fs::remove_file(bad_file));
}

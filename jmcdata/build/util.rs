//! Общие хелперы генератора: запись результата и мелкие преобразования наборов
//! значений.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Пишет `contents` в `OUT_DIR/filename` и возвращает путь к файлу.
pub fn write_to_out_dir(filename: &str, contents: &str) -> std::io::Result<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"));
    let path = out_dir.join(filename);
    fs::write(&path, contents)?;
    Ok(path)
}

/// Оставляет первое вхождение каждого значения, сохраняя порядок входа.
pub fn unique(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

/// Булевы аргументы схема пишет как `FALSE`/`TRUE`, поэтому они строятся из
/// `bool`, а не из сгенерированного перечисления.
pub fn is_boolean_values(values: &[String]) -> bool {
    matches!(values, [f, t] if f == "FALSE" && t == "TRUE")
}

/// Отсортированная копия набора значений.
///
/// Этот порядок — и порядок вариантов в перечислении, и (через запятую) ключ,
/// под которым наборы значений делятся между аргументами.
pub fn sorted_values(values: &[String]) -> Vec<String> {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted
}

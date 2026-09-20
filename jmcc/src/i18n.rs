//! Internationalization (i18n) and localization support using ICU4X.

use icu::locale::locale;
use icu::plurals::{PluralCategory, PluralRules};
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Lang {
    #[default]
    En = 0,
    Ru = 1,
}

thread_local! {
    // 0 = unset (fallback to detect_preferred_lang), 1 = Ru, 2 = En
    static CURRENT_LANG: Cell<u8> = const { Cell::new(0) };
}

/// Sets the current locale language.
pub fn set_current_lang(lang: Lang) {
    let val = match lang {
        Lang::Ru => 1,
        Lang::En => 2,
    };
    CURRENT_LANG.set(val);
}

/// Sets the locale by string name (e.g. "ru", "en", "ru-RU", "en-US").
pub fn set_lang_by_name(name: &str) {
    let lower = name.to_lowercase();
    if lower.starts_with("ru") {
        set_current_lang(Lang::Ru);
    } else {
        set_current_lang(Lang::En);
    }
}

/// Returns the currently active language.
#[must_use]
pub fn current_lang() -> Lang {
    match CURRENT_LANG.get() {
        1 => Lang::Ru,
        2 => Lang::En,
        _ => {
            let detected = detect_preferred_lang();
            set_current_lang(detected);
            detected
        }
    }
}

/// Automatically detects preferred language from thread-local state, CLI arguments, or environment variables.
#[must_use]
pub fn detect_preferred_lang() -> Lang {
    // Inspect command-line arguments for --locale or --lang
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        if (arg == "--locale" || arg == "--lang")
            && let Some(next) = args.get(i + 1)
            && next.to_lowercase().starts_with("ru")
        {
            return Lang::Ru;
        } else if let Some(val) = arg
            .strip_prefix("--locale=")
            .or_else(|| arg.strip_prefix("--lang="))
            && val.to_lowercase().starts_with("ru")
        {
            return Lang::Ru;
        }
    }

    // Inspect environment variables
    for var in ["JMCC_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var)
            && val.to_lowercase().starts_with("ru")
        {
            return Lang::Ru;
        }
    }

    Lang::En
}

/// Formats the plural count of errors using ICU4X `PluralRules`.
///
/// # Panics
///
/// Panics if ICU4X data cannot instantiate `PluralRules` for the requested locale.
#[must_use]
pub fn plural_errors(count: usize, lang: Lang) -> String {
    match lang {
        Lang::Ru => {
            let pr = PluralRules::try_new_cardinal(locale!("ru").into())
                .expect("Failed to create PluralRules for 'ru'");
            match pr.category_for(count) {
                PluralCategory::One => format!("{count} семантическая ошибка"),
                PluralCategory::Few => format!("{count} семантические ошибки"),
                PluralCategory::Many | PluralCategory::Other => {
                    format!("{count} семантических ошибок")
                }
                _ => format!("{count} семантических ошибок"),
            }
        }
        Lang::En => {
            let pr = PluralRules::try_new_cardinal(locale!("en").into())
                .expect("Failed to create PluralRules for 'en'");
            match pr.category_for(count) {
                PluralCategory::One => format!("{count} semantic error"),
                _ => format!("{count} semantic errors"),
            }
        }
    }
}

/// Formats the plural count of parameters using ICU4X `PluralRules`.
///
/// # Panics
///
/// Panics if ICU4X data cannot instantiate `PluralRules` for the requested locale.
#[must_use]
pub fn plural_params(count: usize, lang: Lang) -> String {
    match lang {
        Lang::Ru => {
            let pr = PluralRules::try_new_cardinal(locale!("ru").into())
                .expect("Failed to create PluralRules for 'ru'");
            match pr.category_for(count) {
                PluralCategory::One => format!("{count} параметр"),
                PluralCategory::Few => format!("{count} параметра"),
                _ => format!("{count} параметров"),
            }
        }
        Lang::En => {
            let pr = PluralRules::try_new_cardinal(locale!("en").into())
                .expect("Failed to create PluralRules for 'en'");
            match pr.category_for(count) {
                PluralCategory::One => format!("{count} parameter"),
                _ => format!("{count} parameters"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plural_errors_ru() {
        assert_eq!(plural_errors(1, Lang::Ru), "1 семантическая ошибка");
        assert_eq!(plural_errors(2, Lang::Ru), "2 семантические ошибки");
        assert_eq!(plural_errors(4, Lang::Ru), "4 семантические ошибки");
        assert_eq!(plural_errors(5, Lang::Ru), "5 семантических ошибок");
        assert_eq!(plural_errors(11, Lang::Ru), "11 семантических ошибок");
        assert_eq!(plural_errors(21, Lang::Ru), "21 семантическая ошибка");
        assert_eq!(plural_errors(22, Lang::Ru), "22 семантические ошибки");
        assert_eq!(plural_errors(25, Lang::Ru), "25 семантических ошибок");
    }

    #[test]
    fn test_plural_errors_en() {
        assert_eq!(plural_errors(1, Lang::En), "1 semantic error");
        assert_eq!(plural_errors(0, Lang::En), "0 semantic errors");
        assert_eq!(plural_errors(2, Lang::En), "2 semantic errors");
        assert_eq!(plural_errors(5, Lang::En), "5 semantic errors");
    }

    #[test]
    fn test_plural_params_ru() {
        assert_eq!(plural_params(1, Lang::Ru), "1 параметр");
        assert_eq!(plural_params(2, Lang::Ru), "2 параметра");
        assert_eq!(plural_params(4, Lang::Ru), "4 параметра");
        assert_eq!(plural_params(5, Lang::Ru), "5 параметров");
        assert_eq!(plural_params(11, Lang::Ru), "11 параметров");
        assert_eq!(plural_params(21, Lang::Ru), "21 параметр");
        assert_eq!(plural_params(22, Lang::Ru), "22 параметра");
        assert_eq!(plural_params(25, Lang::Ru), "25 параметров");
    }

    #[test]
    fn test_plural_params_en() {
        assert_eq!(plural_params(1, Lang::En), "1 parameter");
        assert_eq!(plural_params(0, Lang::En), "0 parameters");
        assert_eq!(plural_params(2, Lang::En), "2 parameters");
        assert_eq!(plural_params(5, Lang::En), "5 parameters");
    }

    #[test]
    fn test_lang_switching() {
        set_current_lang(Lang::Ru);
        assert_eq!(current_lang(), Lang::Ru);

        set_lang_by_name("en");
        assert_eq!(current_lang(), Lang::En);

        set_lang_by_name("ru-RU");
        assert_eq!(current_lang(), Lang::Ru);

        set_lang_by_name("en-US");
        assert_eq!(current_lang(), Lang::En);
    }
}

//! Alternate (Russian) keyword matching for `.jc`.

use super::Token;

/// Matches an identifier against Russian (and fallback English) reserved keywords.
#[must_use]
pub fn keyword(s: &str) -> Option<Token<'static>> {
    match s {
        "импорт" | "import" => Some(Token::Import),
        "переменная" | "перем" | "пусть" | "var" => Some(Token::Var),
        "функция" | "function" => Some(Token::Function),
        "действие" | "fun" => Some(Token::Fun),
        "определение" | "def" => Some(Token::Def),
        "процесс" | "process" => Some(Token::Process),
        "событие" | "event" => Some(Token::Event),
        "класс" | "class" => Some(Token::Class),
        "перечисление" | "enum" => Some(Token::Enum),
        "встраиваемый" | "inline" => Some(Token::Inline),
        "локальный" | "local" => Some(Token::Local),
        "игра" | "game" => Some(Token::Game),
        "сохранить" | "сохранение" | "save" => Some(Token::Save),
        "строка" | "line" => Some(Token::Line),
        "jmcc" => Some(Token::Jmcc),
        "прервать" | "break" => Some(Token::Break),
        "продолжить" | "continue" => Some(Token::Continue),
        "ссылка" | "ref" => Some(Token::Ref),
        "если" | "if" => Some(Token::If),
        "иначе" | "else" => Some(Token::Else),
        "иначе_если" | "иначеесли" | "elif" => Some(Token::Elif),
        "не" | "not" => Some(Token::Not),
        "и" | "and" => Some(Token::And),
        "или" | "or" => Some(Token::Or),
        "в" | "in" => Some(Token::In),
        "вернуть" | "возврат" | "return" => Some(Token::Return),
        "истина" | "правда" | "true" => Some(Token::True),
        "ложь" | "false" => Some(Token::False),
        "простой" | "plain" => Some(Token::Plain),
        "устаревший" | "legacy" => Some(Token::Legacy),
        "минисообщение" | "minimessage" => Some(Token::Minimessage),
        "джсон" | "json" => Some(Token::Json),
        "экспорт" | "export" => Some(Token::Export),
        "из" | "from" => Some(Token::From),
        "как" | "as" => Some(Token::As),
        "константа" | "const" => Some(Token::Const),
        "тип" | "псевдоним" | "type" | "typealias" => Some(Token::TypeAlias),
        "выбор" | "сопоставить" | "match" => Some(Token::Match),
        "вариант" | "случай" | "case" => Some(Token::Case),
        "по_умолчанию" | "default" => Some(Token::Default),
        "попытка" | "try" => Some(Token::Try),
        "исключение" | "перехват" | "поймать" | "catch" => {
            Some(Token::Catch)
        }
        "выбросить" | "бросить" | "throw" => Some(Token::Throw),
        "интерфейс" | "interface" => Some(Token::Interface),
        "реализует" | "implements" => Some(Token::Implements),
        "расширяет" | "extends" => Some(Token::Extends),
        "пока" | "while" => Some(Token::While),
        "для" | "for" => Some(Token::For),
        _ => None,
    }
}

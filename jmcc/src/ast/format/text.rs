//! Text literals, `$`-interpolations, and types: backtick variables,
//! text values, `%mod%name` substitutions, and escaping.

use super::*;

impl Formatter<'_> {
    #[instrument(skip(self, v), level = "trace")]
    pub(super) fn fmt_variable(&mut self, v: &VariableExpr) {
        let prefix = match v.scope {
            VarScope::Game => "g",
            VarScope::Save => "s",
            VarScope::Inline => "i",
            VarScope::Local => "l",
            VarScope::Line => "",
            VarScope::Jmcc => "j",
        };
        self.w(prefix);
        self.w("`");
        self.fmt_text_parts(&v.name.parts, true);
        self.w("`");
    }

    #[instrument(skip(self, tv), level = "trace")]
    pub(super) fn fmt_text_value(&mut self, tv: &TextValue) {
        let prefix = match tv.parsing {
            TextParsing::Plain => "p",
            TextParsing::Legacy => "",
            TextParsing::MiniMessage => "m",
            TextParsing::Json => "j",
        };
        self.w(prefix);
        self.w("\"");
        self.fmt_text_parts(&tv.parts, false);
        self.w("\"");
    }

    #[instrument(skip(self, tv), level = "trace")]
    pub(super) fn fmt_var_name(&mut self, tv: &TextValue) {
        if tv.parts.len() == 1
            && let TextPart::Literal(s) = &tv.parts[0]
        {
            let name = self.r(*s);
            if is_valid_plain_ident(name) {
                self.w(name);
                return;
            }
        }
        self.w("`");
        self.fmt_text_parts(&tv.parts, true);
        self.w("`");
    }

    #[instrument(skip(self, parts), level = "trace")]
    pub(super) fn fmt_text_parts(&mut self, parts: &[TextPart], backtick: bool) {
        for part in parts {
            match part {
                TextPart::Literal(s) => {
                    let raw = self.r(*s);
                    let escaped = if backtick {
                        escape_backtick_string(raw)
                    } else {
                        escape_string(raw, '"')
                    };
                    self.w(&escaped);
                }
                TextPart::Interp(eid) => {
                    self.w("${");
                    self.fmt_expr(*eid, 0);
                    self.w("}");
                }
            }
        }
    }
}

pub(super) fn escape_string(s: &str, quote: char) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            c if c == quote => {
                result.push('\\');
                result.push(c);
            }
            c => result.push(c),
        }
    }
    result
}

pub(super) fn escape_backtick_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => result.push_str("\\\\"),
            '`' => result.push_str("\\`"),
            '$' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                result.push('\\');
                result.push('$');
            }
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            c => result.push(c),
        }
        i += 1;
    }
    result
}

pub(super) fn is_valid_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    !matches!(
        name,
        "if" | "else"
            | "elif"
            | "while"
            | "for"
            | "in"
            | "break"
            | "return"
            | "var"
            | "const"
            | "function"
            | "fun"
            | "def"
            | "process"
            | "event"
            | "class"
            | "interface"
            | "enum"
            | "typealias"
            | "import"
            | "export"
            | "inline"
            | "match"
            | "try"
            | "catch"
            | "throw"
            | "true"
            | "false"
            | "not"
            | "and"
            | "or"
            | "as"
            | "ref"
            | "local"
            | "game"
            | "save"
            | "line"
            | "jmcc"
            | "если"
            | "иначе"
            | "иначе_если"
            | "пока"
            | "для"
            | "в"
            | "прервать"
            | "вернуть"
            | "пер"
            | "конст"
            | "функция"
            | "фн"
            | "процесс"
            | "событие"
            | "класс"
            | "интерфейс"
            | "перечисление"
            | "псевдоним_типа"
            | "импорт"
            | "экспорт"
            | "встроить"
            | "совпадение"
            | "попытка"
            | "поймать"
            | "бросить"
            | "правда"
            | "ложь"
            | "не"
            | "и"
            | "или"
            | "как"
            | "ссылка"
            | "локальный"
            | "игра"
            | "сохранить"
            | "линия"
    )
}

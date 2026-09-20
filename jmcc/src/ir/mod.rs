use egg::Symbol;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

pub mod arena;
pub mod builder;
pub mod codegen;
pub mod constructors;
pub mod ctx;
pub mod dunder;
pub mod hir;
pub mod hir_expand;
pub mod mir;
pub mod opt;
pub mod schema;
pub mod walk;

pub const KNOWN_OBJECTS: &[&str] = &[
    "variable",
    "entity",
    "player",
    "world",
    "item",
    "block",
    "code",
    "select",
    "repeat",
    "value",
    "controller",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarName(pub Symbol);

impl Display for VarName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}

impl FromStr for VarName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.strip_prefix('$')
            .map(|r| VarName(Symbol::from(r)))
            .ok_or_else(|| "var must start with $".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrLit(pub Symbol);

impl Display for StrLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let escaped = self
            .0
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        write!(f, "\"{escaped}\"")
    }
}

impl FromStr for StrLit {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            let inner = &s[1..s.len() - 1];
            let unescaped = inner.replace("\\\"", "\"").replace("\\\\", "\\");
            Ok(StrLit(Symbol::from(unescaped)))
        } else {
            Err("str must be quoted".into())
        }
    }
}

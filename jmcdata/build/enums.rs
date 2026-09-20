//! Перечисления генерируемого кода: схемные (`EventId`, `GameValueId`,
//! `ValueType`, `ArgType`) и по одному на каждый встречающийся набор значений
//! аргумента.

use std::collections::{HashMap, HashSet};

use heck::ToPascalCase as _;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use crate::assets::RawAction;
use crate::util::{is_boolean_values, sorted_values};

/// Набор значений аргумента, под который заводится отдельное перечисление.
pub struct ArgEnumDef {
    /// Имя типа: имя действия плюс имя аргумента в `UpperCamel`.
    pub name: String,
    /// Отсортированный набор значений.
    pub values: Vec<String>,
}

/// Подбирает валидный уникальный идентификатор варианта для строки схемы.
///
/// `heck` не обратен `serde`-овскому `rename_all`: `X_Z` и `XZ` дают один и тот
/// же `UpperCamel`, а `Self` идентификатором быть не может. Сама строка схемы
/// пишется в `#[serde(rename = ...)]` дословно, поэтому разводить приходится
/// только идентификаторы.
fn variant_ident(value: &str, used: &mut HashSet<String>) -> Ident {
    let base = match value.to_pascal_case() {
        ident if ident == "Self" => "Itself".to_owned(),
        ident => ident,
    };

    let mut ident = base.clone();
    let mut n = 2;
    while !used.insert(ident.clone()) {
        ident = format!("{base}_{n}");
        n += 1;
    }

    format_ident!("{ident}")
}

/// Единственное место, где пишется тело перечисления: варианты со своими
/// `#[serde(rename)]` и `as_str`, возвращающий строку схемы.
///
/// Схемные перечисления и наборы значений аргументов отличаются только
/// атрибутами и тем, есть ли у `as_str` док-комментарий, поэтому оба идут
/// через эту функцию.
fn enum_definition(
    enum_ident: &Ident,
    attrs: TokenStream,
    as_str_doc: TokenStream,
    values: &[String],
) -> TokenStream {
    let mut used = HashSet::new();
    let mut variants_ts = TokenStream::new();
    let mut as_str_arms = TokenStream::new();
    for v in values {
        let variant_ident = variant_ident(v, &mut used);
        let ser_lit = Literal::string(v);
        variants_ts.extend(quote! {
            #[serde(rename = #ser_lit)]
            #variant_ident,
        });
        as_str_arms.extend(quote! {
            Self::#variant_ident => #ser_lit,
        });
    }

    quote! {
        #attrs
        pub enum #enum_ident {
            #variants_ts
        }

        impl #enum_ident {
            #as_str_doc
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    #as_str_arms
                }
            }
        }
    }
}

/// Схемное перечисление: `serde`, `#[repr(u16)]` и `as_str`.
pub fn gen_enum_tokens(name: &str, mut variants: Vec<String>) -> TokenStream {
    variants.sort();
    enum_definition(
        &format_ident!("{}", name),
        quote! {
            #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            #[repr(u16)]
        },
        quote! {
            /// The schema string of the variant — exactly what serde writes.
            ///
            /// Needed to reach the by-id tables (`ACTION_DEF_BY_ID`) from an
            /// `ActionId`: `phf` cannot hash a generated enum, so the static map
            /// is keyed by this string.
        },
        &variants,
    )
}

/// Ключ — отсортированные значения через запятую: два аргумента с одним и тем
/// же набором значений делят один сгенерированный тип.
pub fn collect_arg_enums(actions: &[RawAction]) -> HashMap<String, ArgEnumDef> {
    let mut defs: HashMap<String, ArgEnumDef> = HashMap::new();

    for action in actions {
        for arg in &action.args {
            let Some(values) = &arg.values else { continue };

            if is_boolean_values(values) {
                continue;
            }
            // `scope` has a fixed value set; `VariableScope` already covers it.
            if arg.id == "scope" {
                continue;
            }

            let sorted = sorted_values(values);
            defs.entry(sorted.join(",")).or_insert_with(|| ArgEnumDef {
                name: format!(
                    "{}{}",
                    action.name.to_pascal_case(),
                    arg.id.to_pascal_case()
                ),
                values: sorted,
            });
        }
    }

    defs
}

/// `as_str` отдаёт ровно то, что дала бы сериализация `serde`, — без
/// промежуточной аллокации и `unwrap`.
pub fn gen_arg_enums(defs: &HashMap<String, ArgEnumDef>) -> (TokenStream, HashMap<String, String>) {
    let mut tokens = TokenStream::new();
    let mut mapping = HashMap::new();

    let mut keys: Vec<&String> = defs.keys().collect();
    keys.sort();

    for key in keys {
        let def = &defs[key];
        let enum_ident = format_ident!("{}", def.name);
        mapping.insert(key.clone(), def.name.clone());

        // An empty string means "no value" and gets no variant.
        let values: Vec<String> = def
            .values
            .iter()
            .filter(|v| !v.trim().is_empty())
            .cloned()
            .collect();

        tokens.extend(enum_definition(
            &enum_ident,
            quote! {
                #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            },
            TokenStream::new(),
            &values,
        ));
    }

    (tokens, mapping)
}

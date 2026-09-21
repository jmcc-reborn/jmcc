//! Описания действий: `ActionArg`/`ActionDef` и `static ACTION_DEFS` —
//! единственный экземпляр каждого описания в бинаре.

use proc_macro2::TokenStream;
use quote::quote;

use crate::assets::{RawAction, RawArg};

/// `ActionArg`/`ActionDef` — то, как описание действия выглядит в бинаре.
pub fn gen_struct_defs() -> TokenStream {
    quote! {
        #[derive(Debug, Clone)]
        pub struct ActionArg {
            pub id: &'static str,
            pub arg_type: &'static str,
            pub array: Option<u32>,
            pub values: Option<&'static [&'static str]>,
        }

        #[derive(Debug, Clone)]
        pub struct ActionDef {
            pub id: &'static str,
            pub name: &'static str,
            pub object: &'static str,
            pub args: &'static [ActionArg],
            pub action_type: &'static str,
            pub assign: Option<&'static [ActionArg]>,
            pub origin: Option<&'static str>,
            pub boolean: bool,
            pub lambda: Option<&'static [ActionArg]>,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct GameValueDef {
            pub id: &'static str,
            pub value_type: &'static str,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct EventDef {
            pub id: &'static str,
            pub cancellable: bool,
        }
    }
}

fn render_arg(arg: &RawArg) -> String {
    let (id, ty) = (arg.id.as_str(), arg.arg_type.as_str());
    let array = arg
        .array
        .map_or_else(|| "None".to_owned(), |v| format!("Some({v})"));
    let values = arg.values.as_ref().map_or_else(
        || "None".to_owned(),
        |vals| {
            let joined = vals
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Some(&[{joined}])")
        },
    );
    format!("ActionArg {{ id: \"{id}\", arg_type: \"{ty}\", array: {array}, values: {values} }}")
}

fn render_args(args: &[RawArg]) -> String {
    args.iter().map(render_arg).collect::<Vec<_>>().join(", ")
}

fn render_optional_args(args: Option<&Vec<RawArg>>) -> String {
    args.map_or_else(
        || "None".to_owned(),
        |args| format!("Some(&[{}])", render_args(args)),
    )
}

fn render_optional_str(value: Option<&String>) -> String {
    value.map_or_else(|| "None".to_owned(), |value| format!("Some(\"{value}\")"))
}

/// `ActionDef` одного действия строкой — единственное место, где пишутся его
/// поля.
fn render_action_def(action: &RawAction) -> String {
    format!(
        "ActionDef {{ id: \"{}\", name: \"{}\", object: \"{}\", args: &[{}], \
         action_type: \"{}\", assign: {}, origin: {}, boolean: {}, lambda: {} }}",
        action.id,
        action.name,
        action.object,
        render_args(&action.args),
        action.action_type,
        render_optional_args(action.assign.as_ref()),
        render_optional_str(action.origin.as_ref()),
        action.boolean,
        render_optional_args(action.lambda.as_ref()),
    )
}

/// `static`-массив со всеми описаниями действий.
///
/// Таблицы поиска ссылаются на него по индексу, поэтому описания лежат в бинаре
/// в одном экземпляре, хотя способов их найти несколько. Один массив вместо
/// `static` на каждое действие убирает повторяющуюся обвязку
/// `static <ID>: ActionDef = ActionDef { ... };`.
pub fn gen_action_defs(actions: &[RawAction]) -> String {
    let defs = actions
        .iter()
        .map(render_action_def)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "pub static ACTION_DEFS: [ActionDef; {}] = [{defs}];\n\n\
         #[must_use]\n\
         pub fn get_action_defs() -> &'static [ActionDef] {{\n    \
             &ACTION_DEFS\n\
         }}\n\n",
        actions.len()
    )
}

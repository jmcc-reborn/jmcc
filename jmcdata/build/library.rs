//! Сборка тела `generated.rs`: порядок, в котором перечисления, описания
//! действий и методы `Op` складываются в один `TokenStream`.

use proc_macro2::TokenStream;
use quote::quote;

use crate::actions::gen_struct_defs;
use crate::assets::Assets;
use crate::enums::{collect_arg_enums, gen_arg_enums, gen_enum_tokens};
use crate::op_builder::gen_action_methods;
use crate::util::unique;

/// Тело `generated.rs` без таблиц поиска — их дописывает
/// [`crate::lookups::gen_lookup_maps`].
pub fn gen_library(assets: &Assets) -> TokenStream {
    let arg_enums = collect_arg_enums(&assets.actions);
    let (arg_enum_tokens, enum_mapping) = gen_arg_enums(&arg_enums);

    let mut tokens = TokenStream::new();

    tokens.extend(quote! {
        use crate::{module::{Op, Value, VariableScope, Conditional}, op_builder::OpBuilder};

        /// A boolean in the spelling `JustMC` expects.
        #[allow(dead_code)]
        fn b(a: bool) -> std::borrow::Cow<'static, str> {
            if a { "TRUE".into() } else { "FALSE".into() }
        }

        /// An `Enum` value from a schema string; the builder methods never fill
        /// in the variable/scope pair, so this is the only shape they build.
        const fn e(value: std::borrow::Cow<'_, str>) -> Value<'_> {
            Value::Enum {
                value,
                variable: None,
                scope: None,
            }
        }
    });

    tokens.extend(gen_enum_tokens(
        "EventId",
        unique(assets.events.iter().map(|e| e.id.clone())),
    ));
    tokens.extend(gen_enum_tokens(
        "GameValueId",
        unique(assets.game_values.iter().map(|gv| gv.id.clone())),
    ));
    tokens.extend(gen_enum_tokens(
        "ValueType",
        unique(assets.game_values.iter().map(|gv| gv.value_type.clone())),
    ));
    tokens.extend(gen_enum_tokens(
        "ArgType",
        unique(
            assets
                .actions
                .iter()
                .flat_map(|a| &a.args)
                .map(|arg| arg.arg_type.clone()),
        ),
    ));

    tokens.extend(arg_enum_tokens);

    tokens.extend(gen_struct_defs());

    let methods_ts = gen_action_methods(&assets.actions, &enum_mapping);
    // Argument counts are dictated by the JustMC schema, not by this design.
    tokens.extend(quote! {
        #[allow(clippy::too_many_arguments)]
        impl<'a> Op<'a> {
            #methods_ts
        }
    });

    tokens.extend(gen_enum_tokens(
        "ActionId",
        unique(assets.actions.iter().map(|a| a.id.clone())),
    ));

    tokens
}

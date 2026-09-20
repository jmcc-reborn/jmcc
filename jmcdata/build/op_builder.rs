//! Генерация `impl Op<'a>`: по методу-конструктору на каждое действие.

use std::collections::HashMap;

use heck::ToPascalCase as _;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::assets::RawAction;
use crate::util::{is_boolean_values, sorted_values};

/// `self` и другие зарезервированные слова не могут стоять в сигнатуре как
/// обычные идентификаторы.
fn arg_ident_raw(id: &str) -> &str {
    match id {
        "type" => "r#type",
        "match" => "r#match",
        "self" => "r#self",
        other => other,
    }
}

/// Один метод на действие: сигнатура из аргументов схемы и сборка через
/// `OpBuilder`.
pub fn gen_action_methods(
    actions: &[RawAction],
    enum_mapping: &HashMap<String, String>,
) -> TokenStream {
    let mut methods_ts = TokenStream::new();

    for action in actions {
        let action_id_ident = format_ident!("{}", action.id.to_pascal_case());
        let fn_name = format_ident!("{}_{}", action.object, action.name);
        let is_container =
            action.action_type == "container" || action.action_type == "container_with_conditional";

        let mut sig_parts = Vec::<TokenStream>::new();
        let mut builder_ts = TokenStream::new();

        if action.action_type == "container_with_conditional" {
            sig_parts.push(quote! { conditional: Conditional });
        }

        for arg in &action.args {
            let arg_jmc = arg.id.as_str();
            let arg_ident = format_ident!("{}", arg_ident_raw(arg_jmc));
            let arg_ident_inner = format_ident!("{}_val", arg_ident_raw(arg_jmc));

            if arg.id == "scope" {
                sig_parts.push(quote! { #arg_ident: impl Into<VariableScope> });
                builder_ts.extend(quote! {
                    let #arg_ident_inner = #arg_ident.into();
                    let builder = builder.with_value(#arg_jmc, e(#arg_ident_inner.as_str().into()));
                });
            } else if let Some(values) = &arg.values {
                if is_boolean_values(values) {
                    sig_parts.push(quote! { #arg_ident: impl Into<bool> });
                    builder_ts.extend(quote! {
                        let builder = builder.with_value(#arg_jmc, e(b(#arg_ident.into())));
                    });
                } else {
                    let key = sorted_values(values).join(",");
                    let enum_name = enum_mapping
                        .get(&key)
                        .expect("every value set has a generated enum");
                    let enum_ident = format_ident!("{}", enum_name);
                    sig_parts.push(quote! { #arg_ident: Option<#enum_ident> });
                    builder_ts.extend(quote! {
                        let builder = if let Some(v) = #arg_ident {
                            builder.with_value(#arg_jmc, e(v.as_str().into()))
                        } else {
                            builder
                        };
                    });
                }
            } else {
                sig_parts.push(quote! { #arg_ident: impl Into<Value<'a>> });
                builder_ts.extend(quote! {
                    let #arg_ident_inner = #arg_ident.into();
                    let builder = builder.with_value(#arg_jmc, #arg_ident_inner);
                });
            }
        }

        if is_container {
            sig_parts.push(quote! { operations: Vec<Op<'a>> });
        }

        let sig_ts = sig_parts
            .into_iter()
            .reduce(|acc, part| quote! { #acc , #part })
            .unwrap_or_default();

        if action.action_type == "container_with_conditional" {
            builder_ts.extend(quote! {
                let builder = builder.with_conditional(conditional);
            });
        }

        if is_container {
            builder_ts.extend(quote! {
                let builder = builder.with_operations(operations);
            });
        }

        methods_ts.extend(quote! {
            #[inline]
            #[must_use]
            pub fn #fn_name(#sig_ts) -> Op<'a> {
                let builder = OpBuilder::new(ActionId::#action_id_ident);
                #builder_ts
                builder.build()
            }
        });
    }

    methods_ts
}

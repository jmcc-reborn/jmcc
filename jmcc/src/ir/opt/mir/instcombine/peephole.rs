//! `ActionPeepholePass`: combines adjacent compatible platform actions and specializes
//! operations into native `JustMC` actions (`increment`, `decrement`, merged `send_message`, `append_list`).

use crate::ir::StrLit;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use crate::ir::opt::mir::common::{get_variable_name, node_str_eq, references_any_variable};
use egg::*;
use std::collections::{HashMap, HashSet};

fn structural_eq(expr: &RecExpr<Mir>, a: Id, b: Id) -> bool {
    if a == b {
        return true;
    }
    let (na, nb) = (&expr[a], &expr[b]);
    if std::mem::discriminant(na) != std::mem::discriminant(nb) {
        return false;
    }
    let (ca, cb) = (na.children(), nb.children());
    if ca.len() != cb.len() {
        return false;
    }
    if ca.is_empty() {
        return na == nb;
    }
    ca.iter().zip(cb).all(|(&x, &y)| structural_eq(expr, x, y))
}

fn extract_action_args_map(expr: &RecExpr<Mir>, id: Id) -> Option<HashMap<String, Id>> {
    let ids = match &expr[id] {
        Mir::Action(ids) if ids.len() >= 4 => ids,
        _ => return None,
    };
    let Mir::List(args) = &expr[ids[3]] else {
        return None;
    };
    let mut map = HashMap::new();
    for &a in args {
        if let Mir::Named([n, v]) = &expr[a]
            && let Mir::Str(s) = &expr[*n]
        {
            map.insert(s.0.as_str().to_owned(), *v);
        }
    }
    Some(map)
}

/// Checks if an action is a candidate for increment/decrement specialization:
/// `variable::add(var, [var, K])` -> `variable::increment(var, K)`
/// `variable::subtract(var, [var, K])` -> `variable::decrement(var, K)`
fn try_specialize_inc_dec(
    expr: &RecExpr<Mir>,
    id: Id,
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    let ids = match &expr[id] {
        Mir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };

    if !node_str_eq(&expr[ids[0]], "variable") {
        return None;
    }

    let is_add = node_str_eq(&expr[ids[1]], "add");
    let is_sub = node_str_eq(&expr[ids[1]], "subtract");
    if !is_add && !is_sub {
        return None;
    }

    let args = extract_action_args_map(expr, id)?;
    let var_id = args.get("variable").copied()?;
    let val_id = args.get("value").copied()?;

    // In JustMC, variable::add/subtract seeds its accumulator with the first item of value.
    // Therefore, it only represents an increment/decrement of `var` if `value` is a 2-element
    // list [var, step]. A bare number `value = step` *replaces* `var` with `step`.
    let step_id = match &expr[val_id] {
        Mir::List(items) if items.len() == 2 => {
            if !structural_eq(expr, items[0], var_id) {
                return None;
            }
            items[1]
        }
        _ => return None,
    };

    let step_num = match &expr[step_id] {
        Mir::Num(n) if n.0 > 0.0 => *n,
        _ => return None,
    };

    let new_name_sym = if is_add { "increment" } else { "decrement" };

    let mapped_var = id_map[usize::from(var_id)];
    let mapped_step = nn.add(Mir::Num(step_num));

    let str_var = nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_num = nn.add(Mir::Str(StrLit(Symbol::from("number"))));

    let named_var = nn.add(Mir::Named([str_var, mapped_var]));
    let named_num = nn.add(Mir::Named([str_num, mapped_step]));

    let args_list = nn.add(Mir::List(vec![named_var, named_num].into_boxed_slice()));
    let str_obj = nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_name = nn.add(Mir::Str(StrLit(Symbol::from(new_name_sym))));

    let mapped_sel = id_map[usize::from(ids[2])];
    let mapped_block = id_map[usize::from(ids[4])];
    let mapped_lambda = id_map[usize::from(ids[5])];
    let mapped_cond = id_map[usize::from(ids[6])];

    Some(
        nn.add(Mir::Action(
            vec![
                str_obj,
                str_name,
                mapped_sel,
                args_list,
                mapped_block,
                mapped_lambda,
                mapped_cond,
            ]
            .into_boxed_slice(),
        )),
    )
}

/// Unpacks a `messages` argument into a vector of IDs.
fn unpack_messages(expr: &RecExpr<Mir>, msg_arg: Id) -> Vec<Id> {
    match &expr[msg_arg] {
        Mir::List(items) => items.to_vec(),
        _ => vec![msg_arg],
    }
}

/// Tries to merge two consecutive `player::message` actions if they target the same receiver
/// and have compatible merging options.
fn try_merge_player_messages(
    expr: &RecExpr<Mir>,
    act1: Id,
    act2: Id,
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    let ids1 = match &expr[act1] {
        Mir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };
    let ids2 = match &expr[act2] {
        Mir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };

    if !node_str_eq(&expr[ids1[0]], "player")
        || !node_str_eq(&expr[ids1[1]], "message")
        || !node_str_eq(&expr[ids2[0]], "player")
        || !node_str_eq(&expr[ids2[1]], "message")
    {
        return None;
    }

    // Both must have matching target selectors.
    if !structural_eq(expr, ids1[2], ids2[2]) {
        return None;
    }

    // Must have empty conditions, blocks, and lambdas.
    if !matches!(&expr[ids1[4]], Mir::Nop)
        || !matches!(&expr[ids1[5]], Mir::Nop)
        || !matches!(&expr[ids1[6]], Mir::Nop)
        || !matches!(&expr[ids2[4]], Mir::Nop)
        || !matches!(&expr[ids2[5]], Mir::Nop)
        || !matches!(&expr[ids2[6]], Mir::Nop)
    {
        return None;
    }

    let args1 = extract_action_args_map(expr, act1)?;
    let args2 = extract_action_args_map(expr, act2)?;

    let msg1_id = args1.get("messages").copied()?;
    let msg2_id = args2.get("messages").copied()?;

    let merging1 = args1.get("merging").copied();
    let merging2 = args2.get("merging").copied();

    let items1 = unpack_messages(expr, msg1_id);
    let items2 = unpack_messages(expr, msg2_id);

    let is_sep_lines_or_single = |merging: Option<Id>, count: usize| -> bool {
        merging.map_or(count <= 1, |m| node_str_eq(&expr[m], "SEPARATE_LINES"))
    };

    // Two separate player::message actions broadcast two separate lines.
    // Merging them into a single action requires SEPARATE_LINES mode so they
    // do not get concatenated into a single string.
    if !is_sep_lines_or_single(merging1, items1.len())
        || !is_sep_lines_or_single(merging2, items2.len())
    {
        return None;
    }

    // Hard limit on message array size is 18 in JustMC schema.
    if items1.len() + items2.len() > 18 {
        return None;
    }

    let mut combined_items = items1;
    combined_items.extend(items2);
    let mapped_items: Vec<Id> = combined_items
        .iter()
        .map(|&i| id_map[usize::from(i)])
        .collect();
    let new_msg_list = nn.add(Mir::List(mapped_items.into_boxed_slice()));

    let str_messages = nn.add(Mir::Str(StrLit(Symbol::from("messages"))));
    let named_messages = nn.add(Mir::Named([str_messages, new_msg_list]));

    let str_merging = nn.add(Mir::Str(StrLit(Symbol::from("merging"))));
    let str_sep_lines = nn.add(Mir::Str(StrLit(Symbol::from("SEPARATE_LINES"))));
    let named_merging = nn.add(Mir::Named([str_merging, str_sep_lines]));

    let new_named_args = vec![named_messages, named_merging];

    let args_list = nn.add(Mir::List(new_named_args.into_boxed_slice()));
    let str_obj = nn.add(Mir::Str(StrLit(Symbol::from("player"))));
    let str_name = nn.add(Mir::Str(StrLit(Symbol::from("message"))));

    let mapped_sel = id_map[usize::from(ids1[2])];
    let mapped_block = id_map[usize::from(ids1[4])];
    let mapped_lambda = id_map[usize::from(ids1[5])];
    let mapped_cond = id_map[usize::from(ids1[6])];

    Some(
        nn.add(Mir::Action(
            vec![
                str_obj,
                str_name,
                mapped_sel,
                args_list,
                mapped_block,
                mapped_lambda,
                mapped_cond,
            ]
            .into_boxed_slice(),
        )),
    )
}

struct AppendMergeCtx<'a> {
    expr: &'a RecExpr<Mir>,
    id_map: &'a [Id],
    nn: &'a mut NodeArena<Mir>,
}

fn try_merge_append_list(
    ctx: &mut AppendMergeCtx<'_>,
    ids1: &[Id],
    args1: &HashMap<String, Id>,
    args2: &HashMap<String, Id>,
    var1: Id,
    var_set: &HashSet<String>,
) -> Option<Id> {
    let l1_2 = args1.get("list_2").copied()?;
    let l2_2 = args2.get("list_2").copied()?;

    if references_any_variable(ctx.expr, l2_2, var_set) {
        return None;
    }

    let items1 = unpack_messages(ctx.expr, l1_2);
    let items2 = unpack_messages(ctx.expr, l2_2);

    let mut combined = items1;
    combined.extend(items2);

    let mapped_combined: Vec<Id> = combined
        .iter()
        .map(|&i| ctx.id_map[usize::from(i)])
        .collect();
    let new_l2 = ctx.nn.add(Mir::List(mapped_combined.into_boxed_slice()));

    let str_var = ctx.nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_l1 = ctx.nn.add(Mir::Str(StrLit(Symbol::from("list_1"))));
    let str_l2 = ctx.nn.add(Mir::Str(StrLit(Symbol::from("list_2"))));

    let mapped_var = ctx.id_map[usize::from(var1)];
    let named_var = ctx.nn.add(Mir::Named([str_var, mapped_var]));
    let named_l1 = ctx.nn.add(Mir::Named([str_l1, mapped_var]));
    let named_l2 = ctx.nn.add(Mir::Named([str_l2, new_l2]));

    let args_list = ctx.nn.add(Mir::List(
        vec![named_var, named_l1, named_l2].into_boxed_slice(),
    ));
    let str_obj = ctx.nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_name = ctx.nn.add(Mir::Str(StrLit(Symbol::from("append_list"))));

    let mapped_sel = ctx.id_map[usize::from(ids1[2])];
    let mapped_block = ctx.id_map[usize::from(ids1[4])];
    let mapped_lambda = ctx.id_map[usize::from(ids1[5])];
    let mapped_cond = ctx.id_map[usize::from(ids1[6])];

    Some(
        ctx.nn.add(Mir::Action(
            vec![
                str_obj,
                str_name,
                mapped_sel,
                args_list,
                mapped_block,
                mapped_lambda,
                mapped_cond,
            ]
            .into_boxed_slice(),
        )),
    )
}

fn try_merge_append_value(
    ctx: &mut AppendMergeCtx<'_>,
    ids1: &[Id],
    args1: &HashMap<String, Id>,
    args2: &HashMap<String, Id>,
    var1: Id,
    var_set: &HashSet<String>,
) -> Option<Id> {
    let vals1 = args1.get("values").copied()?;
    let vals2 = args2.get("values").copied()?;

    if references_any_variable(ctx.expr, vals2, var_set) {
        return None;
    }

    let items1 = unpack_messages(ctx.expr, vals1);
    let items2 = unpack_messages(ctx.expr, vals2);

    if items1.len() + items2.len() > 21 {
        return None;
    }

    let mut combined = items1;
    combined.extend(items2);

    let mapped_combined: Vec<Id> = combined
        .iter()
        .map(|&i| ctx.id_map[usize::from(i)])
        .collect();
    let new_vals = ctx.nn.add(Mir::List(mapped_combined.into_boxed_slice()));

    let str_var = ctx.nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_vals = ctx.nn.add(Mir::Str(StrLit(Symbol::from("values"))));

    let mapped_var = ctx.id_map[usize::from(var1)];
    let named_var = ctx.nn.add(Mir::Named([str_var, mapped_var]));
    let named_vals = ctx.nn.add(Mir::Named([str_vals, new_vals]));

    let args_list = ctx
        .nn
        .add(Mir::List(vec![named_var, named_vals].into_boxed_slice()));
    let str_obj = ctx.nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_name = ctx.nn.add(Mir::Str(StrLit(Symbol::from("append_value"))));

    let mapped_sel = ctx.id_map[usize::from(ids1[2])];
    let mapped_block = ctx.id_map[usize::from(ids1[4])];
    let mapped_lambda = ctx.id_map[usize::from(ids1[5])];
    let mapped_cond = ctx.id_map[usize::from(ids1[6])];

    Some(
        ctx.nn.add(Mir::Action(
            vec![
                str_obj,
                str_name,
                mapped_sel,
                args_list,
                mapped_block,
                mapped_lambda,
                mapped_cond,
            ]
            .into_boxed_slice(),
        )),
    )
}

/// Tries to merge two consecutive `variable::append_list` or `variable::append_value` actions.
fn try_merge_append_actions(
    expr: &RecExpr<Mir>,
    act1: Id,
    act2: Id,
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    let ids1 = match &expr[act1] {
        Mir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };
    let ids2 = match &expr[act2] {
        Mir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };

    if !node_str_eq(&expr[ids1[0]], "variable") || !node_str_eq(&expr[ids2[0]], "variable") {
        return None;
    }

    let is_append_list =
        node_str_eq(&expr[ids1[1]], "append_list") && node_str_eq(&expr[ids2[1]], "append_list");
    let is_append_value =
        node_str_eq(&expr[ids1[1]], "append_value") && node_str_eq(&expr[ids2[1]], "append_value");

    if !is_append_list && !is_append_value {
        return None;
    }

    let args1 = extract_action_args_map(expr, act1)?;
    let args2 = extract_action_args_map(expr, act2)?;

    let var1 = args1.get("variable").copied()?;
    let var2 = args2.get("variable").copied()?;

    let var1_name = get_variable_name(expr, var1)?;
    let var2_name = get_variable_name(expr, var2)?;

    if var1_name != var2_name || var1_name.as_str().contains('%') {
        return None;
    }

    let mut var_set = HashSet::new();
    var_set.insert(var1_name.to_string());

    let mut ctx = AppendMergeCtx { expr, id_map, nn };

    if is_append_list {
        try_merge_append_list(&mut ctx, ids1, &args1, &args2, var1, &var_set)
    } else {
        try_merge_append_value(&mut ctx, ids1, &args1, &args2, var1, &var_set)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActionPeepholePass;

impl FunctionPass<Mir> for ActionPeepholePass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Mir::Block(children) = node {
                let mut new_children = Vec::with_capacity(children.len());
                let mut idx = 0;

                while idx < children.len() {
                    let child = children[idx];

                    // Try merging with the next action if available.
                    if idx + 1 < children.len() {
                        let next_child = children[idx + 1];

                        // 1. Try merging consecutive player::message calls.
                        if let Some(merged) = try_merge_player_messages(
                            expr,
                            child,
                            next_child,
                            &id_map,
                            &mut new_nodes,
                        ) {
                            new_children.push(merged);
                            idx += 2;
                            continue;
                        }

                        // 2. Try merging consecutive append_list / append_value calls.
                        if let Some(merged) = try_merge_append_actions(
                            expr,
                            child,
                            next_child,
                            &id_map,
                            &mut new_nodes,
                        ) {
                            new_children.push(merged);
                            idx += 2;
                            continue;
                        }
                    }

                    // 3. Try specializing individual action (e.g. add/sub -> inc/dec).
                    if let Some(specialized) =
                        try_specialize_inc_dec(expr, child, &id_map, &mut new_nodes)
                    {
                        new_children.push(specialized);
                        idx += 1;
                        continue;
                    }

                    new_children.push(id_map[usize::from(child)]);
                    idx += 1;
                }

                new_nodes.add(Mir::Block(new_children.into_boxed_slice()))
            } else {
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                new_nodes.add(mapped)
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}

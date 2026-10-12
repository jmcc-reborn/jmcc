//! Merges consecutive `variable::set_value` actions into `variable::set_values`.

use crate::ir::StrLit;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use crate::ir::opt::mir::common::{
    extract_var_value_args, get_variable_name, is_set_value_action, references_any_variable,
};
use egg::*;
use std::collections::HashSet;

fn find_group_end(expr: &RecExpr<Mir>, children: &[Id], start: usize) -> usize {
    let mut vars_set: HashSet<String> = HashSet::new();

    let Some(first_args) = extract_var_value_args(expr, children[start]) else {
        log::trace!("    [Fold] First action at index {start} has no args, cannot group.");
        return start + 1;
    };
    let Some(first_var_name) = get_variable_name(expr, first_args.0) else {
        log::trace!(
            "    [Fold] First action at index {start} has unresolvable variable name, cannot group."
        );
        return start + 1;
    };
    if first_var_name.as_str().contains('%') {
        log::trace!(
            "    [Fold] First action at index {start} writes to dynamic placeholder var, cannot group."
        );
        return start + 1;
    }
    vars_set.insert(first_var_name.to_string());
    log::trace!("    [Fold] Group starts at index {start} with var '{first_var_name}'");

    let mut end = start + 1;
    while end < children.len() && is_set_value_action(expr, children[end]) {
        let Some((var_id, val_id)) = extract_var_value_args(expr, children[end]) else {
            log::trace!("    [Fold] Action at index {end} has no args, stopping group.");
            break;
        };

        // Folding must not change read-after-write ordering.
        if references_any_variable(expr, val_id, &vars_set)
            || references_any_variable(expr, var_id, &vars_set)
        {
            log::trace!("    [Fold] Action at index {end} depends on group vars, stopping group.");
            break;
        }

        // Folding must not change write-after-write ordering.
        let Some(var_name) = get_variable_name(expr, var_id) else {
            log::trace!(
                "    [Fold] Action at index {end} has unresolvable variable name, stopping group."
            );
            break;
        };
        if var_name.as_str().contains('%') || vars_set.contains(var_name.as_str()) {
            log::trace!(
                "    [Fold] Action at index {end} writes to duplicate or dynamic var '{var_name}', stopping group."
            );
            break;
        }

        log::trace!("    [Fold] Adding var '{var_name}' from index {end} to group.");
        vars_set.insert(var_name.to_string());
        end += 1;
    }

    log::trace!(
        "    [Fold] Group ends at index {} (size: {})",
        end,
        end - start
    );
    end
}

fn create_set_values_action(
    expr: &RecExpr<Mir>,
    group: &[Id],
    id_map: &[Id],
    new_nodes: &mut NodeArena<Mir>,
) -> Id {
    let mut mapped_vars = Vec::new();
    let mut mapped_vals = Vec::new();

    for &action_id in group {
        let (var_id, val_id) = extract_var_value_args(expr, action_id).unwrap();
        mapped_vars.push(id_map[usize::from(var_id)]);
        mapped_vals.push(id_map[usize::from(val_id)]);
    }

    let vars_list = new_nodes.add(Mir::List(mapped_vars.into_boxed_slice()));
    let vals_list = new_nodes.add(Mir::List(mapped_vals.into_boxed_slice()));

    let str_vars = new_nodes.add(Mir::Str(StrLit(Symbol::from("variables"))));
    let str_vals = new_nodes.add(Mir::Str(StrLit(Symbol::from("values"))));

    let named_vars = new_nodes.add(Mir::Named([str_vars, vars_list]));
    let named_vals = new_nodes.add(Mir::Named([str_vals, vals_list]));

    let args_list = new_nodes.add(Mir::List(vec![named_vars, named_vals].into_boxed_slice()));

    let str_obj = new_nodes.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_name = new_nodes.add(Mir::Str(StrLit(Symbol::from("set_values"))));

    let Mir::Action(first_ids) = &expr[group[0]] else {
        unreachable!()
    };
    let mapped_sel = id_map[usize::from(first_ids[2])];
    let mapped_block = id_map[usize::from(first_ids[4])];
    let mapped_lambda = id_map[usize::from(first_ids[5])];
    let mapped_cond = id_map[usize::from(first_ids[6])];

    new_nodes.add(Mir::Action(
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
    ))
}

fn flatten_block_children(expr: &RecExpr<Mir>, id: Id, out: &mut Vec<Id>) {
    match &expr[id] {
        Mir::Block(children) => {
            for &c in children {
                flatten_block_children(expr, c, out);
            }
        }
        Mir::Nop => {}
        _ => out.push(id),
    }
}

#[derive(Debug, Clone, Default)]
pub struct SetVariableFoldingPass;

impl FunctionPass<Mir> for SetVariableFoldingPass {
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
                let mut flat_children = Vec::with_capacity(children.len());
                for &child in children {
                    flatten_block_children(expr, child, &mut flat_children);
                }

                let mut new_children = Vec::with_capacity(flat_children.len());
                let mut idx = 0;

                while idx < flat_children.len() {
                    let child = flat_children[idx];

                    if is_set_value_action(expr, child) {
                        let group_end = find_group_end(expr, &flat_children, idx);

                        if group_end - idx > 1 {
                            log::trace!(
                                "  [Fold] Merging {} set_value actions into set_values at index {idx}",
                                group_end - idx
                            );
                            let folded_action = create_set_values_action(
                                expr,
                                &flat_children[idx..group_end],
                                &id_map,
                                &mut new_nodes,
                            );
                            new_children.push(folded_action);
                            idx = group_end;
                            continue;
                        }
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

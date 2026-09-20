//! Merges consecutive `variable::set_value` actions into `variable::set_values`.

use super::common::{
    extract_var_value_args, get_variable_name, is_set_value_action, references_any_variable,
};
use crate::ir::StrLit;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
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
    vars_set.insert(first_var_name.to_string());
    log::trace!("    [Fold] Group starts at index {start} with var '{first_var_name}'");

    let mut end = start + 1;
    while end < children.len() && is_set_value_action(expr, children[end]) {
        let Some((var_id, val_id)) = extract_var_value_args(expr, children[end]) else {
            log::trace!("    [Fold] Action at index {end} has no args, stopping group.");
            break;
        };

        // Folding must not change read-after-write ordering.
        if references_any_variable(expr, val_id, &vars_set) {
            log::trace!(
                "    [Fold] Action at index {end} has value depending on group vars, stopping group."
            );
            break;
        }

        // Folding must not change write-after-write ordering.
        let Some(var_name) = get_variable_name(expr, var_id) else {
            log::trace!(
                "    [Fold] Action at index {end} has unresolvable variable name, stopping group."
            );
            break;
        };
        if vars_set.contains(var_name.as_str()) {
            log::trace!(
                "    [Fold] Action at index {end} writes to duplicate var '{var_name}', stopping group."
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
    let mut variables = Vec::new();
    let mut values = Vec::new();

    for &action_id in group {
        if let Some((var_id, val_id)) = extract_var_value_args(expr, action_id) {
            variables.push(id_map[usize::from(var_id)]);
            values.push(id_map[usize::from(val_id)]);
        }
    }

    log::trace!(
        "    [Fold] Creating set_values action with {} vars and {} vals",
        variables.len(),
        values.len()
    );

    let vars_list_id = new_nodes.add(Mir::List(variables.into_boxed_slice()));

    let vals_list_id = new_nodes.add(Mir::List(values.into_boxed_slice()));

    let vars_str_id = new_nodes.add(Mir::Str(StrLit(Symbol::from("variables"))));
    let vars_named_id = new_nodes.add(Mir::Named([vars_str_id, vars_list_id]));

    let vals_str_id = new_nodes.add(Mir::Str(StrLit(Symbol::from("values"))));
    let vals_named_id = new_nodes.add(Mir::Named([vals_str_id, vals_list_id]));

    let args_list_id = new_nodes.add(Mir::List(
        vec![vars_named_id, vals_named_id].into_boxed_slice(),
    ));

    let object_id = new_nodes.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let name_id = new_nodes.add(Mir::Str(StrLit(Symbol::from("set_values"))));
    let nop_id = new_nodes.add(Mir::Nop);

    new_nodes.add(Mir::Action(
        vec![
            object_id,
            name_id,
            nop_id,
            args_list_id,
            nop_id,
            nop_id,
            nop_id,
        ]
        .into_boxed_slice(),
    ))
}

fn fold_block_children(
    expr: &RecExpr<Mir>,
    children: &[Id],
    id_map: &[Id],
    new_nodes: &mut NodeArena<Mir>,
) -> Vec<Id> {
    let mut result = Vec::new();
    let mut i = 0;

    log::trace!("  [Fold] Processing block with {} children", children.len());

    while i < children.len() {
        if is_set_value_action(expr, children[i]) {
            log::trace!("  [Fold] Found set_value action at index {i}");
            let group_end = find_group_end(expr, children, i);

            if group_end - i > 1 {
                let group = &children[i..group_end];
                log::trace!(
                    "  [Fold] Merging actions from index {} to {}",
                    i,
                    group_end - 1
                );
                let new_action_id = create_set_values_action(expr, group, id_map, new_nodes);
                result.push(new_action_id);
            } else {
                log::trace!("  [Fold] Single set_value at index {i}, copying as-is");
                let node = &expr[children[i]];
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                let id = new_nodes.add(mapped);
                result.push(id);
            }

            i = group_end;
        } else {
            let node = &expr[children[i]];
            let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
            let id = new_nodes.add(mapped);
            result.push(id);
            i += 1;
        }
    }

    result
}

/// Folds consecutive `variable::set_value` actions into `variable::set_values`.
#[must_use]
pub fn fold_set_variables(expr: &RecExpr<Mir>) -> RecExpr<Mir> {
    if expr.is_empty() {
        return expr.clone();
    }

    let mut new_nodes = NodeArena::with_capacity(expr.len());
    let mut id_map = vec![Id::from(0); expr.len()];

    for (i, node) in expr.as_ref().iter().enumerate() {
        let old_id = Id::from(i);

        let new_id = if let Mir::Block(children) = node {
            let folded = fold_block_children(expr, children, &id_map, &mut new_nodes);
            new_nodes.add(Mir::Block(folded.into_boxed_slice()))
        } else {
            let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
            new_nodes.add(mapped)
        };

        id_map[usize::from(old_id)] = new_id;
    }

    new_nodes.into_recexpr()
}

#[derive(Debug, Clone, Default)]
pub struct SetVariableFoldingPass;

impl FunctionPass<Mir> for SetVariableFoldingPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        fold_set_variables(expr)
    }
}

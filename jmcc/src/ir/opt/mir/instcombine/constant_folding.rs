//! Constant folding pass for MIR.
//!
//! Evaluates operations on constant values at compile time across multiple types:
//! - **Strings/Text**: `Concat` merging, `join_text`, `get_text_length`, `trim_text`, `replace_text`.
//! - **Lists**: `get_list_value`, `get_list_length`, `contains_value`.
//! - **Maps**: `get_map_value`, `get_map_size`.
//! - **Vectors & Locations**: `get_vector_component`, `get_coordinate`, `get_vector_length`.

use crate::ir::StrLit;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use crate::ir::opt::mir::common::node_str_eq;
use egg::*;
use ordered_float::OrderedFloat;
use std::collections::HashMap;

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

fn extract_string_lit(expr: &RecExpr<Mir>, id: Id) -> Option<&str> {
    match &expr[id] {
        Mir::Str(s) => Some(s.0.as_str()),
        Mir::Text([s, _]) => match &expr[*s] {
            Mir::Str(lit) => Some(lit.0.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn extract_number_lit(expr: &RecExpr<Mir>, id: Id) -> Option<f64> {
    match &expr[id] {
        Mir::Num(n) => Some(n.0),
        _ => None,
    }
}

fn extract_enum_str(expr: &RecExpr<Mir>, id: Id) -> Option<String> {
    std::iter::once(id)
        .chain(expr[id].children().iter().copied())
        .find_map(|i| {
            if let Mir::Str(s) = &expr[i] {
                Some(s.0.to_string())
            } else {
                None
            }
        })
}

/// Creates a `variable::set_value(variable = target, value = val)` action.
fn make_set_value_action(
    target_var: Id,
    val_id: Id,
    first_ids: &[Id],
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Id {
    let str_var = nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_val = nn.add(Mir::Str(StrLit(Symbol::from("value"))));

    let named_var = nn.add(Mir::Named([str_var, target_var]));
    let named_val = nn.add(Mir::Named([str_val, val_id]));

    let args_list = nn.add(Mir::List(vec![named_var, named_val].into_boxed_slice()));
    let str_obj = nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let str_name = nn.add(Mir::Str(StrLit(Symbol::from("set_value"))));

    let mapped_sel = id_map[usize::from(first_ids[2])];
    let mapped_block = id_map[usize::from(first_ids[4])];
    let mapped_lambda = id_map[usize::from(first_ids[5])];
    let mapped_cond = id_map[usize::from(first_ids[6])];

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
    ))
}

/// Attempts to fold a `Concat` node by merging adjacent string literals.
fn fold_concat(expr: &RecExpr<Mir>, children: &[Id], id_map: &[Id], nn: &mut NodeArena<Mir>) -> Id {
    let mut merged = Vec::new();
    let mut cur_str = String::new();

    for &c in children {
        if let Some(s) = extract_string_lit(expr, c) {
            cur_str.push_str(s);
        } else {
            if !cur_str.is_empty() {
                let sym = nn.add(Mir::Str(StrLit(Symbol::from(cur_str.as_str()))));
                merged.push(sym);
                cur_str.clear();
            }
            merged.push(id_map[usize::from(c)]);
        }
    }

    if !cur_str.is_empty() {
        let sym = nn.add(Mir::Str(StrLit(Symbol::from(cur_str.as_str()))));
        merged.push(sym);
    }

    if merged.len() == 1 {
        merged[0]
    } else {
        nn.add(Mir::Concat(merged.into_boxed_slice()))
    }
}

/// Folds string and text actions: `join_text`, `get_text_length`, `trim_text`, `replace_text`.
fn try_fold_text_action(
    expr: &RecExpr<Mir>,
    name: &str,
    args: &HashMap<String, Id>,
    target_var: Option<Id>,
    ids: &[Id],
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    if name == "join_text" {
        let list_id = args.get("list").copied()?;
        let sep_id = args.get("separator").copied()?;
        let sep = extract_string_lit(expr, sep_id).unwrap_or("");

        if let Mir::List(items) = &expr[list_id] {
            let mut joined = String::new();
            let mut all_strings = true;
            for (idx, &item) in items.iter().enumerate() {
                if let Some(s) = extract_string_lit(expr, item) {
                    if idx > 0 {
                        joined.push_str(sep);
                    }
                    joined.push_str(s);
                } else {
                    all_strings = false;
                    break;
                }
            }
            if all_strings {
                let res = nn.add(Mir::Str(StrLit(Symbol::from(joined.as_str()))));
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            }
        }
    } else if name == "get_text_length" {
        let text_id = args.get("text").copied()?;
        if let Some(s) = extract_string_lit(expr, text_id) {
            let len = s.chars().count() as f64;
            let res = nn.add(Mir::Num(OrderedFloat(len)));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    } else if name == "trim_text" {
        let text_id = args.get("text").copied()?;
        if let Some(s) = extract_string_lit(expr, text_id) {
            let trimmed = s.trim();
            let res = nn.add(Mir::Str(StrLit(Symbol::from(trimmed))));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    } else if name == "replace_text" {
        let text_id = args.get("text").copied()?;
        let search_id = args.get("search").copied()?;
        let replace_id = args.get("replace").copied()?;
        if let (Some(t), Some(s), Some(r)) = (
            extract_string_lit(expr, text_id),
            extract_string_lit(expr, search_id),
            extract_string_lit(expr, replace_id),
        ) {
            let replaced = t.replace(s, r);
            let res = nn.add(Mir::Str(StrLit(Symbol::from(replaced.as_str()))));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    }
    None
}

/// Folds list actions: `get_list_value`, `get_list_length`, `contains_value`.
fn try_fold_list_action(
    expr: &RecExpr<Mir>,
    name: &str,
    args: &HashMap<String, Id>,
    target_var: Option<Id>,
    ids: &[Id],
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    if name == "get_list_value" {
        let list_id = args.get("list").copied()?;
        let idx_id = args.get("number").or_else(|| args.get("index")).copied()?;
        if let (Mir::List(items), Some(n)) = (&expr[list_id], extract_number_lit(expr, idx_id)) {
            let is_valid_idx = n >= 0.0 && n.fract() == 0.0 && !n.is_nan();
            let idx = if is_valid_idx { n as usize } else { usize::MAX };
            if idx < items.len() {
                let res = id_map[usize::from(items[idx])];
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            } else if let Some(&def) = args.get("default_value") {
                let res = id_map[usize::from(def)];
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            }
        }
    } else if name == "get_list_length" {
        let list_id = args.get("list").copied()?;
        if let Mir::List(items) = &expr[list_id] {
            let res = nn.add(Mir::Num(OrderedFloat(items.len() as f64)));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    } else if name == "contains_value" {
        let list_id = args.get("list").copied()?;
        let val_id = args.get("value").copied()?;
        if let Mir::List(items) = &expr[list_id] {
            let found = items.iter().any(|&item| structural_eq(expr, item, val_id));
            let res = nn.add(Mir::Num(OrderedFloat(if found { 1.0 } else { 0.0 })));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    }
    None
}

/// Folds map actions: `get_map_value`, `get_map_size`.
fn try_fold_map_action(
    expr: &RecExpr<Mir>,
    name: &str,
    args: &HashMap<String, Id>,
    target_var: Option<Id>,
    ids: &[Id],
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    if name == "get_map_value" {
        let map_id = args.get("map").copied()?;
        let key_id = args.get("key").or_else(|| args.get("key_text")).copied()?;
        let key_str = extract_string_lit(expr, key_id);

        if let (Mir::Map(entries), Some(k)) = (&expr[map_id], key_str) {
            for &entry in entries {
                if let Mir::Named([ekey, eval]) = &expr[entry]
                    && let Some(ek) = extract_string_lit(expr, *ekey)
                    && ek == k
                {
                    let res = id_map[usize::from(*eval)];
                    return target_var
                        .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                        .or(Some(res));
                }
            }
        }
    } else if name == "get_map_size" {
        let map_id = args.get("map").copied()?;
        if let Mir::Map(entries) = &expr[map_id] {
            let res = nn.add(Mir::Num(OrderedFloat(entries.len() as f64)));
            return target_var
                .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                .or(Some(res));
        }
    }
    None
}

/// Folds vector and location component extraction and operations.
fn try_fold_vec_loc_action(
    expr: &RecExpr<Mir>,
    name: &str,
    args: &HashMap<String, Id>,
    target_var: Option<Id>,
    ids: &[Id],
    id_map: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    if name == "get_vector_component" {
        let vec_id = args.get("vector").copied()?;
        let comp_id = args.get("vector_component").copied()?;
        let comp = extract_enum_str(expr, comp_id)?;

        if let Mir::Action(vec_ids) = &expr[vec_id]
            && node_str_eq(&expr[vec_ids[0]], "variable")
            && node_str_eq(&expr[vec_ids[1]], "set_vector")
        {
            let vec_args = extract_action_args_map(expr, vec_id)?;
            let coord_arg = match comp.as_str() {
                "X" | "x" => "x",
                "Y" | "y" => "y",
                "Z" | "z" => "z",
                _ => return None,
            };
            if let Some(&c_id) = vec_args.get(coord_arg) {
                let res = id_map[usize::from(c_id)];
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            }
        }
    } else if name == "get_coordinate" {
        let loc_id = args.get("location").copied()?;
        let typ_id = args.get("type").copied()?;
        let typ = extract_enum_str(expr, typ_id)?;

        if let Mir::Action(loc_ids) = &expr[loc_id]
            && node_str_eq(&expr[loc_ids[0]], "variable")
            && node_str_eq(&expr[loc_ids[1]], "set_all_coordinates")
        {
            let loc_args = extract_action_args_map(expr, loc_id)?;
            let coord_arg = match typ.as_str() {
                "X" | "x" => "x",
                "Y" | "y" => "y",
                "Z" | "z" => "z",
                "YAW" | "yaw" => "yaw",
                "PITCH" | "pitch" => "pitch",
                _ => return None,
            };
            if let Some(&c_id) = loc_args.get(coord_arg) {
                let res = id_map[usize::from(c_id)];
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            }
        }
    } else if name == "get_vector_length" {
        let vec_id = args.get("vector").copied()?;
        if let Mir::Action(vec_ids) = &expr[vec_id]
            && node_str_eq(&expr[vec_ids[0]], "variable")
            && node_str_eq(&expr[vec_ids[1]], "set_vector")
        {
            let vec_args = extract_action_args_map(expr, vec_id)?;
            if let (Some(&x_id), Some(&y_id), Some(&z_id)) =
                (vec_args.get("x"), vec_args.get("y"), vec_args.get("z"))
                && let (Some(x), Some(y), Some(z)) = (
                    extract_number_lit(expr, x_id),
                    extract_number_lit(expr, y_id),
                    extract_number_lit(expr, z_id),
                )
            {
                let len = z.mul_add(z, y.mul_add(y, x * x)).sqrt();
                let res = nn.add(Mir::Num(OrderedFloat(len)));
                return target_var
                    .map(|tgt| make_set_value_action(tgt, res, ids, id_map, nn))
                    .or(Some(res));
            }
        }
    }
    None
}

/// Attempts to fold constant action invocations into a constant or `set_value`.
fn try_fold_action(
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

    let action_name = match &expr[ids[1]] {
        Mir::Str(s) => s.0.as_str(),
        _ => return None,
    };

    let args = extract_action_args_map(expr, id)?;
    let target_var = args.get("variable").map(|&v| id_map[usize::from(v)]);

    try_fold_text_action(expr, action_name, &args, target_var, ids, id_map, nn)
        .or_else(|| try_fold_list_action(expr, action_name, &args, target_var, ids, id_map, nn))
        .or_else(|| try_fold_map_action(expr, action_name, &args, target_var, ids, id_map, nn))
        .or_else(|| try_fold_vec_loc_action(expr, action_name, &args, target_var, ids, id_map, nn))
}

#[derive(Debug, Clone, Default)]
pub struct MirConstantFoldingPass;

impl FunctionPass<Mir> for MirConstantFoldingPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = match node {
                Mir::Concat(children) => fold_concat(expr, children, &id_map, &mut new_nodes),
                Mir::Action(_) => try_fold_action(expr, old_id, &id_map, &mut new_nodes)
                    .unwrap_or_else(|| {
                        let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                        new_nodes.add(mapped)
                    }),
                _ => {
                    let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                    new_nodes.add(mapped)
                }
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}

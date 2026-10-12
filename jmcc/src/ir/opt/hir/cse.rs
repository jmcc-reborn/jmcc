//! Common Subexpression Elimination (CSE / Local Value Numbering) pass for HIR.
//!
//! Identifies repeated pure subexpressions (such as class slot indexing `obj[slot]`
//! and pure arithmetic) within statement blocks, binds the first evaluation to
//! a compiler temporary, and reuses it for subsequent identical evaluations.

use super::common::{get_target_var_name, get_var_name};
use crate::ir::VarName;
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::{HashMap, HashSet};

fn is_pure_hir(expr: &RecExpr<Hir>, id: Id) -> bool {
    let mut pure = true;
    walk_subtree(expr, id, |node, _| {
        if matches!(
            node,
            Hir::Action(_)
                | Hir::FuncCall(_)
                | Hir::ProcCall(_)
                | Hir::While(_)
                | Hir::If(_)
                | Hir::Break
                | Hir::Continue
                | Hir::Return(_)
                | Hir::FuncDecl(_)
                | Hir::ProcDecl(_)
                | Hir::EventDecl(_)
        ) {
            pure = false;
            false
        } else {
            true
        }
    });
    pure
}

fn collect_read_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |_node, id| {
        if let Some(name) = get_var_name(expr, id) {
            out.insert(name);
        }
        true
    });
}

fn has_placeholders(expr: &RecExpr<Hir>, root: Id) -> bool {
    let mut found = false;
    walk_subtree(expr, root, |node, _| match node {
        Hir::Str(s) if s.0.as_str().contains('%') => {
            found = true;
            false
        }
        Hir::Var(v) if v.0.as_str().contains('%') => {
            found = true;
            false
        }
        _ => true,
    });
    found
}

/// Represents a structural key for a pure subexpression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExprKey {
    Index(Symbol, Symbol),          // obj_var, slot/index
    Binary(String, Symbol, Symbol), // op, left_var, right_var
}

#[derive(Debug, Clone, Default)]
pub struct CommonSubexpressionEliminationPass;

impl FunctionPass<Hir> for CommonSubexpressionEliminationPass {
    const OPT_LEVEL: u8 = 2;

    #[expect(
        clippy::too_many_lines,
        reason = "CSE traverses and substitutes HIR expressions in basic blocks"
    )]
    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len() * 2);
        let mut id_map = vec![Id::from(0); expr.len()];
        let mut temp_counter = 0usize;

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut new_children = Vec::with_capacity(children.len() * 2);
                let mut available_exprs: HashMap<ExprKey, (Symbol, HashSet<Symbol>)> =
                    HashMap::new();

                for &child in children {
                    let child_node = &expr[child];

                    // Invalidate available expressions if a variable is written
                    match child_node {
                        Hir::VarDecl([target, _]) | Hir::Set([target, _]) => {
                            if let Some(written) = get_target_var_name(expr, *target) {
                                available_exprs.retain(|_, (_, deps)| !deps.contains(&written));
                            }
                        }
                        _ => {}
                    }

                    // Check if child is VarDecl([target, val]) or Set([target, val]) with an expression
                    if let Hir::VarDecl([target, val]) | Hir::Set([target, val]) = child_node
                        && is_pure_hir(expr, *val)
                        && !has_placeholders(expr, *val)
                    {
                        let expr_key = match &expr[*val] {
                            Hir::Index([obj, slot]) => {
                                get_var_name(expr, *obj).and_then(|obj_name| {
                                    let slot_sym = match &expr[*slot] {
                                        Hir::Num(n) => Symbol::from(n.to_string()),
                                        Hir::Str(s) => s.0,
                                        Hir::Var(v) => v.0,
                                        _ => Symbol::from(""),
                                    };
                                    if !slot_sym.as_str().is_empty() {
                                        Some(ExprKey::Index(obj_name, slot_sym))
                                    } else {
                                        None
                                    }
                                })
                            }
                            Hir::Add([l, r]) | Hir::Sub([l, r]) | Hir::Mul([l, r]) => {
                                let op_str = match &expr[*val] {
                                    Hir::Add(_) => "+",
                                    Hir::Sub(_) => "-",
                                    Hir::Mul(_) => "*",
                                    _ => "",
                                };
                                if let (Some(l_name), Some(r_name)) =
                                    (get_var_name(expr, *l), get_var_name(expr, *r))
                                {
                                    Some(ExprKey::Binary(op_str.to_owned(), l_name, r_name))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };

                        if let Some(key) = expr_key {
                            if let Some((cached_var, _)) = available_exprs.get(&key) {
                                // Reuse cached variable!
                                let mapped_target = id_map[usize::from(*target)];
                                let var_sym = *cached_var;
                                let var_node = new_nodes.add(Hir::Var(VarName(var_sym)));
                                let line_node = new_nodes.add(Hir::Line(var_node));
                                let assign_node = match child_node {
                                    Hir::VarDecl(_) => {
                                        new_nodes.add(Hir::VarDecl([mapped_target, line_node]))
                                    }
                                    _ => new_nodes.add(Hir::Set([mapped_target, line_node])),
                                };
                                new_children.push(assign_node);
                                continue;
                            }

                            // First occurrence: record in available_exprs
                            if let Some(target_name) = get_target_var_name(expr, *target) {
                                let mut deps = HashSet::new();
                                collect_read_vars(expr, *val, &mut deps);
                                available_exprs.insert(key, (target_name, deps));
                            } else {
                                // Bind to a fresh temporary
                                temp_counter += 1;
                                let temp_name = Symbol::from(format!("__cse_{temp_counter}"));
                                let mut deps = HashSet::new();
                                collect_read_vars(expr, *val, &mut deps);

                                let temp_var = new_nodes.add(Hir::Var(VarName(temp_name)));
                                let temp_line = new_nodes.add(Hir::Line(temp_var));
                                let mapped_val = id_map[usize::from(*val)];
                                let decl = new_nodes.add(Hir::VarDecl([temp_line, mapped_val]));
                                new_children.push(decl);

                                let mapped_target = id_map[usize::from(*target)];
                                let assign = match child_node {
                                    Hir::VarDecl(_) => {
                                        new_nodes.add(Hir::VarDecl([mapped_target, temp_line]))
                                    }
                                    _ => new_nodes.add(Hir::Set([mapped_target, temp_line])),
                                };
                                new_children.push(assign);

                                available_exprs.insert(key, (temp_name, deps));
                                continue;
                            }
                        }
                    }

                    // Clear available expressions if control flow occurs
                    if matches!(
                        child_node,
                        Hir::While(_) | Hir::If(_) | Hir::FuncCall(_) | Hir::ProcCall(_)
                    ) {
                        available_exprs.clear();
                    }

                    new_children.push(id_map[usize::from(child)]);
                }

                new_nodes.add(Hir::Block(new_children.into_boxed_slice()))
            } else {
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                new_nodes.add(mapped)
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}

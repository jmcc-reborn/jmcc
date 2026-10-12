//! Tail Call Optimization (TCO) pass for HIR.
//!
//! Converts recursive tail calls (`return f(new_args)`) into a `while (true)`
//! loop with parameter updates, completely eliminating expensive `call_function`
//! frame allocations in `JustMC`.

use super::common::string_lit;
use crate::ir::VarName;
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use egg::{Id, Language as _, RecExpr, Symbol};

#[derive(Debug, Clone, Default)]
pub struct TailCallOptimizationPass;

impl FunctionPass<Hir> for TailCallOptimizationPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len() * 2);
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::FuncDecl([name_id, params_id, body_id]) = node {
                let func_name = string_lit(expr, *name_id);
                let mut tail_call_info = None;

                // Inspect function body to see if it ends with a tail call to func_name
                if let Some(target_name) = func_name
                    && let Hir::Block(body_children) = &expr[*body_id]
                {
                    // Check last statement
                    if let Some(&last_child) = body_children.last() {
                        let call_candidate = match &expr[last_child] {
                            Hir::Return(ret_val) => Some(*ret_val),
                            Hir::FuncCall(_) => Some(last_child),
                            _ => None,
                        };

                        if let Some(call_id) = call_candidate
                            && let Hir::FuncCall([target, args]) = &expr[call_id]
                            && string_lit(expr, *target) == Some(target_name)
                            && let Hir::List(arg_list) = &expr[*args]
                            && let Hir::List(param_list) = &expr[*params_id]
                            && arg_list.len() == param_list.len()
                        {
                            let has_ref = param_list.iter().any(|&p_id| {
                                let p_name = match &expr[p_id] {
                                    Hir::List(pair) if !pair.is_empty() => {
                                        string_lit(expr, pair[0])
                                    }
                                    _ => string_lit(expr, p_id),
                                };
                                p_name.is_some_and(|n| crate::utils::split_param_ref(n.as_str()).1)
                            });
                            if !has_ref {
                                tail_call_info =
                                    Some((last_child, arg_list.clone(), param_list.clone()));
                            }
                        }
                    }
                }

                if let Some((tail_stmt, arg_list, param_list)) = tail_call_info
                    && let Hir::Block(body_children) = &expr[*body_id]
                {
                    // Build TCO while loop:
                    // while (true) {
                    //     ... body before tail call ...
                    //     // update parameters
                    //     __tco_p0 = arg0; ...
                    //     p0 = __tco_p0; ...
                    //     continue;
                    // }
                    let true_node = new_nodes.add(Hir::Bool(true));
                    let mut loop_body_children = Vec::new();

                    // Add statements before tail call
                    for &stmt in body_children {
                        if stmt == tail_stmt {
                            break;
                        }
                        loop_body_children.push(id_map[usize::from(stmt)]);
                    }

                    // Evaluate new arguments into temporaries first to avoid clobbering
                    let mut temp_lines = Vec::new();
                    for (idx, &arg) in arg_list.iter().enumerate() {
                        let temp_sym = Symbol::from(format!("__tco_tmp_{idx}"));
                        let temp_var = new_nodes.add(Hir::Var(VarName(temp_sym)));
                        let temp_line = new_nodes.add(Hir::Line(temp_var));
                        let mapped_arg = id_map[usize::from(arg)];
                        let decl = new_nodes.add(Hir::VarDecl([temp_line, mapped_arg]));
                        loop_body_children.push(decl);
                        temp_lines.push(temp_line);
                    }

                    // Assign temporaries to parameters
                    for (idx, &param) in param_list.iter().enumerate() {
                        let mapped_param = id_map[usize::from(param)];
                        let set_param = new_nodes.add(Hir::Set([mapped_param, temp_lines[idx]]));
                        loop_body_children.push(set_param);
                    }

                    let continue_node = new_nodes.add(Hir::Continue);
                    loop_body_children.push(continue_node);

                    let loop_body =
                        new_nodes.add(Hir::Block(loop_body_children.into_boxed_slice()));
                    let while_loop = new_nodes.add(Hir::While([true_node, loop_body]));

                    let new_func_body = new_nodes.add(Hir::Block(Box::new([while_loop])));
                    let mapped_name = id_map[usize::from(*name_id)];
                    let mapped_params = id_map[usize::from(*params_id)];

                    new_nodes.add(Hir::FuncDecl([mapped_name, mapped_params, new_func_body]))
                } else {
                    let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                    new_nodes.add(mapped)
                }
            } else {
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                new_nodes.add(mapped)
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}

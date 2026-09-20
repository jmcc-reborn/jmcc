//! Dead Function Elimination: removes unused function and process declarations.

use super::super::common::{collect_declarations, string_lit};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::ModulePass;
use crate::ir::walk::{walk_subtree, walk_subtree_with};
use egg::*;
use std::collections::{HashMap, HashSet};

/// Actions naming a declaration by string: the string is a reference, same as `FuncCall`/
/// `ProcCall`. Without it DFE drops the `code::start_process("dropGenerator", ...)` target.
const NAME_REFERENCE_ACTIONS: [(&str, &str); 2] =
    [("code", "start_process"), ("code", "call_function")];

/// Declarations a string from `NAME_REFERENCE_ACTIONS` may point at: the exact name, else a
/// short name only when unambiguous — keeping all candidates would pull in half of `std`.
/// In doubt we keep: a spare declaration is only size, a dropped one is a broken program.
fn referenced_declarations<'a>(referenced: &'a str, decls: &'a Declarations) -> Vec<&'a str> {
    if decls.by_name.contains_key(referenced) {
        return vec![referenced];
    }
    match decls.by_short.get(referenced) {
        Some(names) if names.len() == 1 => names.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Object and name of a `Hir::Action` node.
fn action_target(expr: &RecExpr<Hir>, ids: &[Id]) -> Option<(Symbol, Symbol)> {
    let object = string_lit(expr, *ids.first()?)?;
    let name = string_lit(expr, *ids.get(1)?)?;
    Some((object, name))
}

/// Marks declarations an action references by string: they become live and their bodies are
/// pushed onto the traversal stack.
fn mark_string_references(
    expr: &RecExpr<Hir>,
    ids: &[Id],
    decls: &Declarations,
    used_funcs: &mut HashSet<Symbol>,
    stack: &mut Vec<Id>,
) {
    let Some((object, name)) = action_target(expr, ids) else {
        return;
    };
    if !NAME_REFERENCE_ACTIONS.contains(&(object.as_str(), name.as_str())) {
        return;
    }
    let Some(args_id) = ids.get(3) else {
        return;
    };
    if dynamic_name(expr, *args_id, name_argument(name.as_str())) {
        log::trace!(
            "DFE: '{}' is called by a name built at run time; keeping every declaration",
            name.as_str()
        );
        for (full, (_, body_id)) in &decls.by_name {
            used_funcs.insert(Symbol::from(full.as_str()));
            stack.push(*body_id);
        }
        return;
    }
    for referenced in string_args(expr, *args_id) {
        for name in referenced_declarations(referenced.as_str(), decls) {
            if !used_funcs.insert(Symbol::from(name)) {
                continue;
            }
            if let Some(&(_, body_id)) = decls.by_name.get(name) {
                stack.push(body_id);
            }
        }
    }
}

/// String literals found in an action's arguments (`ids[3]` is the argument list).
fn string_args(expr: &RecExpr<Hir>, args_id: Id) -> Vec<Symbol> {
    let mut found = Vec::new();
    walk_subtree(expr, args_id, |node, _| {
        if let Hir::Str(s) = node {
            found.push(s.0);
        }
        true
    });
    found
}

/// The argument a `NAME_REFERENCE_ACTIONS` entry takes its name from.
fn name_argument(action: &str) -> &'static str {
    if action == "start_process" {
        "process_name"
    } else {
        "function_name"
    }
}

/// Whether the name argument is something other than a literal string.
///
/// `code::call_function(name_var, …)` looks its target up at run time: nothing
/// here says which declaration it reaches, so none of them may be treated as
/// dead. The string it holds is ordinary data and says nothing either — it can
/// be built, or reassigned, before the call.
///
/// The argument may be named or leading positional; the compiler writes both.
fn dynamic_name(expr: &RecExpr<Hir>, args_id: Id, argument: &str) -> bool {
    let Hir::List(args) = &expr[args_id] else {
        // Not the shape an argument list has: nothing can be concluded, so the
        // declarations are kept.
        return true;
    };
    for &arg in args {
        if let Hir::Named([key, value]) = &expr[arg]
            && string_lit(expr, *key).is_some_and(|key| key.as_str() == argument)
        {
            return !matches!(expr[*value], Hir::Str(_));
        }
    }
    args.first()
        .is_some_and(|&first| !matches!(expr[first], Hir::Str(_)))
}

/// Function and process declarations of a module.
struct Declarations {
    /// Full name -> (declaration node, body).
    by_name: HashMap<String, (Id, Id)>,
    /// Short name -> full names. `apply_mangling` rewrites calls but not the text inside
    /// `code::start_process`, so the string stays `dropGenerator` while the declaration is
    /// `module::dropGenerator`.
    by_short: HashMap<String, Vec<String>>,
}

impl Declarations {
    fn collect(expr: &RecExpr<Hir>) -> Self {
        let mut by_short: HashMap<String, Vec<String>> = HashMap::new();
        let by_name = collect_declarations(expr)
            .into_iter()
            .map(|(name, decl)| {
                let name = name.as_str().to_owned();
                if let Some((_, short)) = name.rsplit_once("::") {
                    by_short
                        .entry(short.to_owned())
                        .or_default()
                        .push(name.clone());
                }
                (name, decl)
            })
            .collect();
        Self { by_name, by_short }
    }
}

/// Perform dead function/process elimination on a HIR module.
///
/// Removes `FuncDecl` and `ProcDecl` nodes that are never called from
/// entry points (events, top-level code, or other live functions).
#[must_use]
pub fn eliminate_dead_functions(expr: &RecExpr<Hir>, keep_tests: bool) -> RecExpr<Hir> {
    if expr.is_empty() {
        return expr.clone();
    }

    let decls = Declarations::collect(expr);

    let mut used_funcs = HashSet::new();

    // Test functions are entry points only when keep_tests is enabled
    if keep_tests {
        for (name, (_decl_id, _body_id)) in &decls.by_name {
            let short = name.rsplit_once("::").map_or(name.as_str(), |(_, s)| s);
            if short.starts_with("test_")
                || short.starts_with("тест_")
                || short == "test"
                || short == "тест"
            {
                used_funcs.insert(Symbol::from(name.as_str()));
            }
        }
    }

    walk_subtree_with(expr, expr.root(), |node, _id, stack| {
        match node {
            // Only enter the declaration body if it is already known to be live;
            // otherwise a call from an unused function would revive itself.
            Hir::FuncDecl([name_id, _, body_id]) | Hir::ProcDecl([name_id, _, body_id]) => {
                if string_lit(expr, *name_id).is_some_and(|name| used_funcs.contains(&name)) {
                    stack.push(*body_id);
                }
            }
            Hir::FuncCall([target, _]) | Hir::ProcCall([target, _]) => {
                if let Some(name) = string_lit(expr, *target)
                    && used_funcs.insert(name)
                    && let Some(&(_, body_id)) = decls.by_name.get(name.as_str())
                {
                    stack.push(body_id);
                }
                stack.extend(node.children().iter().copied());
            }
            Hir::Action(ids) => {
                mark_string_references(expr, ids, &decls, &mut used_funcs, stack);
                stack.extend(node.children().iter().copied());
            }
            _ => stack.extend(node.children().iter().copied()),
        }
    });

    let mut dead_nodes = HashSet::new();
    for (i, node) in expr.as_ref().iter().enumerate() {
        if let Hir::FuncDecl([name_id, _, _]) | Hir::ProcDecl([name_id, _, _]) = node
            && let Some(name) = string_lit(expr, *name_id)
            && !used_funcs.contains(&name)
        {
            dead_nodes.insert(Id::from(i));
            log::trace!("DFE: Marking unused function/process '{name}' as dead");
        }
    }

    if dead_nodes.is_empty() {
        return expr.clone();
    }

    let mut new_nodes = NodeArena::with_capacity(expr.len() - dead_nodes.len());
    let mut id_map = vec![Id::from(0); expr.len()];

    for (i, node) in expr.as_ref().iter().enumerate() {
        let old_id = Id::from(i);

        if dead_nodes.contains(&old_id) {
            continue;
        }

        let mapped_node = if let Hir::Block(children) = node {
            let new_children: Vec<Id> = children
                .iter()
                .filter(|&&c| !dead_nodes.contains(&c))
                .map(|c| id_map[usize::from(*c)])
                .collect();

            if new_children.is_empty() {
                Hir::Nop
            } else {
                Hir::Block(new_children.into_boxed_slice())
            }
        } else {
            node.clone().map_children(|c| id_map[usize::from(c)])
        };

        let new_id = new_nodes.add(mapped_node);
        id_map[usize::from(old_id)] = new_id;
    }

    new_nodes.into_recexpr()
}

#[derive(Debug, Clone, Default)]
pub struct DeadFunctionEliminationPass {
    pub keep_tests: bool,
}

impl ModulePass<Hir> for DeadFunctionEliminationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        eliminate_dead_functions(expr, self.keep_tests)
    }
}

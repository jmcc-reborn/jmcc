//! Copy Propagation pass: replaces uses of variables that are copies of other variables or constants.

use crate::ir::VarName;
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::opt::hir::common::{get_var_name, get_var_name_in, string_lit};
use egg::{Id, Language as _, RecExpr, Symbol};
use jmcdata::generated::get_action_def;
use std::collections::{HashMap, HashSet};

/// The arguments of an action that name a variable instead of reading a value.
///
/// The schema tells them apart by `arg_type`: a `variable` argument is the
/// target of the action (`variable::exists`, `variable::append_list`), every
/// other one is a value. An action the schema does not know gives an empty set:
/// nothing can be said about its arguments, and treating a name as a value is
/// what the pass has always done.
fn target_args(expr: &RecExpr<Hir>, ids: &[Id]) -> HashSet<Symbol> {
    let (Some(object), Some(name)) = (string_lit(expr, ids[0]), string_lit(expr, ids[1])) else {
        return HashSet::new();
    };
    let Some(def) = get_action_def(object.as_str(), name.as_str()) else {
        return HashSet::new();
    };
    def.args
        .iter()
        .filter(|arg| arg.arg_type == "variable")
        .map(|arg| Symbol::from(arg.id))
        .collect()
}

/// The variables an action writes through its `variable`-typed arguments.
///
/// [`target_args`] names the arguments; this resolves each of them to the
/// variable it points at, which is what the copy map is keyed by. An action that
/// writes a variable makes every copy of it stale — including the one the
/// action's own result is read from right afterwards.
fn written_targets(expr: &RecExpr<Hir>, ids: &[Id], targets: &HashSet<Symbol>) -> HashSet<Symbol> {
    let Some(&args) = ids.get(3) else {
        return HashSet::new();
    };
    let mut names = HashSet::new();
    let mut stack = vec![args];
    while let Some(id) = stack.pop() {
        match &expr[id] {
            Hir::Named([name, value])
                if string_lit(expr, *name).is_some_and(|name| targets.contains(&name)) =>
            {
                if let Some(variable) = get_var_name(expr, *value) {
                    names.insert(variable);
                }
            }
            _ => stack.extend(expr[id].children().iter().copied()),
        }
    }
    names
}

/// The variables a body writes, directly or through the control flow inside it.
///
/// Used where a value known before a construct stops being known inside it: a
/// loop body may run any number of times, so everything it assigns is unknown at
/// the loop's head.
fn written_vars(expr: &RecExpr<Hir>, id: Id) -> HashSet<Symbol> {
    let mut names = HashSet::new();
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        match &expr[id] {
            Hir::Set([target, value]) | Hir::VarDecl([target, value]) => {
                if let Some(name) = get_var_name(expr, *target) {
                    names.insert(name);
                }
                stack.push(*value);
            }
            Hir::Inc(inner) | Hir::Dec(inner) => {
                if let Some(name) = get_var_name(expr, *inner) {
                    names.insert(name);
                }
            }
            Hir::Action(ids) => {
                names.extend(target_args(expr, ids));
                stack.extend(expr[id].children().iter().copied());
            }
            _ => stack.extend(expr[id].children().iter().copied()),
        }
    }
    names
}

struct CopyPropContext<'a> {
    expr: &'a RecExpr<Hir>,
    new_nodes: NodeArena<Hir>,
}

enum ScopeKind {
    Line,
    Local,
    Game,
    Save,
}

enum FunctionKind {
    Function,
    Process,
}

enum CopyResolution {
    Variable(Id),
    Value(Id),
}

impl<'a> CopyPropContext<'a> {
    fn new(expr: &'a RecExpr<Hir>) -> Self {
        Self {
            expr,
            new_nodes: NodeArena::with_capacity(expr.len()),
        }
    }

    #[inline(always)]
    fn add(&mut self, node: Hir) -> Id {
        let id = self.new_nodes.add(node);
        log::trace!(
            "  cp: add node id={id:?}: {:?}",
            self.new_nodes.as_slice()[usize::from(id)]
        );
        id
    }

    fn get_var_name_new(&self, id: Id) -> Option<Symbol> {
        get_var_name_in(self.new_nodes.as_slice(), id)
    }

    fn is_simple_value_new(&self, id: Id) -> bool {
        match &self.new_nodes.as_slice()[usize::from(id)] {
            Hir::Num(_) | Hir::Bool(_) | Hir::Str(_) | Hir::Var(_) => true,
            Hir::Line(inner) | Hir::Local(inner) | Hir::Game(inner) | Hir::Save(inner) => {
                self.is_simple_value_new(*inner)
            }
            _ => false,
        }
    }

    fn clone_var_node(&mut self, id: Id) -> Id {
        let node = self.expr[id].clone();
        match node {
            Hir::Line(inner) => {
                let new_inner = self.clone_var_node(inner);
                self.add(Hir::Line(new_inner))
            }
            Hir::Local(inner) => {
                let new_inner = self.clone_var_node(inner);
                self.add(Hir::Local(new_inner))
            }
            Hir::Game(inner) => {
                let new_inner = self.clone_var_node(inner);
                self.add(Hir::Game(new_inner))
            }
            Hir::Save(inner) => {
                let new_inner = self.clone_var_node(inner);
                self.add(Hir::Save(new_inner))
            }
            _ => self.add(node),
        }
    }

    fn visit_target(&mut self, id: Id, copy_map: &mut HashMap<Symbol, Id>) -> Id {
        if get_var_name(self.expr, id).is_some() {
            self.clone_var_node(id)
        } else {
            self.visit(id, copy_map)
        }
    }

    fn resolve_copy(&self, copy_map: &HashMap<Symbol, Id>, name: Symbol) -> Option<CopyResolution> {
        let mut current_name = name;
        let mut current_id = None;
        let mut iterations = 0;
        let max_iterations = copy_map.len() + 1;

        while let Some(&next_id) = copy_map.get(&current_name) {
            current_id = Some(next_id);
            if let Some(next_name) = self.get_var_name_new(next_id) {
                if next_name != current_name && iterations <= max_iterations {
                    current_name = next_name;
                    iterations += 1;
                    continue;
                }
                return Some(CopyResolution::Variable(next_id));
            } else {
                return Some(CopyResolution::Value(next_id));
            }
        }
        current_id.map(CopyResolution::Variable)
    }

    fn try_resolve_var(&self, id: Id, copy_map: &HashMap<Symbol, Id>) -> Option<Id> {
        if let Hir::Var(VarName(name)) = &self.expr[id] {
            return self
                .resolve_copy(copy_map, *name)
                .map(|resolution| match resolution {
                    CopyResolution::Variable(id) | CopyResolution::Value(id) => id,
                });
        }
        None
    }

    fn visit(&mut self, id: Id, copy_map: &mut HashMap<Symbol, Id>) -> Id {
        let node = self.expr[id].clone();

        match node {
            Hir::Var(VarName(name)) => match self.resolve_copy(copy_map, name) {
                Some(CopyResolution::Variable(id)) => {
                    log::trace!("  cp: replace var {name} -> var {id:?}");
                    id
                }
                Some(CopyResolution::Value(id)) => {
                    log::trace!("  cp: replace var {name} -> const {id:?}");
                    id
                }
                None => self.add(Hir::Var(VarName(name))),
            },

            Hir::Line(inner) => self.visit_scoped(inner, copy_map, ScopeKind::Line),
            Hir::Local(inner) => self.visit_scoped(inner, copy_map, ScopeKind::Local),
            Hir::Game(inner) => self.visit_scoped(inner, copy_map, ScopeKind::Game),
            Hir::Save(inner) => self.visit_scoped(inner, copy_map, ScopeKind::Save),

            Hir::Let(ids) => self.visit_let(ids, copy_map),
            Hir::Set(ids) => self.visit_set(ids, copy_map),
            Hir::VarDecl(ids) => self.visit_var_decl(ids, copy_map),
            Hir::Inc(inner) => self.visit_update(inner, copy_map, true),
            Hir::Dec(inner) => self.visit_update(inner, copy_map, false),

            Hir::Block(ids) => self.visit_block(&ids, copy_map),
            Hir::If(ids) => self.visit_if(ids, copy_map),
            Hir::While(ids) => self.visit_while(ids, copy_map),

            Hir::FuncDecl(ids) => self.visit_function(ids, copy_map, FunctionKind::Function),
            Hir::ProcDecl(ids) => self.visit_function(ids, copy_map, FunctionKind::Process),
            Hir::EventDecl(ids) => self.visit_event(ids, copy_map),
            Hir::ClassDecl(ids) => self.visit_class(ids, copy_map),

            Hir::Action(ids) => self.visit_action(&ids, copy_map),

            Hir::Break | Hir::Continue | Hir::Nop => self.add(node),

            _ => self.visit_children(node, copy_map),
        }
    }

    fn visit_scoped(
        &mut self,
        inner: Id,
        copy_map: &mut HashMap<Symbol, Id>,
        scope: ScopeKind,
    ) -> Id {
        if let Some(id) = self.try_resolve_var(inner, copy_map) {
            return id;
        }
        let inner = self.visit(inner, copy_map);
        self.add(match scope {
            ScopeKind::Line => Hir::Line(inner),
            ScopeKind::Local => Hir::Local(inner),
            ScopeKind::Game => Hir::Game(inner),
            ScopeKind::Save => Hir::Save(inner),
        })
    }

    fn visit_let(
        &mut self,
        [variable, value, body]: [Id; 3],
        copies: &mut HashMap<Symbol, Id>,
    ) -> Id {
        let value = self.visit(value, copies);
        let new_variable = self.clone_var_node(variable);
        let variable_name = get_var_name(self.expr, variable);
        let saved = variable_name.and_then(|name| {
            let old = copies.get(&name).copied();
            if self.is_simple_value_new(value) {
                log::trace!("  cp: let copy/const {name} -> {value:?}");
                copies.insert(name, value);
            } else {
                copies.remove(&name);
            }
            old
        });
        let body = self.visit(body, copies);
        if let Some(name) = variable_name {
            if let Some(old) = saved {
                copies.insert(name, old);
            } else {
                copies.remove(&name);
            }
        }
        self.add(Hir::Let([new_variable, value, body]))
    }

    fn visit_set(&mut self, [target, value]: [Id; 2], copies: &mut HashMap<Symbol, Id>) -> Id {
        let value = self.visit(value, copies);
        let target_name = get_var_name(self.expr, target);
        if let Some(name) = target_name {
            log::trace!("  cp: invalidate copies for {name}");
            self.invalidate_copies(copies, name);
        }
        let target = self.visit_target(target, copies);
        if let Some(name) = target_name
            && self.is_simple_value_new(value)
        {
            log::trace!("  cp: set copy/const {name} -> {value:?}");
            copies.insert(name, value);
        }
        self.add(Hir::Set([target, value]))
    }

    fn visit_var_decl(
        &mut self,
        [variable, value]: [Id; 2],
        copies: &mut HashMap<Symbol, Id>,
    ) -> Id {
        let value = self.visit(value, copies);
        let new_variable = self.visit_target(variable, copies);
        if let Some(name) = get_var_name(self.expr, variable)
            && self.is_simple_value_new(value)
        {
            log::trace!("  cp: vardecl copy/const {name} -> {value:?}");
            copies.insert(name, value);
        }
        self.add(Hir::VarDecl([new_variable, value]))
    }

    fn visit_update(&mut self, inner: Id, copies: &mut HashMap<Symbol, Id>, increment: bool) -> Id {
        if let Some(name) = get_var_name(self.expr, inner) {
            self.invalidate_copies(copies, name);
        }
        let inner = self.visit_target(inner, copies);
        self.add(if increment {
            Hir::Inc(inner)
        } else {
            Hir::Dec(inner)
        })
    }

    fn invalidate_copies(&self, copies: &mut HashMap<Symbol, Id>, name: Symbol) {
        copies.remove(&name);
        copies.retain(|_, value| {
            self.get_var_name_new(*value)
                .is_none_or(|used| used != name)
        });
    }

    fn visit_block(&mut self, ids: &[Id], copies: &mut HashMap<Symbol, Id>) -> Id {
        let ids = ids
            .iter()
            .map(|id| self.visit(*id, copies))
            .collect::<Vec<_>>();
        self.add(Hir::Block(ids.into_boxed_slice()))
    }

    fn visit_if(
        &mut self,
        [condition, then, otherwise]: [Id; 3],
        copies: &mut HashMap<Symbol, Id>,
    ) -> Id {
        let condition = self.visit(condition, copies);
        let saved = std::mem::take(copies);
        let mut then_copies = saved.clone();
        let then = self.visit(then, &mut then_copies);
        let mut otherwise_copies = saved;
        let otherwise = self.visit(otherwise, &mut otherwise_copies);
        copies.extend(
            then_copies
                .into_iter()
                .filter(|(name, value)| otherwise_copies.get(name) == Some(value)),
        );
        self.add(Hir::If([condition, then, otherwise]))
    }

    fn visit_while(&mut self, [condition, body]: [Id; 2], copies: &mut HashMap<Symbol, Id>) -> Id {
        // The condition is read again on every iteration, after the body has
        // run — so nothing the body writes keeps the value it had before the
        // loop. Folding `a < 10` with the `a = 1` known at the loop's head would
        // freeze the loop into one that never ends.
        for name in written_vars(self.expr, body) {
            copies.remove(&name);
        }
        let condition = self.visit(condition, copies);
        let saved = std::mem::take(copies);
        let body = self.visit(body, copies);
        let restored = saved
            .into_iter()
            .filter(|(name, _)| self.resolve_copy(copies, *name).is_some())
            .collect::<Vec<_>>();
        copies.extend(restored);
        self.add(Hir::While([condition, body]))
    }

    fn visit_function(
        &mut self,
        [name, params, body]: [Id; 3],
        copies: &mut HashMap<Symbol, Id>,
        kind: FunctionKind,
    ) -> Id {
        let name = self.visit(name, copies);
        let params = self.visit(params, copies);
        let saved = std::mem::take(copies);
        let body = self.visit(body, copies);
        *copies = saved;
        self.add(match kind {
            FunctionKind::Function => Hir::FuncDecl([name, params, body]),
            FunctionKind::Process => Hir::ProcDecl([name, params, body]),
        })
    }

    fn visit_event(&mut self, [name, body]: [Id; 2], copies: &mut HashMap<Symbol, Id>) -> Id {
        let name = self.visit(name, copies);
        let saved = std::mem::take(copies);
        let body = self.visit(body, copies);
        *copies = saved;
        self.add(Hir::EventDecl([name, body]))
    }

    fn visit_class(
        &mut self,
        [name, parent, body]: [Id; 3],
        copies: &mut HashMap<Symbol, Id>,
    ) -> Id {
        let name = self.visit(name, copies);
        let parent = self.visit(parent, copies);
        let saved = std::mem::take(copies);
        let body = self.visit(body, copies);
        *copies = saved;
        self.add(Hir::ClassDecl([name, parent, body]))
    }

    fn visit_action(&mut self, ids: &[Id], copies: &mut HashMap<Symbol, Id>) -> Id {
        let targets = target_args(self.expr, ids);
        let written_targets = written_targets(self.expr, ids, &targets);
        // A container's body may write anything, and its condition is read after
        // the body — on every iteration, for `repeat::while`. What the body
        // assigns is therefore unknown at the condition and after the container.
        let body = ids
            .get(4)
            .copied()
            .filter(|&id| !matches!(self.expr[id], Hir::Nop));
        let written = body.map_or_else(HashSet::new, |body| written_vars(self.expr, body));
        let condition = self.condition_index(ids);
        let mut new_ids = Vec::with_capacity(ids.len());
        for (index, &id) in ids.iter().enumerate() {
            match index {
                // The argument list. An argument the schema declares as a
                // `variable` names the variable to act on rather than reads it,
                // so it has to stay a reference.
                3 => new_ids.push(self.visit_action_args(id, copies, &targets)),
                // The body and the lambda are their own copy scopes, and the
                // condition is read after the body.
                4 | 5 => {
                    let saved = std::mem::take(copies);
                    new_ids.push(self.visit(id, copies));
                    *copies = saved;
                }
                index if Some(index) == condition => {
                    for name in &written {
                        copies.remove(name);
                    }
                    new_ids.push(self.visit(id, copies));
                }
                _ => new_ids.push(self.visit(id, copies)),
            }
        }
        for name in written_targets.iter().chain(&written) {
            self.invalidate_copies(copies, *name);
        }
        self.add(Hir::Action(new_ids.into_boxed_slice()))
    }

    /// The child an action keeps its condition in, if it is a container with one.
    ///
    /// It is the last child, and the same slot the code generator reads: an
    /// action with too few children for it has no condition.
    fn condition_index(&self, ids: &[Id]) -> Option<usize> {
        let index = ids.len().checked_sub(1)?;
        if index > 4 && !matches!(self.expr[ids[index]], Hir::Nop) {
            Some(index)
        } else {
            None
        }
    }

    /// Visits an action's argument list, keeping the arguments that name a
    /// variable as variable references.
    ///
    /// Folded into their value, they would stop naming anything:
    /// `variable::exists(a)` would become `variable::exists(1)` and ask about a
    /// variable called `1`. Reading arguments are free to be folded — there the
    /// value is what matters.
    fn visit_action_args(
        &mut self,
        id: Id,
        copies: &mut HashMap<Symbol, Id>,
        targets: &HashSet<Symbol>,
    ) -> Id {
        let node = self.expr[id].clone();
        if let Hir::Named([name, value]) = node {
            let is_target = string_lit(self.expr, name).is_some_and(|n| targets.contains(&n));
            let name = self.visit(name, copies);
            let value = if is_target {
                self.visit_target(value, copies)
            } else {
                self.visit(value, copies)
            };
            return self.add(Hir::Named([name, value]));
        }
        let mut children = node.children().to_vec();
        for child in &mut children {
            *child = self.visit_action_args(*child, copies, targets);
        }
        let mut visited = children.into_iter();
        self.add(node.map_children(|_| visited.next().unwrap()))
    }

    fn visit_children(&mut self, node: Hir, copies: &mut HashMap<Symbol, Id>) -> Id {
        let children = node
            .children()
            .iter()
            .map(|child| self.visit(*child, copies))
            .collect::<Vec<_>>();
        let mut children = children.into_iter();
        self.add(node.map_children(|_| children.next().unwrap()))
    }

    fn build(self) -> RecExpr<Hir> {
        self.new_nodes.into_recexpr()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CopyPropagationPass;

impl FunctionPass<Hir> for CopyPropagationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        log::trace!("=== Copy Propagation Start ===");
        log::trace!("Input expr ({} nodes): {expr:?}", expr.len());

        let mut ctx = CopyPropContext::new(expr);
        let root = expr.root();
        let mut copy_map: HashMap<Symbol, Id> = HashMap::new();
        let _new_root = ctx.visit(root, &mut copy_map);
        let result = ctx.build();

        log::trace!("Output expr ({} nodes): {result:?}", result.len());
        log::trace!("=== Copy Propagation End ===");

        result
    }
}

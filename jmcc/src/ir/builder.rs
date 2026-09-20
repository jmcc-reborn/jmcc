//! Shared helpers for HIR builders: `HirBuilder` (`ir/hir.rs`) and
//! `OverloadExpander` (`ir/hir_expand.rs`).

use std::collections::HashMap;

use egg::{Id, Symbol};

use crate::ast::semantic::{DefId, Type};
use crate::ast::{BinOp, UnOp, VarScope};
use crate::ir::ctx::ClassInfo;
use crate::ir::hir::Hir;
use crate::ir::schema;
use crate::ir::{StrLit, VarName};

/// Allocates a fresh temporary variable `__ht<n>` and increments the counter.
#[must_use]
pub fn fresh_temp(n: &mut usize) -> Symbol {
    let s = format!("__ht{}", *n);
    *n += 1;
    Symbol::from(s)
}

/// Returns the variable placeholder for string interpolation.
#[must_use]
pub fn get_var_placeholder(name: &Symbol, scope: VarScope) -> String {
    match scope {
        VarScope::Game => format!("%var({name})"),
        VarScope::Save => format!("%var_save({name})"),
        VarScope::Local => format!("%var_local({name})"),
        _ => format!("%var_line({name})"),
    }
}

/// Returns the scope for the i-th declaration name: explicit scope or default.
#[must_use]
pub fn decl_scope(scopes: &[Option<VarScope>], i: usize, default: VarScope) -> VarScope {
    scopes.get(i).copied().flatten().unwrap_or(default)
}

/// Expands `$` variable substitutions in a text literal, calling `resolve` for each parsed name.
#[must_use]
pub fn expand_dollar_vars(s: &str, mut resolve: impl FnMut(&str, bool, &mut String)) -> String {
    if !s.contains('$') {
        return s.to_owned();
    }
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            result.push(c);
            continue;
        }
        let is_braced = chars.peek() == Some(&'{');
        if is_braced {
            chars.next();
        }
        let mut var_name = String::new();
        let mut found_closing = false;
        if is_braced {
            for c in chars.by_ref() {
                if c == '}' {
                    found_closing = true;
                    break;
                }
                var_name.push(c);
            }
        } else {
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' {
                    var_name.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            found_closing = true;
        }
        if var_name.is_empty() {
            result.push('$');
            if is_braced && !found_closing {
                result.push('{');
            }
            continue;
        }
        if is_braced && !found_closing {
            result.push_str(&format!("${{{var_name}}}"));
            continue;
        }
        resolve(&var_name, is_braced, &mut result);
    }
    result
}

/// Minimal HIR node emitter interface shared across builders.
pub trait HirEmitter {
    /// Emits a node into the arena and returns its `Id`.
    #[must_use]
    fn emit(&mut self, node: Hir) -> Id;

    /// Returns a reference to the node with the given `Id`.
    #[must_use]
    fn node(&self, id: Id) -> &Hir;

    /// Allocates a fresh temporary variable.
    #[must_use]
    fn new_temp(&mut self) -> Symbol;

    /// Wraps `body` in `let` bindings, in reverse order.
    #[must_use]
    fn wrap_lets(&mut self, body: Id, bindings: Vec<(Id, Id)>) -> Id {
        let mut cur = body;
        for (var, val) in bindings.into_iter().rev() {
            cur = self.emit(Hir::Let([var, val, cur]));
        }
        cur
    }

    /// Returns a single node as-is, or wraps multiple nodes in a block.
    #[must_use]
    fn block_or_single(&mut self, ids: Vec<Id>) -> Id {
        if ids.len() == 1 {
            ids[0]
        } else {
            self.emit(Hir::Block(ids.into_boxed_slice()))
        }
    }

    /// Checks if a node is an atomic value suitable for an argument slot.
    #[must_use]
    fn is_atomic(&self, id: Id) -> bool {
        match self.node(id) {
            Hir::Num(_)
            | Hir::Bool(_)
            | Hir::Str(_)
            | Hir::Var(_)
            | Hir::Action(_)
            | Hir::Text(_)
            | Hir::List(_)
            | Hir::Map(_)
            | Hir::Ctor(_)
            | Hir::Nbt(_) => true,
            Hir::Line(i) | Hir::Game(i) | Hir::Save(i) | Hir::Local(i) => {
                matches!(self.node(*i), Hir::Var(_))
            }
            _ => false,
        }
    }
}

/// Result of converting an AST binary operator to a HIR node.
pub enum BinOpHir {
    /// Regular binary operation node.
    Node(Hir),
    /// `BinOp::Assign` - handled specially by callers.
    Assign,
    /// `BinOp::In` - handled specially by callers.
    In,
    /// `BinOp::Range` - handled specially by callers.
    Range,
    /// `BinOp::RangeInclusive` - handled specially by callers.
    RangeInclusive,
}

/// Maps an AST binary operator to a HIR node representation.
#[must_use]
pub const fn bin_op_to_hir(op: BinOp, left: Id, right: Id) -> BinOpHir {
    match op {
        BinOp::Add => BinOpHir::Node(Hir::Add([left, right])),
        BinOp::Sub => BinOpHir::Node(Hir::Sub([left, right])),
        BinOp::Mul => BinOpHir::Node(Hir::Mul([left, right])),
        BinOp::Div => BinOpHir::Node(Hir::Div([left, right])),
        BinOp::Mod => BinOpHir::Node(Hir::Mod([left, right])),
        BinOp::Pow => BinOpHir::Node(Hir::Pow([left, right])),
        BinOp::Eq => BinOpHir::Node(Hir::Eq([left, right])),
        BinOp::Ne => BinOpHir::Node(Hir::Ne([left, right])),
        BinOp::Lt => BinOpHir::Node(Hir::Lt([left, right])),
        BinOp::Le => BinOpHir::Node(Hir::Le([left, right])),
        BinOp::Gt => BinOpHir::Node(Hir::Gt([left, right])),
        BinOp::Ge => BinOpHir::Node(Hir::Ge([left, right])),
        BinOp::And => BinOpHir::Node(Hir::And([left, right])),
        BinOp::Or => BinOpHir::Node(Hir::Or([left, right])),
        BinOp::BitAnd => BinOpHir::Node(Hir::BitAnd([left, right])),
        BinOp::BitOr => BinOpHir::Node(Hir::BitOr([left, right])),
        BinOp::BitXor => BinOpHir::Node(Hir::BitXor([left, right])),
        BinOp::Shl => BinOpHir::Node(Hir::Shl([left, right])),
        BinOp::Shr => BinOpHir::Node(Hir::Shr([left, right])),
        BinOp::In => BinOpHir::In,
        BinOp::Range => BinOpHir::Range,
        BinOp::RangeInclusive => BinOpHir::RangeInclusive,
        BinOp::Assign => BinOpHir::Assign,
    }
}

/// Maps an AST unary operator to a HIR node representation.
#[must_use]
pub const fn un_op_to_hir(op: UnOp, operand: Id) -> Hir {
    match op {
        UnOp::Not => Hir::Not(operand),
        UnOp::Neg => Hir::Neg(operand),
        UnOp::Inc => Hir::Inc(operand),
        UnOp::Dec => Hir::Dec(operand),
    }
}

/// Destructures a multi-output action value into temporary variables.
///
/// # Panics
///
/// Panics if the node is an `Action` with fewer than 7 children.
#[must_use]
pub fn destructure_action_value<E: HirEmitter>(
    emitter: &mut E,
    val: Id,
    count: usize,
) -> Option<(Vec<Id>, Id)> {
    let Hir::Action(ids) = emitter.node(val) else {
        return None;
    };
    let ids = ids.to_vec();
    let (Hir::Str(obj), Hir::Str(name)) = (emitter.node(ids[0]), emitter.node(ids[1])) else {
        return None;
    };
    let (obj, name) = (obj.0, name.0);
    let def = schema::action_def(obj.as_str(), name.as_str())?;
    let assigns = def.assign?;
    if assigns.len() < 2 || count > assigns.len() {
        return None;
    }

    let mut temps = Vec::with_capacity(assigns.len());
    let mut named = Vec::with_capacity(assigns.len() + 1);
    for assign in assigns {
        let fresh = emitter.new_temp();
        let raw = emitter.emit(Hir::Var(VarName(fresh)));
        let temp = emitter.emit(Hir::Line(raw));
        temps.push(temp);
        let nid = emitter.emit(Hir::Str(StrLit(Symbol::from(assign.id))));
        named.push(emitter.emit(Hir::Named([nid, temp])));
    }
    if let Hir::List(args) = emitter.node(ids[3]) {
        named.extend(args.iter().copied());
    }
    let args = emitter.emit(Hir::List(named.into_boxed_slice()));
    let obj_id = emitter.emit(Hir::Str(StrLit(obj)));
    let name_id = emitter.emit(Hir::Str(StrLit(name)));
    let stmt = emitter.emit(Hir::Action(
        vec![obj_id, name_id, ids[2], args, ids[4], ids[5], ids[6]].into_boxed_slice(),
    ));
    Some((temps, stmt))
}

/// Searches for a value using `f` walking up the class hierarchy.
#[must_use]
pub fn find_in_class_chain<F, R>(ty: &Type, classes: &HashMap<DefId, ClassInfo>, f: F) -> Option<R>
where
    F: Fn(&ClassInfo) -> Option<R>,
{
    let mut current_def = match ty {
        Type::Class(def_id, _) => *def_id,
        _ => return None,
    };
    while let Some(class) = classes.get(&current_def) {
        if let Some(result) = f(class) {
            return Some(result);
        }
        match class.parent {
            Some(parent) if parent != current_def => current_def = parent,
            _ => break,
        }
    }
    None
}

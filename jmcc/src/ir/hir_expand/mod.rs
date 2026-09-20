//! Overload expansion: transforms optimized HIR into HIR with concrete action
//! invocations (`__add__` and other class operator methods).
//!
//! Defines `OverloadExpander`, its `HirEmitter` implementation, traversal driver,
//! and `expand_overloads` entry point; submodules organize the rest by responsibility.

mod calls;
mod exprs;
mod scopes;
mod stmts;
mod text;
mod types;

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use egg::{Id, RecExpr, Symbol};
use tracing::{info, instrument, trace};

use crate::ast::semantic::{DefId, Type};
use crate::ast::*;
use crate::ir::builder::{
    BinOpHir, HirEmitter, bin_op_to_hir, decl_scope, destructure_action_value, expand_dollar_vars,
    find_in_class_chain, fresh_temp, get_var_placeholder, un_op_to_hir,
};
use crate::ir::ctx::IrCtx;
use crate::ir::schema;
use crate::ir::{StrLit, VarName, arena::NodeArena, hir::Hir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverloadError {
    UnsupportedBinaryOp(BinOp),
    InterpInVarName,
    UnsupportedStatement,
    UnsupportedExpr(&'static str),
    InvalidDefaultExpr,
    UnsupportedVarScope(VarScope),
    EnumVariantNotFound { enum_name: String, variant: String },
    PropertyNotFound { property: String, ty: String },
    CallNotFound { name: String },
    RecursiveCall { name: String },
}

impl fmt::Display for OverloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedBinaryOp(op) => write!(f, "unsupported binary operator: {op:?}"),
            Self::InterpInVarName => write!(f, "interpolation in variable name"),
            Self::UnsupportedStatement => write!(f, "unsupported statement kind"),
            Self::UnsupportedExpr(kind) => write!(f, "unsupported expression kind: {kind}"),
            Self::InvalidDefaultExpr => write!(f, "invalid default expression"),
            Self::UnsupportedVarScope(scope) => write!(f, "unsupported variable scope: {scope:?}"),
            Self::EnumVariantNotFound { enum_name, variant } => {
                write!(f, "enum '{enum_name}' has no variant '{variant}'")
            }
            Self::PropertyNotFound { property, ty } => {
                write!(f, "property '{property}' not found on type '{ty}'")
            }
            Self::CallNotFound { name } => {
                write!(f, "call '{name}' is neither an action nor a function")
            }
            Self::RecursiveCall { name } => {
                write!(f, "recursive inline expansion of '{name}'")
            }
        }
    }
}

impl std::error::Error for OverloadError {}

pub type Result<T> = std::result::Result<T, OverloadError>;

macro_rules! map_unary {
    ($self:expr, $a:expr, $ctor:path) => {{
        let a = $self.expand_node($a)?;
        Ok($self.add($ctor(a)))
    }};
}

macro_rules! map_binary {
    ($self:expr, $a:expr, $b:expr, $ctor:path) => {{
        let a = $self.expand_node($a)?;
        let b = $self.expand_node($b)?;
        Ok($self.add($ctor([a, b])))
    }};
}

macro_rules! map_ternary {
    ($self:expr, $a:expr, $b:expr, $c:expr, $ctor:path) => {{
        let a = $self.expand_node($a)?;
        let b = $self.expand_node($b)?;
        let c = $self.expand_node($c)?;
        Ok($self.add($ctor([a, b, c])))
    }};
}

macro_rules! map_slice {
    ($self:expr, $ids:expr, $ctor:path) => {{
        let mut new_ids = Vec::with_capacity($ids.len());
        for id in $ids {
            new_ids.push($self.expand_node(id)?);
        }
        Ok($self.add($ctor(new_ids.into_boxed_slice())))
    }};
}

macro_rules! try_overload {
    ($self:expr, $a:expr, $b:expr, $method:expr, $ctor:path) => {
        $self.try_expand_overload($a, $b, $method, |a, b| $ctor([a, b]))
    };
}

#[derive(Clone)]
enum ExpanderBinding {
    Var { name: Symbol, scope: VarScope },
}

/// Hoisted call arguments: the values for the call plus the `(var, value)` bindings to emit before
/// it, as an argument list holds values and not computations.
type AtomizedArgs = (Vec<Id>, Vec<(Id, Id)>);

pub struct OverloadExpander<'a> {
    ast: &'a Ast,
    ctx: &'a mut IrCtx,
    src: &'a RecExpr<Hir>,
    /// Expression types from semantic analysis, used for property access.
    types: &'a HashMap<ExprId, Type>,
    nodes: NodeArena<Hir>,
    temp_n: usize,
    scopes: Vec<HashMap<Symbol, ExpanderBinding>>,
    inline_vars_stack: Vec<HashMap<Symbol, Id>>,
    inline_return_var: Option<Id>,
    /// Functions currently being expanded; guards against recursion. Cycles can close through
    /// overloads (`f` → `a + b` → `__add__` → `f`), which the inliner cannot see.
    inline_stack: Vec<Symbol>,
    cache: HashMap<Id, Id>,
}

/// Common emitter interface: delegates to inherent `add` / arena / `fresh`.
///
/// Without its own `#[instrument]`: underlying `add` and `fresh` are already instrumented,
/// and an extra nesting layer would needlessly alter trace log structure.
impl HirEmitter for OverloadExpander<'_> {
    fn emit(&mut self, node: Hir) -> Id {
        self.add(node)
    }

    fn node(&self, id: Id) -> &Hir {
        &self.nodes.as_slice()[usize::from(id)]
    }

    fn new_temp(&mut self) -> Symbol {
        self.fresh()
    }
}

fn find_max_ht_index(src: &RecExpr<Hir>) -> usize {
    let mut max_idx = 0;
    for node in src.as_ref() {
        if let Hir::Var(VarName(sym)) = node {
            let s = sym.as_str();
            for part in s.split("__ht") {
                let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(val) = digits.parse::<usize>() {
                    max_idx = max_idx.max(val + 1);
                }
            }
        }
    }
    max_idx
}

impl<'a> OverloadExpander<'a> {
    #[must_use]
    pub fn new(
        ast: &'a Ast,
        types: &'a HashMap<ExprId, Type>,
        ctx: &'a mut IrCtx,
        src: &'a RecExpr<Hir>,
    ) -> Self {
        let temp_n = find_max_ht_index(src);
        Self {
            ast,
            ctx,
            src,
            types,
            nodes: NodeArena::with_capacity(src.len()),
            temp_n,
            scopes: vec![HashMap::new()],
            inline_vars_stack: vec![HashMap::new()],
            inline_return_var: None,
            inline_stack: Vec::new(),
            cache: HashMap::new(),
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn add(&mut self, node: Hir) -> Id {
        self.nodes.add_tagged("overload", node)
    }

    #[instrument(skip(self), level = "trace")]
    fn src_node(&self, id: Id) -> &Hir {
        &self.src[id]
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn fresh(&mut self) -> Symbol {
        fresh_temp(&mut self.temp_n)
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn sym(&self, id: StrId) -> Symbol {
        Symbol::from(self.ast.strings.resolve(&id))
    }

    #[instrument(skip(self, tv), level = "trace")]
    pub(super) fn sym_str_from_text(&self, tv: &TextValue) -> Result<Symbol> {
        let mut name = String::new();
        for part in &tv.parts {
            match part {
                TextPart::Literal(s) => name.push_str(self.ast.strings.resolve(s)),
                TextPart::Interp(_) => return Err(OverloadError::InterpInVarName),
            }
        }
        Ok(Symbol::from(name))
    }

    /// # Errors
    ///
    /// Returns an error if an overloaded expression cannot be expanded into HIR.
    #[instrument(skip(self), level = "info")]
    pub fn expand(mut self) -> Result<RecExpr<Hir>> {
        info!("Expanding operator overloads in optimized HIR");
        let root = self.src.root();
        self.expand_node(root)?;
        Ok(self.nodes.into_recexpr())
    }

    #[instrument(skip(self), level = "debug")]
    fn expand_node(&mut self, id: Id) -> Result<Id> {
        if let Some(&cached) = self.cache.get(&id) {
            trace!(?id, ?cached, "cache hit");
            return Ok(cached);
        }
        let node = self.src_node(id).clone();
        let new_id = self.expand_node_inner(node)?;
        self.cache.insert(id, new_id);
        Ok(new_id)
    }

    #[instrument(skip(self, node), level = "trace")]
    fn expand_node_inner(&mut self, node: Hir) -> Result<Id> {
        match node {
            Hir::Add([a, b]) => try_overload!(self, a, b, "__add__", Hir::Add),
            Hir::Sub([a, b]) => try_overload!(self, a, b, "__subtract__", Hir::Sub),
            Hir::Mul([a, b]) => try_overload!(self, a, b, "__multiply__", Hir::Mul),
            Hir::Div([a, b]) => try_overload!(self, a, b, "__divide__", Hir::Div),
            Hir::Mod([a, b]) => try_overload!(self, a, b, "__remainder__", Hir::Mod),
            Hir::Pow([a, b]) => try_overload!(self, a, b, "__pow__", Hir::Pow),

            Hir::Eq([a, b]) => try_overload!(self, a, b, "__equals__", Hir::Eq),
            Hir::Ne([a, b]) => try_overload!(self, a, b, "__not_equals__", Hir::Ne),
            Hir::Lt([a, b]) => try_overload!(self, a, b, "__less__", Hir::Lt),
            Hir::Le([a, b]) => try_overload!(self, a, b, "__less_or_equals__", Hir::Le),
            Hir::Gt([a, b]) => try_overload!(self, a, b, "__greater__", Hir::Gt),
            Hir::Ge([a, b]) => try_overload!(self, a, b, "__greater_or_equals__", Hir::Ge),

            Hir::BitAnd([a, b]) => try_overload!(self, a, b, "__bitand__", Hir::BitAnd),
            Hir::BitOr([a, b]) => try_overload!(self, a, b, "__bitor__", Hir::BitOr),
            Hir::BitXor([a, b]) => try_overload!(self, a, b, "__bitxor__", Hir::BitXor),
            Hir::Shl([a, b]) => try_overload!(self, a, b, "__lshift__", Hir::Shl),
            Hir::Shr([a, b]) => try_overload!(self, a, b, "__rshift__", Hir::Shr),

            Hir::And([a, b]) => try_overload!(self, a, b, "__and__", Hir::And),
            Hir::Or([a, b]) => try_overload!(self, a, b, "__or__", Hir::Or),

            Hir::Not(a) => map_unary!(self, a, Hir::Not),
            Hir::Neg(a) => map_unary!(self, a, Hir::Neg),
            Hir::Inc(a) => map_unary!(self, a, Hir::Inc),
            Hir::Dec(a) => map_unary!(self, a, Hir::Dec),

            Hir::Num(n) => Ok(self.add(Hir::Num(n))),
            Hir::Bool(b) => Ok(self.add(Hir::Bool(b))),
            Hir::Str(s) => Ok(self.add(Hir::Str(s))),
            Hir::Var(v) => Ok(self.add(Hir::Var(v))),
            Hir::Nop => Ok(self.add(Hir::Nop)),
            Hir::Break => Ok(self.add(Hir::Break)),

            Hir::Local(i) => map_unary!(self, i, Hir::Local),
            Hir::Game(i) => map_unary!(self, i, Hir::Game),
            Hir::Save(i) => map_unary!(self, i, Hir::Save),
            Hir::Line(i) => map_unary!(self, i, Hir::Line),

            Hir::If([c, t, e]) => map_ternary!(self, c, t, e, Hir::If),
            Hir::While([c, b]) => map_binary!(self, c, b, Hir::While),
            Hir::Return(v) => map_unary!(self, v, Hir::Return),

            Hir::Let([v, val, body]) => map_ternary!(self, v, val, body, Hir::Let),
            Hir::Set([t, v]) => map_binary!(self, t, v, Hir::Set),
            Hir::VarDecl([n, v]) => map_binary!(self, n, v, Hir::VarDecl),

            Hir::Block(ids) => map_slice!(self, ids, Hir::Block),
            Hir::List(ids) => map_slice!(self, ids, Hir::List),
            Hir::Map(ids) => map_slice!(self, ids, Hir::Map),
            Hir::Concat(ids) => map_slice!(self, ids, Hir::Concat),

            Hir::Named([n, v]) => map_binary!(self, n, v, Hir::Named),
            Hir::Enum(v) => map_unary!(self, v, Hir::Enum),
            Hir::Text([t, c]) => map_binary!(self, t, c, Hir::Text),
            Hir::Nbt(c) => map_unary!(self, c, Hir::Nbt),
            Hir::Sel(s) => map_unary!(self, s, Hir::Sel),

            Hir::Index([o, i]) => map_binary!(self, o, i, Hir::Index),
            Hir::Slice([o, s, e]) => map_ternary!(self, o, s, e, Hir::Slice),

            Hir::Action(ids) => map_slice!(self, ids, Hir::Action),
            Hir::Ctor(ids) => map_slice!(self, ids, Hir::Ctor),

            Hir::FuncDecl([n, p, b]) => map_ternary!(self, n, p, b, Hir::FuncDecl),
            Hir::ProcDecl([n, p, b]) => map_ternary!(self, n, p, b, Hir::ProcDecl),
            Hir::EventDecl([n, b]) => map_binary!(self, n, b, Hir::EventDecl),
            Hir::ClassDecl([n, p, b]) => map_ternary!(self, n, p, b, Hir::ClassDecl),
            Hir::EnumDecl([n, v]) => map_binary!(self, n, v, Hir::EnumDecl),

            Hir::FuncCall([t, a]) => map_binary!(self, t, a, Hir::FuncCall),
            Hir::ProcCall([t, a]) => map_binary!(self, t, a, Hir::ProcCall),
        }
    }

    #[instrument(skip(self, make_node), level = "trace")]
    fn try_expand_overload(
        &mut self,
        a: Id,
        b: Id,
        method: &str,
        make_node: impl Fn(Id, Id) -> Hir,
    ) -> Result<Id> {
        let a_new = self.expand_node(a)?;
        let b_new = self.expand_node(b)?;

        if let Some(f) = self.needs_overload(a_new, method) {
            trace!(method, "expanding overload via inline function");
            return self.expand_inline_from_hir(&f, vec![a_new, b_new]);
        }

        Ok(self.add(make_node(a_new, b_new)))
    }
}

/// # Errors
///
/// Returns an error if an overloaded expression cannot be expanded into HIR.
#[instrument(skip_all, level = "info")]
pub fn expand_overloads(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    ctx: &mut IrCtx,
    hir: &RecExpr<Hir>,
) -> Result<RecExpr<Hir>> {
    info!("Building OverloadExpander and running");
    let expander = OverloadExpander::new(ast, types, ctx, hir);
    expander.expand()
}

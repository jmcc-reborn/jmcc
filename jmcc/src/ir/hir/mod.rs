//! HIR: the `Hir` language and building `RecExpr<Hir>` from AST (`ast_to_hir`).
//!
//! Contains core language declarations: `define_language!`, `IrError`,
//! `HirBuilder` with `build`, and shared emission primitives. Submodules
//! implement specific concerns: `scopes` (name bindings), `decls` (declarations),
//! `stmts` (statements), `exprs` (expressions), `props` (properties and indexing),
//! `calls` (invocations), `actions` (platform actions and selectors), and `text` (strings).

use crate::ir::{StrLit, VarName};
use egg::{Id, RecExpr, Symbol, define_language};
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tracing::{info, instrument, trace, warn};

use crate::ast::semantic::{DefId, Type};
use crate::ast::*;
use crate::ir::KNOWN_OBJECTS;
use crate::ir::arena::NodeArena;
use crate::ir::builder::{
    BinOpHir, HirEmitter, bin_op_to_hir, decl_scope, destructure_action_value, expand_dollar_vars,
    find_in_class_chain, fresh_temp, get_var_placeholder, un_op_to_hir,
};
use crate::ir::ctx::IrCtx;
use crate::ir::schema;
use std::rc::Rc;

mod actions;
mod calls;
mod decls;
mod exprs;
mod props;
mod scopes;
mod stmts;
mod text;

define_language! {
    pub enum Hir {
        Num(OrderedFloat<f64>), Bool(bool), Str(StrLit), Var(VarName),
        "let" = Let([Id; 3]),
        "+" = Add([Id; 2]), "-" = Sub([Id; 2]), "*" = Mul([Id; 2]), "/" = Div([Id; 2]), "%" = Mod([Id; 2]), "**" = Pow([Id; 2]),
        "==" = Eq([Id; 2]), "!=" = Ne([Id; 2]), "<" = Lt([Id; 2]), "<=" = Le([Id; 2]), ">" = Gt([Id; 2]), ">=" = Ge([Id; 2]),
        "&" = BitAnd([Id; 2]), "|" = BitOr([Id; 2]), "^" = BitXor([Id; 2]), "<<" = Shl([Id; 2]), ">>" = Shr([Id; 2]),
        "&&" = And([Id; 2]), "||" = Or([Id; 2]), "!" = Not(Id), "neg" = Neg(Id), "++" = Inc(Id), "--" = Dec(Id),
        "if" = If([Id; 3]), "while" = While([Id; 2]), "break" = Break, "return" = Return(Id),
        "list" = List(Box<[Id]>), "map" = Map(Box<[Id]>), "concat" = Concat(Box<[Id]>),
        "index" = Index([Id; 2]), "slice" = Slice([Id; 3]), "text" = Text([Id; 2]), "nbt" = Nbt(Id),
        "ctor" = Ctor(Box<[Id]>), "sel" = Sel(Id), "enum" = Enum(Id), "action" = Action(Box<[Id]>),
        "funcdecl" = FuncDecl([Id; 3]), "procdecl" = ProcDecl([Id; 3]), "eventdecl" = EventDecl([Id; 2]),
        "classdecl" = ClassDecl([Id; 3]), "enumdecl" = EnumDecl([Id; 2]),
        "funccall" = FuncCall([Id; 2]), "proccall" = ProcCall([Id; 2]),
        "named" = Named([Id; 2]), "set" = Set([Id; 2]), "vardecl" = VarDecl([Id; 2]),
        "block" = Block(Box<[Id]>), "nop" = Nop,
        "local" = Local(Id), "game" = Game(Id), "save" = Save(Id), "line" = Line(Id),
    }
}

#[derive(Debug, Error)]
pub enum IrError {
    #[error("invalid assignment operator: {0}")]
    InvalidAssignOp(String),
    #[error("only string literals and variables can be used in variable names")]
    InvalidVarNameExpr,
    #[error("undeclared variable: {0}")]
    UndeclaredVariable(String),
    #[error("unsupported 'in' operator")]
    UnsupportedIn,
    #[error("property '{property}' not found on type '{ty}'")]
    PropertyNotFound { property: String, ty: String },
    #[error("slice assignment not supported for type '{0}'")]
    SliceAssignNotSupported(String),
    #[error("subscript assignment not supported for type '{0}'")]
    SubscriptAssignNotSupported(String),
    #[error("unknown selector '{selector}' for object '{object}'")]
    UnknownSelector { object: String, selector: String },
    #[error("too many arguments for action '{object}::{name}'")]
    TooManyArguments { object: String, name: String },
    #[error("enum variant '{variant}' not found in enum '{enum_name}'")]
    EnumVariantNotFound { enum_name: String, variant: String },
    #[error("unknown type: {0}")]
    UnknownType(String),
    #[error("enum index {index} out of bounds (have {max} variants)")]
    EnumIndexOutOfBounds { index: usize, max: usize },
}

const fn positional(value: ExprId) -> ArgExpr {
    ArgExpr {
        name: None,
        value,
        spread: 0,
        is_ref: false,
    }
}

#[derive(Clone)]
enum ClassMember {
    Method(Rc<FunctionDecl>),
    Process(Rc<ProcessDecl>),
    Getter(Rc<FunctionDecl>),
    Setter(Rc<FunctionDecl>),
}

#[derive(Clone)]
enum FieldAccess {
    Slot(usize),
    Dict,
}

enum Binding {
    Var { name: Symbol, scope: VarScope },
    Func { name: Symbol },
    Proc { name: Symbol },
}

#[derive(Debug, Clone)]
pub(super) struct LoopCtx {
    pub label: Option<Symbol>,
    pub break_flag: Option<Symbol>,
    pub outer_flags_checked: Vec<Symbol>,
}

pub struct HirBuilder<'a> {
    ast: &'a Ast,
    types: &'a HashMap<ExprId, Type>,
    ir_ctx: &'a mut IrCtx,
    scopes: Vec<HashMap<Symbol, Binding>>,
    inline_vars_stack: Vec<HashMap<Symbol, Id>>,
    inline_return_var: Option<Id>,
    temp_n: usize,
    nodes: NodeArena<Hir>,
    is_statement: bool,
    default_scope: VarScope,
    loop_stack: Vec<LoopCtx>,
}

/// Common emitter interface: delegates to inherent `add` / `get` / `fresh`.
impl HirEmitter for HirBuilder<'_> {
    #[inline(always)]
    fn emit(&mut self, node: Hir) -> Id {
        self.add(node)
    }

    #[inline(always)]
    fn node(&self, id: Id) -> &Hir {
        self.get(id)
    }

    #[inline(always)]
    fn new_temp(&mut self) -> Symbol {
        self.fresh()
    }
}

impl<'a> HirBuilder<'a> {
    #[instrument(skip(ast, types, ir_ctx), level = "trace")]
    pub fn new(
        ast: &'a Ast,
        types: &'a HashMap<ExprId, Type>,
        ir_ctx: &'a mut IrCtx,
        edition: u16,
    ) -> Self {
        Self {
            ast,
            types,
            ir_ctx,
            scopes: vec![HashMap::new()],
            inline_vars_stack: vec![HashMap::new()],
            inline_return_var: None,
            temp_n: 0,
            nodes: NodeArena::with_capacity(ast.statements.len()),
            is_statement: false,
            default_scope: if edition < 2026 {
                VarScope::Local
            } else {
                VarScope::Line
            },
            loop_stack: Vec::new(),
        }
    }

    #[inline(always)]
    fn add(&mut self, node: Hir) -> Id {
        self.nodes.add_tagged("hir", node)
    }

    #[inline(always)]
    fn get(&self, id: Id) -> &Hir {
        &self.nodes.as_slice()[usize::from(id)]
    }

    #[inline(always)]
    fn nop(&mut self) -> Id {
        self.add(Hir::Nop)
    }

    fn str_lit(&mut self, s: impl Into<Symbol>) -> Id {
        self.add(Hir::Str(StrLit(s.into())))
    }

    /// # Errors
    ///
    /// Returns an error if an AST node cannot be represented in HIR.
    #[instrument(skip(self), level = "trace")]
    pub fn build(mut self) -> Result<RecExpr<Hir>, IrError> {
        info!("Building HIR…");

        for stmt in &self.ast.statements {
            self.declare_callable(stmt);
            if let Statement::Class(c) = stmt {
                for s in &c.body {
                    self.declare_callable(s);
                }
            }
        }

        self.convert_block_inner(&self.ast.statements)?;
        info!(nodes = self.nodes.len(), "HIR built");
        Ok(self.nodes.into_recexpr())
    }
}

/// # Errors
///
/// Returns an error if an AST node cannot be represented in HIR.
#[instrument(skip(ast, types, ir_ctx), level = "info")]
pub fn ast_to_hir(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    ir_ctx: &mut IrCtx,
    edition: u16,
) -> Result<RecExpr<Hir>, IrError> {
    info!("Starting AST to HIR conversion");
    let builder = HirBuilder::new(ast, types, ir_ctx, edition);
    builder.build()
}

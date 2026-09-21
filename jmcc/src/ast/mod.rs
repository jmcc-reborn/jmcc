use id_arena::Id;
use lasso::Spur;
use line_index::LineIndex;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

pub mod ast_pretty;
pub mod format;
pub mod import;
pub mod lambda_lift;
pub mod lexer;
pub mod parser;
pub mod semantic;

pub use import::{
    ImportResolver, parse_file, parse_file_with_full_options, parse_file_with_options,
    parse_file_with_overlays, parse_file_with_packages,
};
pub use lambda_lift::lift_lambdas;
pub use semantic::{analyze, analyze_for_diagnostics};

pub type ExprId = Id<Expr>;
pub type StrId = Spur;
pub type Span = Range<usize>;

pub type DefId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Class(DefId, Vec<Type>),
    Enum(DefId),
    InferVar(usize),
    Never,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextParsing {
    Plain,
    Legacy,
    MiniMessage,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPart {
    Literal(StrId),
    Interp(ExprId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextValue {
    pub parts: Vec<TextPart>,
    pub parsing: TextParsing,
    pub span: Span,
}

/// Placeholder used to replace interpolation in a name written as a text value.
pub const INTERP_PLACEHOLDER: &str = "${...}";

/// Converts a text value into a string representation for symbol resolution,
/// replacing each interpolation part with [`INTERP_PLACEHOLDER`].
#[must_use]
pub fn text_value_to_string(ast: &Ast, tv: &TextValue) -> String {
    tv.parts
        .iter()
        .map(|p| match p {
            TextPart::Literal(l) => ast.strings.resolve(l).to_owned(),
            TextPart::Interp(_) => INTERP_PLACEHOLDER.to_owned(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarScope {
    Inline,
    Local,
    Game,
    Save,
    Line,
    Jmcc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NumberExpr {
    pub value: f64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoolExpr {
    pub value: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableExpr {
    pub name: TextValue,
    pub scope: VarScope,
    pub value_type: Option<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbtExpr {
    pub raw: StrId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListExpr {
    pub values: Vec<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapExpr {
    pub keys: Vec<ExprId>,
    pub values: Vec<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TernaryExpr {
    pub cond: ExprId,
    pub then_val: ExprId,
    pub else_val: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    In,
    Range,
    RangeInclusive,
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
    Ne,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Pow,
    Assign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryExpr {
    pub op: BinOp,
    pub left: ExprId,
    pub right: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Inc,
    Dec,
    Neg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaryExpr {
    pub op: UnOp,
    pub operand: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyExpr {
    pub object: ExprId,
    pub property: StrId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptExpr {
    pub object: ExprId,
    pub index: ExprId,
    pub end: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgExpr {
    pub name: Option<StrId>,
    pub value: ExprId,
    pub spread: u8,
    pub is_ref: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallExpr {
    pub target: ExprId,
    pub method: StrId,
    pub args: Vec<ArgExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionExpr {
    pub object: StrId,
    pub name: StrId,
    pub args: Vec<ArgExpr>,
    pub operations: Option<Vec<Statement>>,
    pub lambda: Option<Vec<ExprId>>,
    pub selector: Option<StrId>,
    pub invert: Option<bool>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructorExpr {
    pub name: StrId,
    pub args: Vec<ArgExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastExpr {
    pub expr: ExprId,
    pub ty: StrId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expr(ExprId),
    Block(Vec<Statement>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaExpr {
    pub params: Vec<Param>,
    pub return_type: Option<StrId>,
    pub body: LambdaBody,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(NumberExpr),
    Bool(BoolExpr),
    Ident(StrId, Span),
    Text(TextValue),
    Variable(VariableExpr),
    Nbt(NbtExpr),
    List(ListExpr),
    Map(MapExpr),
    Ternary(TernaryExpr),
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Property(PropertyExpr),
    Subscript(SubscriptExpr),
    Call(CallExpr),
    Action(ActionExpr),
    Constructor(ConstructorExpr),
    Cast(CastExpr),
    Match(MatchExpr),
    Lambda(LambdaExpr),
}

impl Expr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Expr::Number(e) => e.span.clone(),
            Expr::Bool(e) => e.span.clone(),
            Expr::Ident(_, s) => s.clone(),
            Expr::Text(e) => e.span.clone(),
            Expr::Variable(e) => e.span.clone(),
            Expr::Nbt(e) => e.span.clone(),
            Expr::List(e) => e.span.clone(),
            Expr::Map(e) => e.span.clone(),
            Expr::Ternary(e) => e.span.clone(),
            Expr::Binary(e) => e.span.clone(),
            Expr::Unary(e) => e.span.clone(),
            Expr::Property(e) => e.span.clone(),
            Expr::Subscript(e) => e.span.clone(),
            Expr::Call(e) => e.span.clone(),
            Expr::Action(e) => e.span.clone(),
            Expr::Constructor(e) => e.span.clone(),
            Expr::Cast(e) => e.span.clone(),
            Expr::Match(e) => e.span.clone(),
            Expr::Lambda(e) => e.span.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    SideEffect,
    Default(StrId),
    Named(Vec<ImportItem>),
    Namespace(StrId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItem {
    pub original: StrId,
    pub local: StrId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStmt {
    pub path: StrId,
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: StrId,
    pub ty: Option<StrId>,
    pub default: Option<ExprId>,
    pub is_ref: bool,
    pub spread: u8,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestAttribute {
    pub is_test: bool,
    pub should_panic: bool,
    pub expected_panic: Option<String>,
    pub is_ignore: bool,
    pub ignore_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: StrId,
    pub generics: Vec<StrId>,
    pub params: Vec<Param>,
    pub return_type: Option<StrId>,
    pub body: Vec<Statement>,
    pub is_inline: bool,
    pub is_exported: bool,
    pub is_getter: bool,
    pub is_setter: bool,
    pub is_overload: bool,
    pub aliases: Vec<StrId>,
    pub test_attr: Option<TestAttribute>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessDecl {
    pub name: StrId,
    pub params: Vec<Param>,
    pub body: Vec<Statement>,
    pub is_exported: bool,
    pub aliases: Vec<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventDecl {
    pub event_name: StrId,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: StrId,
    pub generics: Vec<StrId>,
    pub parent: Option<StrId>,
    pub implements: Vec<StrId>,
    pub body: Vec<Statement>,
    pub is_inline: bool,
    pub lang_item: bool,
    pub is_dict: bool,
    pub is_exported: bool,
    pub aliases: Vec<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub name: StrId,
    pub generics: Vec<StrId>,
    pub parents: Vec<StrId>,
    pub body: Vec<Statement>,
    pub is_exported: bool,
    pub aliases: Vec<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: StrId,
    pub values: Vec<StrId>,
    pub span: Span,
    pub is_exported: bool,
    pub aliases: Vec<StrId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAliasDecl {
    pub name: StrId,
    pub generics: Vec<StrId>,
    pub target_ty: StrId,
    pub span: Span,
    pub is_exported: bool,
    pub aliases: Vec<StrId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    pub condition: ExprId,
    pub then_body: Vec<Statement>,
    pub elif_branches: Vec<(ExprId, Vec<Statement>)>,
    pub elif_spans: Vec<Span>,
    pub else_body: Option<Vec<Statement>>,
    pub is_not: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub scopes: Vec<Option<VarScope>>,
    pub names: Vec<TextValue>,
    pub tys: Vec<Option<StrId>>,
    pub value: Option<ExprId>,
    pub is_exported: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignStmt {
    pub targets: Vec<ExprId>,
    pub op: Option<StrId>,
    pub value: ExprId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt {
    pub value: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub patterns: Vec<ExprId>,
    pub guard: Option<ExprId>,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub expr: ExprId,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    pub expr: ExprId,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryCatchStmt {
    pub try_body: Vec<Statement>,
    pub catch_var: Option<TextValue>,
    pub catch_type: Option<StrId>,
    pub catch_body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrowStmt {
    pub value: Option<ExprId>,
    pub exception_type: Option<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileStmt {
    pub label: Option<StrId>,
    pub condition: ExprId,
    pub body: Vec<Statement>,
    pub is_not: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForStmt {
    pub label: Option<StrId>,
    pub vars: Vec<TextValue>,
    pub scopes: Vec<Option<VarScope>>,
    pub iterable: ExprId,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakStmt {
    pub label: Option<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueStmt {
    pub label: Option<StrId>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Import(ImportStmt),
    Function(FunctionDecl),
    Process(ProcessDecl),
    Event(EventDecl),
    Class(ClassDecl),
    Enum(EnumDecl),
    TypeAlias(TypeAliasDecl),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    VarDecl(VarDecl),
    Assign(AssignStmt),
    Return(ReturnStmt),
    Expr(ExprId),
    Match(MatchStmt),
    TryCatch(TryCatchStmt),
    Throw(ThrowStmt),
    Interface(InterfaceDecl),
}

#[derive(Debug, Clone)]
pub struct Ast {
    pub exprs: id_arena::Arena<Expr>,
    pub strings: lasso::Rodeo,
    pub statements: Vec<Statement>,
    pub sources: HashMap<PathBuf, String>,
    /// Precomputed line start index for each file in [`Ast::sources`], enabling O(log n) span lookup.
    pub line_indexes: HashMap<PathBuf, LineIndex>,
    pub file_offsets: Vec<(PathBuf, usize, usize)>,
}

/// Structural equality of expressions, ignoring source positions.
///
/// Distinguishes a duplicated receiver (`x.method(x, y)`) from a real argument: exported
/// `JustMC` projects often re-emit the receiver as a separate node with identical contents.
#[must_use]
pub fn structurally_eq(ast: &Ast, a: ExprId, b: ExprId) -> bool {
    if a == b {
        return true;
    }
    match (&ast.exprs[a], &ast.exprs[b]) {
        (Expr::Number(x), Expr::Number(y)) => x.value == y.value,
        (Expr::Bool(x), Expr::Bool(y)) => x.value == y.value,
        (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
        (Expr::Nbt(x), Expr::Nbt(y)) => x.raw == y.raw,
        (Expr::Text(x), Expr::Text(y)) => {
            x.parsing == y.parsing && same_parts(ast, &x.parts, &y.parts)
        }
        (Expr::Variable(x), Expr::Variable(y)) => {
            x.scope == y.scope
                && x.name.parsing == y.name.parsing
                && same_parts(ast, &x.name.parts, &y.name.parts)
        }
        (Expr::List(x), Expr::List(y)) => same_exprs(ast, &x.values, &y.values),
        (Expr::Map(x), Expr::Map(y)) => {
            same_exprs(ast, &x.keys, &y.keys) && same_exprs(ast, &x.values, &y.values)
        }
        (Expr::Ternary(x), Expr::Ternary(y)) => {
            structurally_eq(ast, x.cond, y.cond)
                && structurally_eq(ast, x.then_val, y.then_val)
                && structurally_eq(ast, x.else_val, y.else_val)
        }
        (Expr::Binary(x), Expr::Binary(y)) => {
            x.op == y.op
                && structurally_eq(ast, x.left, y.left)
                && structurally_eq(ast, x.right, y.right)
        }
        (Expr::Unary(x), Expr::Unary(y)) => {
            x.op == y.op && structurally_eq(ast, x.operand, y.operand)
        }
        (Expr::Property(x), Expr::Property(y)) => {
            x.property == y.property && structurally_eq(ast, x.object, y.object)
        }
        (Expr::Subscript(x), Expr::Subscript(y)) => {
            structurally_eq(ast, x.object, y.object)
                && structurally_eq(ast, x.index, y.index)
                && match (x.end, y.end) {
                    (Some(p), Some(q)) => structurally_eq(ast, p, q),
                    (None, None) => true,
                    _ => false,
                }
        }
        (Expr::Call(x), Expr::Call(y)) => {
            x.method == y.method
                && structurally_eq(ast, x.target, y.target)
                && same_args(ast, &x.args, &y.args)
        }
        (Expr::Constructor(x), Expr::Constructor(y)) => {
            x.name == y.name && same_args(ast, &x.args, &y.args)
        }
        (Expr::Cast(x), Expr::Cast(y)) => x.ty == y.ty && structurally_eq(ast, x.expr, y.expr),
        (Expr::Action(x), Expr::Action(y)) => {
            x.object == y.object
                && x.name == y.name
                && x.selector == y.selector
                && x.invert == y.invert
                && same_args(ast, &x.args, &y.args)
        }
        (Expr::Match(x), Expr::Match(y)) => {
            structurally_eq(ast, x.expr, y.expr)
                && x.arms.len() == y.arms.len()
                && x.arms.iter().zip(&y.arms).all(|(a, b)| {
                    same_exprs(ast, &a.patterns, &b.patterns)
                        && match (a.guard, b.guard) {
                            (Some(g1), Some(g2)) => structurally_eq(ast, g1, g2),
                            (None, None) => true,
                            _ => false,
                        }
                        && a.body.len() == b.body.len()
                })
        }
        (Expr::Lambda(x), Expr::Lambda(y)) => {
            x.params == y.params && x.return_type == y.return_type && x.body == y.body
        }
        _ => false,
    }
}

fn same_parts(ast: &Ast, a: &[TextPart], b: &[TextPart]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (x, y) {
            (TextPart::Literal(p), TextPart::Literal(q)) => p == q,
            (TextPart::Interp(p), TextPart::Interp(q)) => structurally_eq(ast, *p, *q),
            _ => false,
        })
}

fn same_exprs(ast: &Ast, a: &[ExprId], b: &[ExprId]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| structurally_eq(ast, x, y))
}

fn same_args(ast: &Ast, a: &[ArgExpr], b: &[ArgExpr]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.name == y.name && x.spread == y.spread && structurally_eq(ast, x.value, y.value)
        })
}

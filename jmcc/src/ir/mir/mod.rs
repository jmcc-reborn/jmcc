//! Lowering HIR into MIR: `Mir` language, errors, and shared entry point.
//!
//! Defines the `Mir` language enum, `MirError`, `MirLowerer` struct with
//! shared arena nodes (`add`, `fresh_temp`, `nop`, `str_id`, `named_arg`),
//! and the `lower_to_mir` free function. Submodules handle specific areas:
//! `stmts` for statements and control flow, `exprs` for expressions,
//! `calls` for function/process invocations, `action` for platform actions,
//! and `vars` for variables and scopes.

use std::collections::{HashMap, HashSet};

use egg::{Id, Language as _, RecExpr, Symbol, define_language};
use ordered_float::OrderedFloat;
use thiserror::Error;
use tracing::{info, instrument, trace};

use crate::ir::ctx::IrCtx;
use crate::ir::schema;
use crate::ir::{StrLit, VarName, arena::NodeArena, hir::Hir};
use crate::utils::resolve_action_arg_def;

define_language! {
    pub enum Mir {
        Num(OrderedFloat<f64>), Bool(bool), Str(StrLit), Var(VarName),

        "let" = Let([Id; 3]),
        "local" = Local(Id), "game" = Game(Id), "save" = Save(Id), "line" = Line(Id),

        "action" = Action(Box<[Id]>),
        "inv_action" = InvertedAction(Box<[Id]>),
        "ctor" = Ctor(Box<[Id]>),
        "gamevalue" = GameValue([Id; 2]),
        "sel" = Sel(Id),
        "nbt" = Nbt(Id), "text" = Text([Id; 2]),

        "list" = List(Box<[Id]>), "map" = Map(Box<[Id]>), "concat" = Concat(Box<[Id]>),
        "named" = Named([Id; 2]), "enum" = Enum(Id),

        "!" = Not(Id),
        "break" = Break,
        "continue" = Continue,
        "return_func" = ReturnFunc(Id),

        "set" = Set([Id; 2]),
        "vardecl" = VarDecl([Id; 2]),

        "funcdecl" = FuncDecl([Id; 3]),
        "procdecl" = ProcDecl([Id; 3]),
        "eventdecl" = EventDecl([Id; 2]),

        "block" = Block(Box<[Id]>), "nop" = Nop,
    }
}

#[derive(Debug, Error)]
pub enum MirError {
    #[error("expected string literal, got {found}")]
    InvalidStringLiteral { found: String },
    #[error("function declaration not found: '{0}'")]
    FunctionNotFound(String),
    #[error("invalid function call target")]
    InvalidFuncCallTarget,
    #[error("unresolved positional argument {positional_idx} for action '{action}'")]
    UnresolvedArgument {
        action: String,
        positional_idx: usize,
    },
    #[error("slice assignment is not natively supported in DiamondFire")]
    SliceAssignmentNotSupported,
    #[error("cannot pass non-variable to reference parameter '{0}'")]
    NonVariablePassedToRef(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ScopeKind {
    Local,
    Game,
    Save,
    Line,
}

enum FunctionKind {
    Function,
    Process,
}

fn extract_str_hir(expr: &RecExpr<Hir>, id: Id) -> Result<String, MirError> {
    match &expr[id] {
        Hir::Str(s) => Ok(s.0.to_string()),
        other => Err(MirError::InvalidStringLiteral {
            found: format!("{other:?}"),
        }),
    }
}

fn is_bool_action(hir: &RecExpr<Hir>, ids: &[Id]) -> Result<bool, MirError> {
    let obj = extract_str_hir(hir, ids[0])?;
    let name = extract_str_hir(hir, ids[1])?;
    Ok(schema::is_boolean_action(&obj, &name))
}

pub struct MirLowerer<'a> {
    hir: &'a RecExpr<Hir>,
    #[expect(dead_code)]
    ctx: &'a IrCtx,
    nodes: NodeArena<Mir>,
    temp_n: usize,
    ref_forward_temp_n: usize,
    edition: u16,
    var_subst: HashMap<Id, Id>,
    named_var_subst: HashMap<(ScopeKind, Symbol), Id>,
    current_ref_params: HashSet<String>,
}

impl<'a> MirLowerer<'a> {
    #[must_use]
    pub fn new(hir: &'a RecExpr<Hir>, ctx: &'a IrCtx, edition: u16) -> Self {
        Self {
            hir,
            ctx,
            nodes: NodeArena::with_capacity(hir.len()),
            temp_n: 0,
            ref_forward_temp_n: 0,
            edition,
            var_subst: HashMap::new(),
            named_var_subst: HashMap::new(),
            current_ref_params: HashSet::new(),
        }
    }

    pub(super) fn extract_var_key(&self, id: Id) -> Option<(ScopeKind, Symbol)> {
        match &self.hir[id] {
            Hir::Line(inner) => self
                .extract_var_symbol(*inner)
                .map(|sym| (ScopeKind::Line, sym)),
            Hir::Local(inner) => self
                .extract_var_symbol(*inner)
                .map(|sym| (ScopeKind::Local, sym)),
            Hir::Game(inner) => self
                .extract_var_symbol(*inner)
                .map(|sym| (ScopeKind::Game, sym)),
            Hir::Save(inner) => self
                .extract_var_symbol(*inner)
                .map(|sym| (ScopeKind::Save, sym)),
            Hir::Var(name) => {
                let default_scope = if self.edition >= 2026 {
                    ScopeKind::Line
                } else {
                    ScopeKind::Local
                };
                Some((default_scope, name.0))
            }
            _ => None,
        }
    }

    fn extract_var_symbol(&self, id: Id) -> Option<Symbol> {
        match &self.hir[id] {
            Hir::Var(name) => Some(name.0),
            _ => None,
        }
    }

    pub(super) fn is_atomic_hir(&self, id: Id) -> bool {
        match &self.hir[id] {
            Hir::Num(_) | Hir::Bool(_) | Hir::Str(_) | Hir::Var(_) => true,
            Hir::Text([_, content]) => matches!(&self.hir[*content], Hir::Str(_)),
            Hir::Enum(val) => matches!(&self.hir[*val], Hir::Str(_)),
            Hir::Line(inner) | Hir::Local(inner) | Hir::Game(inner) | Hir::Save(inner) => {
                matches!(&self.hir[*inner], Hir::Var(_))
            }
            _ => false,
        }
    }

    #[instrument(skip(self, node), level = "trace")]
    fn add(&mut self, node: Mir) -> Id {
        self.nodes.add_tagged("mir", node)
    }

    #[instrument(skip(self), level = "trace")]
    fn fresh_temp(&mut self) -> Id {
        let name = format!("__mt{}", self.temp_n);
        self.temp_n += 1;
        let var_id = self.add(Mir::Var(VarName(Symbol::from(name))));
        if self.edition >= 2026 {
            self.add(Mir::Line(var_id))
        } else {
            self.add(Mir::Local(var_id))
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn fresh_ref_forward_temp(&mut self) -> Id {
        let name = format!("__rf_{}", self.ref_forward_temp_n);
        self.ref_forward_temp_n += 1;
        let var_id = self.add(Mir::Var(VarName(Symbol::from(name))));
        if self.edition >= 2026 {
            self.add(Mir::Line(var_id))
        } else {
            self.add(Mir::Local(var_id))
        }
    }

    pub(super) fn extract_var_name_mir(&self, id: Id) -> Option<String> {
        match &self.nodes.as_slice()[usize::from(id)] {
            Mir::Var(v) => Some(v.0.to_string()),
            Mir::Line(i) | Mir::Local(i) | Mir::Game(i) | Mir::Save(i) => {
                self.extract_var_name_mir(*i)
            }
            _ => None,
        }
    }

    fn emit_nested_ref_warning(var_name: &str, param_name: &str, func_name: &str) {
        tracing::warn!(
            var_name,
            param_name,
            func_name,
            "forwarding reference parameter to another reference parameter; applying compiler soft fix (copy-in/copy-out) for JustMC compatibility"
        );
        #[expect(clippy::print_stderr, reason = "compiler warning output to stderr")]
        {
            match crate::i18n::current_lang() {
                crate::i18n::Lang::Ru => {
                    eprintln!(
                        "\x1b[1;33mпредупреждение\x1b[0m: пересылка ссылочного параметра '{var_name}' в параметр '{param_name}' функции '{func_name}'; применён софт-фикс (copy-in/copy-out) на уровне компилятора для совместимости с платформой JustMC"
                    );
                }
                crate::i18n::Lang::En => {
                    eprintln!(
                        "\x1b[1;33mwarning\x1b[0m: forwarding reference parameter '{var_name}' to reference parameter '{param_name}' of function '{func_name}'; applied compiler soft fix (copy-in/copy-out) for JustMC platform compatibility"
                    );
                }
            }
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn nop(&mut self) -> Id {
        self.add(Mir::Nop)
    }

    /// Arguments list of a HIR node; anything other than `List` is treated as empty.
    fn hir_list(&self, id: Id) -> Vec<Id> {
        if let Hir::List(ids) = &self.hir[id] {
            ids.to_vec()
        } else {
            Vec::new()
        }
    }

    /// Parses a HIR argument into an optional name (`Named`) and the value `Id`.
    fn hir_arg(&self, arg_id: Id) -> Result<(Option<String>, Id), MirError> {
        if let Hir::Named([n, v]) = self.hir[arg_id] {
            Ok((Some(extract_str_hir(self.hir, n)?), v))
        } else {
            Ok((None, arg_id))
        }
    }

    #[instrument(skip(self, s), level = "trace")]
    fn str_id(&mut self, s: &str) -> Id {
        self.add(Mir::Str(StrLit(Symbol::from(s))))
    }

    #[instrument(skip(self, val), level = "trace")]
    fn named_arg(&mut self, name: &str, val: Id) -> Id {
        let name_id = self.str_id(name);
        self.add(Mir::Named([name_id, val]))
    }

    #[instrument(skip(self, args), level = "trace")]
    fn action(&mut self, obj: &str, name: &str, args: Vec<Id>, block: Id) -> Id {
        let obj_id = self.str_id(obj);
        let name_id = self.str_id(name);
        let sel = self.nop();
        let args_list = self.add(Mir::List(args.into_boxed_slice()));
        let lambda = self.nop();
        let cond = self.nop();
        self.add(Mir::Action(
            vec![obj_id, name_id, sel, args_list, block, lambda, cond].into_boxed_slice(),
        ))
    }

    #[instrument(skip(self, args), level = "trace")]
    fn make_action(&mut self, obj: &str, name: &str, args: Vec<Id>) -> Id {
        let block = self.nop();
        self.action(obj, name, args, block)
    }

    #[instrument(skip(self), level = "trace")]
    fn else_action(&mut self, block: Id) -> Id {
        self.action("code", "else", vec![], block)
    }

    #[instrument(skip(self), level = "trace")]
    fn cmp_action(&mut self, name: &str, value: Id, compare: Id, block: Id) -> Id {
        let args = vec![
            self.named_arg("value", value),
            self.named_arg("compare", compare),
        ];
        self.action("variable", name, args, block)
    }

    /// # Errors
    ///
    /// Returns an error if an HIR node cannot be lowered to MIR.
    ///
    /// # Panics
    ///
    /// Panics if lowering creates an invalid expression graph.
    #[instrument(skip(self), level = "trace")]
    pub fn lower(mut self) -> Result<RecExpr<Mir>, MirError> {
        let root = self.lower_stmt(self.hir.root())?;
        assert_eq!(
            usize::from(root),
            self.nodes.len() - 1,
            "MirLowerer root is not the last node! root: {:?}, len: {}. truncate will break the graph!",
            root,
            self.nodes.len()
        );
        self.nodes.truncate(usize::from(root) + 1);
        for (i, node) in self.nodes.iter().enumerate() {
            for child in node.children() {
                assert!(
                    usize::from(*child) < self.nodes.len(),
                    "Invalid child Id {:?} in node {} (len {}). Graph is corrupted before optimizations!",
                    child,
                    i,
                    self.nodes.len()
                );
            }
        }
        Ok(self.nodes.into_recexpr())
    }
}

mod action;
mod calls;
mod exprs;
mod stmts;
mod vars;

/// # Errors
///
/// Returns an error if an HIR node cannot be lowered to MIR.
#[instrument(skip_all, level = "info")]
pub fn lower_to_mir(
    hir: &RecExpr<Hir>,
    ctx: &IrCtx,
    edition: u16,
) -> Result<RecExpr<Mir>, MirError> {
    info!("Starting MIR lowering");
    let lowerer = MirLowerer::new(hir, ctx, edition);
    lowerer.lower()
}

//! Constant Folding pass: evaluates arithmetic/logic operations on constants at compile time.

use super::{MathAnalysis, run_math_pass};
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use egg::*;
use ordered_float::OrderedFloat;
use std::marker::PhantomData;

/// Literal value extractable from an e-class to fold an operation.
trait FoldValue: Copy {
    /// Looks for a literal of its type within the e-class nodes.
    fn from_eclass(egraph: &EGraph<Hir, MathAnalysis>, id: Id) -> Option<Self>;
}

impl FoldValue for f64 {
    fn from_eclass(egraph: &EGraph<Hir, MathAnalysis>, id: Id) -> Option<Self> {
        egraph[id].nodes.iter().find_map(|n| match n {
            Hir::Num(n) => Some(n.0),
            _ => None,
        })
    }
}

impl FoldValue for bool {
    fn from_eclass(egraph: &EGraph<Hir, MathAnalysis>, id: Id) -> Option<Self> {
        egraph[id].nodes.iter().find_map(|n| match n {
            Hir::Bool(b) => Some(*b),
            _ => None,
        })
    }
}

/// Adds a folded node to the e-graph and unions it with the class of the original expression.
///
/// A non-empty result signals egg that the rewrite rule applied.
fn union_folded(egraph: &mut EGraph<Hir, MathAnalysis>, eclass: Id, folded: Hir) -> Vec<Id> {
    let folded_id = egraph.add(folded);
    if egraph.union(eclass, folded_id) {
        return vec![folded_id];
    }
    vec![]
}

/// Applier folding a single operand `?a`.
#[derive(Debug)]
struct Fold1<A, F>(F, PhantomData<A>);

impl<A, F> Applier<Hir, MathAnalysis> for Fold1<A, F>
where
    A: FoldValue,
    F: Fn(A) -> Option<Hir> + Send + Sync + 'static,
{
    fn apply_one(
        &self,
        egraph: &mut EGraph<Hir, MathAnalysis>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<Hir>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let a_var: Var = "?a".parse().unwrap();
        let a_id = egraph.find(subst[a_var]);
        A::from_eclass(egraph, a_id)
            .and_then(|a| (self.0)(a))
            .map_or_else(Vec::new, |folded| union_folded(egraph, eclass, folded))
    }
}

/// Applier folding two operands `?a` and `?b` of the same type.
#[derive(Debug)]
struct Fold2<A, F>(F, PhantomData<A>);

impl<A, F> Applier<Hir, MathAnalysis> for Fold2<A, F>
where
    A: FoldValue,
    F: Fn(A, A) -> Option<Hir> + Send + Sync + 'static,
{
    fn apply_one(
        &self,
        egraph: &mut EGraph<Hir, MathAnalysis>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<Hir>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let a_var: Var = "?a".parse().unwrap();
        let b_var: Var = "?b".parse().unwrap();
        let a_id = egraph.find(subst[a_var]);
        let b_id = egraph.find(subst[b_var]);
        A::from_eclass(egraph, a_id)
            .zip(A::from_eclass(egraph, b_id))
            .and_then(|(a, b)| (self.0)(a, b))
            .map_or_else(Vec::new, |folded| union_folded(egraph, eclass, folded))
    }
}

fn rules() -> Vec<Rewrite<Hir, MathAnalysis>> {
    let num = |f: f64| -> Option<Hir> {
        if f.is_finite() {
            Some(Hir::Num(OrderedFloat(f)))
        } else {
            None
        }
    };

    vec![
        rewrite!("fold-add"; "(+ ?a ?b)" => { Fold2(move |a: f64, b: f64| num(a + b), PhantomData) }),
        rewrite!("fold-sub"; "(- ?a ?b)" => { Fold2(move |a: f64, b: f64| num(a - b), PhantomData) }),
        rewrite!("fold-mul"; "(* ?a ?b)" => { Fold2(move |a: f64, b: f64| num(a * b), PhantomData) }),
        rewrite!("fold-div"; "(/ ?a ?b)" => { Fold2(move |a: f64, b: f64| if b != 0.0 { num(a / b) } else { None }, PhantomData) }),
        rewrite!("fold-mod"; "(% ?a ?b)" => { Fold2(move |a: f64, b: f64| if b != 0.0 { num(a % b) } else { None }, PhantomData) }),
        rewrite!("fold-pow"; "(** ?a ?b)" => { Fold2(move |a: f64, b: f64| num(a.powf(b)), PhantomData) }),
        rewrite!("fold-neg"; "(neg ?a)" => { Fold1(move |a: f64| num(-a), PhantomData) }),
        rewrite!("fold-inc"; "(++ ?a)" => { Fold1(move |a: f64| num(a + 1.0), PhantomData) }),
        rewrite!("fold-dec"; "(-- ?a)" => { Fold1(move |a: f64| num(a - 1.0), PhantomData) }),
        rewrite!("fold-eq"; "(== ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a == b)), PhantomData) }),
        rewrite!("fold-ne"; "(!= ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a != b)), PhantomData) }),
        rewrite!("fold-lt"; "(< ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a < b)), PhantomData) }),
        rewrite!("fold-le"; "(<= ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a <= b)), PhantomData) }),
        rewrite!("fold-gt"; "(> ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a > b)), PhantomData) }),
        rewrite!("fold-ge"; "(>= ?a ?b)" => { Fold2(move |a: f64, b: f64| Some(Hir::Bool(a >= b)), PhantomData) }),
        rewrite!("fold-and"; "(&& ?a ?b)" => { Fold2(move |a: bool, b: bool| Some(Hir::Bool(a && b)), PhantomData) }),
        rewrite!("fold-or"; "(|| ?a ?b)" => { Fold2(move |a: bool, b: bool| Some(Hir::Bool(a || b)), PhantomData) }),
        rewrite!("fold-not"; "(! ?a)" => { Fold1(move |a: bool| Some(Hir::Bool(!a)), PhantomData) }),
    ]
}

static RULES: std::sync::LazyLock<Vec<Rewrite<Hir, MathAnalysis>>> =
    std::sync::LazyLock::new(rules);

fn has_foldable_ops(expr: &RecExpr<Hir>) -> bool {
    expr.as_ref().iter().any(|node| {
        matches!(
            node,
            Hir::Add(_)
                | Hir::Sub(_)
                | Hir::Mul(_)
                | Hir::Div(_)
                | Hir::Mod(_)
                | Hir::Pow(_)
                | Hir::Neg(_)
                | Hir::Inc(_)
                | Hir::Dec(_)
                | Hir::Eq(_)
                | Hir::Ne(_)
                | Hir::Lt(_)
                | Hir::Le(_)
                | Hir::Gt(_)
                | Hir::Ge(_)
                | Hir::And(_)
                | Hir::Or(_)
                | Hir::Not(_)
        )
    })
}

#[derive(Debug, Clone, Default)]
pub struct ConstantFoldingPass;

impl FunctionPass<Hir> for ConstantFoldingPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if !has_foldable_ops(expr) {
            return expr.clone();
        }
        run_math_pass(expr, &RULES)
    }
}

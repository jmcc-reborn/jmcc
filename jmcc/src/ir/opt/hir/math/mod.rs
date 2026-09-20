//! Math optimization passes for HIR.
//!
//! Contains constant folding and algebraic simplification passes.

pub mod algebraic_simplification;
pub mod constant_folding;

pub use algebraic_simplification::AlgebraicSimplificationPass;
pub use constant_folding::ConstantFoldingPass;

use crate::ir::hir::Hir;
use egg::*;
use std::time::Duration;

/// Analysis that performs let-propagation of constants.
#[derive(Default)]
pub struct MathAnalysis;

impl Analysis<Hir> for MathAnalysis {
    type Data = ();

    fn make(_egraph: &mut EGraph<Hir, Self>, _enode: &Hir, _id: Id) {}

    fn merge(&mut self, _to: &mut Self::Data, _from: Self::Data) -> DidMerge {
        DidMerge(false, false)
    }

    fn modify(_egraph: &mut EGraph<Hir, Self>, _id: Id) {}
}

/// Cost function that prefers constants over variables and penalises `Let`.
pub struct MathCostFn;

impl CostFunction<Hir> for MathCostFn {
    type Cost = usize;

    fn cost<C>(&mut self, enode: &Hir, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        match enode {
            // Constants are cheapest — prefer them after propagation.
            Hir::Num(_) | Hir::Bool(_) | Hir::Str(_) | Hir::Nop => 1,
            // Variables are slightly more expensive so that the extractor
            // picks a constant over a variable when both are in the same e-class.
            Hir::Var(_) => 2,
            // `Let` nodes are expensive — prefer the eliminated (body) form.
            Hir::Let([_, _, _]) => {
                50usize.saturating_add(enode.fold(0, |s, id| s.saturating_add(costs(id))))
            }
            // Everything else: AST size.
            _ => enode.fold(1, |sum, id| sum.saturating_add(costs(id))),
        }
    }
}

pub(crate) fn run_math_pass(
    expr: &RecExpr<Hir>,
    rules: &[Rewrite<Hir, MathAnalysis>],
) -> RecExpr<Hir> {
    if expr.is_empty() {
        return expr.clone();
    }

    log::trace!("Starting math pass with {} rules...", rules.len());
    log::trace!("Initial expr: {expr:?}");

    let runner = Runner::<Hir, MathAnalysis>::default()
        .with_iter_limit(30)
        .with_node_limit(10_000)
        .with_time_limit(Duration::from_millis(100))
        .with_expr(expr)
        .run(rules);

    let extractor = Extractor::new(&runner.egraph, MathCostFn);
    let (cost, optimized) = extractor.find_best(runner.roots[0]);

    log::trace!("Math pass complete. Extracted cost: {cost}");
    log::trace!("Optimized expr: {optimized:?}");

    optimized
}

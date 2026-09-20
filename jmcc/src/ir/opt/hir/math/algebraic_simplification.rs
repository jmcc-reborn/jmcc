//! Algebraic Simplification pass: removes identity operations (x+0, x*1, etc.).

use super::{MathAnalysis, run_math_pass};
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use egg::*;

fn rules() -> Vec<Rewrite<Hir, MathAnalysis>> {
    vec![
        rewrite!("add-0-r"; "(+ ?x 0)" => "?x"),
        rewrite!("add-0-l"; "(+ 0 ?x)" => "?x"),
        rewrite!("sub-0"; "(- ?x 0)" => "?x"),
        rewrite!("mul-1-r"; "(* ?x 1)" => "?x"),
        rewrite!("mul-1-l"; "(* 1 ?x)" => "?x"),
        rewrite!("mul-0-r"; "(* ?x 0)" => "0"),
        rewrite!("mul-0-l"; "(* 0 ?x)" => "0"),
        rewrite!("div-1"; "(/ ?x 1)" => "?x"),
        rewrite!("div-0-r"; "(/ 0 ?x)" => "0"),
        rewrite!("mod-1"; "(% ?x 1)" => "0"),
        rewrite!("pow-0"; "(** ?x 0)" => "1"),
        rewrite!("pow-1"; "(** ?x 1)" => "?x"),
        rewrite!("pow-1-base"; "(** 1 ?x)" => "1"),
        rewrite!("pow-0-base"; "(** 0 ?x)" => "0"),
        rewrite!("sub-self"; "(- ?a ?a)" => "0"),
        rewrite!("div-self"; "(/ ?a ?a)" => "1"),
        rewrite!("mod-self"; "(% ?a ?a)" => "0"),
        rewrite!("eq-self"; "(== ?a ?a)" => "true"),
        rewrite!("ne-self"; "(!= ?a ?a)" => "false"),
        rewrite!("lt-self"; "(< ?a ?a)"  => "false"),
        rewrite!("gt-self"; "(> ?a ?a)"  => "false"),
        rewrite!("le-self"; "(<= ?a ?a)" => "true"),
        rewrite!("ge-self"; "(>= ?a ?a)" => "true"),
        rewrite!("neg-neg"; "(neg (neg ?a))" => "?a"),
        rewrite!("neg-sub"; "(neg (- ?a ?b))" => "(- ?b ?a)"),
        rewrite!("and-true-r";  "(&& ?x true)"  => "?x"),
        rewrite!("and-true-l";  "(&& true ?x)"  => "?x"),
        rewrite!("and-false-r"; "(&& ?x false)" => "false"),
        rewrite!("and-false-l"; "(&& false ?x)" => "false"),
        rewrite!("and-self";    "(&& ?x ?x)"    => "?x"),
        rewrite!("or-false-r";  "(|| ?x false)" => "?x"),
        rewrite!("or-false-l";  "(|| false ?x)" => "?x"),
        rewrite!("or-true-r";   "(|| ?x true)"  => "true"),
        rewrite!("or-true-l";   "(|| true ?x)"  => "true"),
        rewrite!("or-self";     "(|| ?x ?x)"    => "?x"),
        rewrite!("not-not"; "(! (! ?x))" => "?x"),
        rewrite!("if-true";  "(if true ?x ?y)" => "?x"),
        rewrite!("if-false"; "(if false ?x ?y)" => "?y"),
        rewrite!("if-not";   "(if (! ?c) ?a ?b)" => "(if ?c ?b ?a)"),
        rewrite!("let-elim"; "(let ?x ?x ?body)" => "?body"),
    ]
}

static RULES: std::sync::LazyLock<Vec<Rewrite<Hir, MathAnalysis>>> =
    std::sync::LazyLock::new(rules);

fn has_simplifiable_ops(expr: &RecExpr<Hir>) -> bool {
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
                | Hir::Eq(_)
                | Hir::Ne(_)
                | Hir::Lt(_)
                | Hir::Le(_)
                | Hir::Gt(_)
                | Hir::Ge(_)
                | Hir::And(_)
                | Hir::Or(_)
                | Hir::Not(_)
                | Hir::If(_)
                | Hir::Let(_)
        )
    })
}

#[derive(Debug, Clone, Default)]
pub struct AlgebraicSimplificationPass;

impl FunctionPass<Hir> for AlgebraicSimplificationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if !has_simplifiable_ops(expr) {
            return expr.clone();
        }
        run_math_pass(expr, &RULES)
    }
}

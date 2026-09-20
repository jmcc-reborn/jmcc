use crate::ir::hir::Hir;
use crate::ir::opt::dispatch_passes;
use crate::ir::opt::hir::inliner::InlineParams;
use crate::ir::opt::{FunctionPass, ModulePass};
use egg::{Id, RecExpr};

impl crate::ir::opt::FunctionContainer for Hir {
    fn get_function_body(&self) -> Option<Id> {
        match self {
            Hir::FuncDecl([_, _, body])
            | Hir::ProcDecl([_, _, body])
            | Hir::EventDecl([_, body]) => Some(*body),
            _ => None,
        }
    }
}

dispatch_passes!(HirModulePasses: ModulePass<Hir> {
    Inline => inliner::InlinePass,
    DeadFunctionElimination => dce::DeadFunctionEliminationPass,
});

dispatch_passes!(HirFunctionPasses: FunctionPass<Hir> {
    NoOp => NoOpHirFunctionPass,
    ConstantFolding => math::ConstantFoldingPass,
    AlgebraicSimplification => math::AlgebraicSimplificationPass,
    CopyPropagation => copy_prop::CopyPropagationPass,
    DeadCodeElimination => dce::DeadCodeEliminationPass,
});

#[derive(Debug, Clone, Default)]
pub struct NoOpHirFunctionPass;

impl FunctionPass<Hir> for NoOpHirFunctionPass {
    const OPT_LEVEL: u8 = 0;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        expr.clone()
    }
}

#[must_use]
pub fn all_module_passes(opt_level: u8, keep_tests: bool) -> Vec<HirModulePasses> {
    vec![
        HirModulePasses::Inline(inliner::InlinePass {
            params: InlineParams::for_opt_level(opt_level, 0),
        }),
        HirModulePasses::DeadFunctionElimination(dce::DeadFunctionEliminationPass { keep_tests }),
    ]
}

#[must_use]
pub fn all_function_passes() -> Vec<HirFunctionPasses> {
    vec![
        HirFunctionPasses::NoOp(NoOpHirFunctionPass),
        HirFunctionPasses::ConstantFolding(math::ConstantFoldingPass),
        HirFunctionPasses::AlgebraicSimplification(math::AlgebraicSimplificationPass),
        HirFunctionPasses::CopyPropagation(copy_prop::CopyPropagationPass),
        HirFunctionPasses::DeadCodeElimination(dce::DeadCodeEliminationPass),
    ]
}

pub mod common;
pub mod copy_prop;
pub mod dce;
pub mod inliner;
pub mod math;

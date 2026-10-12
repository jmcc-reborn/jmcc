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
    DeadArgumentElimination => dead_arg_elim::DeadArgumentEliminationPass,
});

dispatch_passes!(HirFunctionPasses: FunctionPass<Hir> {
    NoOp => NoOpHirFunctionPass,
    GuardFactoring => guard_factoring::GuardFactoringPass,
    ConstantFolding => math::ConstantFoldingPass,
    AlgebraicSimplification => math::AlgebraicSimplificationPass,
    CopyPropagation => copy_prop::CopyPropagationPass,
    DeadCodeElimination => dce::DeadCodeEliminationPass,
    DeadLoop => dead_loop::DeadLoopEliminationPass,
    Licm => licm::LoopInvariantCodeMotionPass,
    LoopUnswitch => loop_unswitch::LoopUnswitchPass,
    LoopFusion => loop_fusion::LoopFusionPass,
    CodeSinking => code_sinking::CodeSinkingPass,
    Cse => cse::CommonSubexpressionEliminationPass,
    LoopUnroll => loop_unroll::LoopUnrollPass,
    Tco => tco::TailCallOptimizationPass,
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
        HirModulePasses::DeadArgumentElimination(dead_arg_elim::DeadArgumentEliminationPass),
    ]
}

#[must_use]
pub fn all_function_passes() -> Vec<HirFunctionPasses> {
    vec![
        HirFunctionPasses::NoOp(NoOpHirFunctionPass),
        HirFunctionPasses::GuardFactoring(guard_factoring::GuardFactoringPass),
        HirFunctionPasses::ConstantFolding(math::ConstantFoldingPass),
        HirFunctionPasses::AlgebraicSimplification(math::AlgebraicSimplificationPass),
        HirFunctionPasses::CopyPropagation(copy_prop::CopyPropagationPass::default()),
        HirFunctionPasses::DeadCodeElimination(dce::DeadCodeEliminationPass),
        HirFunctionPasses::DeadLoop(dead_loop::DeadLoopEliminationPass),
        HirFunctionPasses::Licm(licm::LoopInvariantCodeMotionPass),
        HirFunctionPasses::LoopUnswitch(loop_unswitch::LoopUnswitchPass),
        HirFunctionPasses::LoopFusion(loop_fusion::LoopFusionPass),
        HirFunctionPasses::CodeSinking(code_sinking::CodeSinkingPass),
        HirFunctionPasses::Cse(cse::CommonSubexpressionEliminationPass),
        HirFunctionPasses::LoopUnroll(loop_unroll::LoopUnrollPass),
        HirFunctionPasses::Tco(tco::TailCallOptimizationPass),
    ]
}

pub mod code_sinking;
pub mod common;
pub mod copy_prop;
pub mod cse;
pub mod dce;
pub mod dead_arg_elim;
pub mod dead_loop;
pub mod guard_factoring;
pub mod inliner;
pub mod licm;
pub mod loop_fusion;
pub mod loop_unroll;
pub mod loop_unswitch;
pub mod math;
pub mod tco;

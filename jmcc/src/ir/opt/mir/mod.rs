use crate::ir::mir::Mir;
use crate::ir::opt::dispatch_passes;
use crate::ir::opt::{FunctionPass, ModulePass};
use egg::{Id, RecExpr};

impl crate::ir::opt::FunctionContainer for Mir {
    fn get_function_body(&self) -> Option<Id> {
        match self {
            Mir::FuncDecl([_, _, body])
            | Mir::ProcDecl([_, _, body])
            | Mir::EventDecl([_, body]) => Some(*body),
            _ => None,
        }
    }
}

dispatch_passes!(MirModulePasses: ModulePass<Mir> {
    NoOp => NoOpMirModulePass,
});

dispatch_passes!(MirFunctionPasses: FunctionPass<Mir> {
    DeadStoreElimination => dse::DeadStoreEliminationPass,
    SetVariableFolding => set_variable_folding::SetVariableFoldingPass,
    RedundantReturnElimination => redundant_return_elimination::RedundantReturnEliminationPass,
    CopyCoalescing => copy_coalescing::CopyCoalescingPass,
    CoordinateFolding => coordinate_folding::CoordinateFoldingPass,
    RedundantElseElimination => redundant_else_elimination::RedundantElseEliminationPass,
    BranchFolding => branch_folding::BranchFoldingPass,
    BranchMerging => branch_merging::BranchMergingPass,
    ActionPeephole => instcombine::peephole::ActionPeepholePass,
    ConstantFolding => instcombine::constant_folding::MirConstantFoldingPass,
});

#[derive(Debug, Clone, Default)]
pub struct NoOpMirModulePass;

impl ModulePass<Mir> for NoOpMirModulePass {
    const OPT_LEVEL: u8 = 0;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        expr.clone()
    }
}

#[must_use]
pub fn all_module_passes() -> Vec<MirModulePasses> {
    vec![MirModulePasses::NoOp(NoOpMirModulePass)]
}

#[must_use]
pub fn all_function_passes(edition: u16) -> Vec<MirFunctionPasses> {
    vec![
        MirFunctionPasses::RedundantReturnElimination(
            redundant_return_elimination::RedundantReturnEliminationPass,
        ),
        MirFunctionPasses::CopyCoalescing(copy_coalescing::CopyCoalescingPass),
        MirFunctionPasses::DeadStoreElimination(dse::DeadStoreEliminationPass),
        MirFunctionPasses::SetVariableFolding(set_variable_folding::SetVariableFoldingPass),
        MirFunctionPasses::CoordinateFolding(coordinate_folding::CoordinateFoldingPass),
        MirFunctionPasses::ActionPeephole(instcombine::peephole::ActionPeepholePass),
        MirFunctionPasses::ConstantFolding(instcombine::constant_folding::MirConstantFoldingPass),
        MirFunctionPasses::RedundantElseElimination(
            redundant_else_elimination::RedundantElseEliminationPass { edition },
        ),
        MirFunctionPasses::BranchFolding(branch_folding::BranchFoldingPass),
        MirFunctionPasses::BranchMerging(branch_merging::BranchMergingPass),
    ]
}

pub mod branch_folding;
pub mod branch_merging;
pub mod common;
pub mod coordinate_folding;
pub mod copy_coalescing;
pub mod dse;
pub mod instcombine;
pub mod redundant_else_elimination;
pub mod redundant_return_elimination;
pub mod set_variable_folding;

pub use instcombine::{constant_folding, peephole};

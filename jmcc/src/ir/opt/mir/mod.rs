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
    SetVariableFolding => set_variable_folding::SetVariableFoldingPass,
    RedundantReturnElimination => redundant_return_elimination::RedundantReturnEliminationPass,
    CopyCoalescing => copy_coalescing::CopyCoalescingPass,
    CoordinateFolding => coordinate_folding::CoordinateFoldingPass,
    RedundantElseElimination => redundant_else_elimination::RedundantElseEliminationPass,
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
        MirFunctionPasses::SetVariableFolding(set_variable_folding::SetVariableFoldingPass),
        MirFunctionPasses::CoordinateFolding(coordinate_folding::CoordinateFoldingPass),
        MirFunctionPasses::RedundantElseElimination(
            redundant_else_elimination::RedundantElseEliminationPass { edition },
        ),
    ]
}

pub mod common;
pub mod coordinate_folding;
pub mod copy_coalescing;
pub mod redundant_else_elimination;
pub mod redundant_return_elimination;
pub mod set_variable_folding;

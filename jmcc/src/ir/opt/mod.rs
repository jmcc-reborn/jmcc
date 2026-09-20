pub mod hir;
pub mod mir;

use crate::ir::arena::NodeArena;
use egg::{Id, Language, RecExpr};

/// Declares a pass dispatcher enum for a pass family.
macro_rules! dispatch_passes {
    ($name:ident : $trait:ident < $lang:ident > { $($variant:ident => $inner:ty),+ $(,)? }) => {
        #[derive(Debug, Clone)]
        pub enum $name {
            $($variant($inner)),+
        }

        impl $trait<$lang> for $name {
            const OPT_LEVEL: u8 = 0;

            fn opt_level(&self) -> u8 {
                match self {
                    $($name::$variant(p) => p.opt_level()),+
                }
            }

            fn run(&self, expr: &RecExpr<$lang>) -> RecExpr<$lang> {
                match self {
                    $($name::$variant(p) => p.run(expr)),+
                }
            }
        }
    };
}
pub(crate) use dispatch_passes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassScope {
    Module,
    Function,
}

pub trait ModulePass<L: Language>: std::fmt::Debug {
    /// Minimum `-O` level from which this pass is enabled.
    const OPT_LEVEL: u8;

    /// Optimization level for this pass.
    fn opt_level(&self) -> u8 {
        Self::OPT_LEVEL
    }

    fn run(&self, expr: &RecExpr<L>) -> RecExpr<L>;
}

pub trait FunctionPass<L: Language>: std::fmt::Debug {
    /// Minimum `-O` level from which this pass is enabled.
    const OPT_LEVEL: u8;

    /// Optimization level for this pass.
    fn opt_level(&self) -> u8 {
        Self::OPT_LEVEL
    }

    fn run(&self, expr: &RecExpr<L>) -> RecExpr<L>;
}

pub trait FunctionContainer: Language {
    fn get_function_body(&self) -> Option<Id>;
}

#[derive(Debug, Clone)]
pub struct OptConfig {
    pub opt_level: u8,
    pub enabled_passes: Vec<String>,
    pub disabled_passes: Vec<String>,
}

impl OptConfig {
    #[must_use]
    pub const fn new(
        opt_level: u8,
        enabled_passes: Vec<String>,
        disabled_passes: Vec<String>,
    ) -> Self {
        Self {
            opt_level,
            enabled_passes,
            disabled_passes,
        }
    }
}

/// Optimizes an IR expression by running module passes and function passes.
///
/// # Panics
///
/// Panics if an optimized function body is empty during splicing.
#[must_use]
pub fn optimize<L, MP, FP>(
    expr: &RecExpr<L>,
    config: &OptConfig,
    module_passes: Vec<MP>,
    function_passes: Vec<FP>,
) -> RecExpr<L>
where
    L: Language + FunctionContainer + PartialEq + Send + Sync,
    MP: ModulePass<L> + Clone,
    FP: FunctionPass<L> + Clone + Send + Sync,
{
    if module_passes.is_empty() && function_passes.is_empty() {
        return expr.clone();
    }

    let check_pass = |name: &str, opt_level: u8| -> bool {
        let is_disabled = config
            .disabled_passes
            .iter()
            .any(|p| p.to_lowercase() == name);
        if is_disabled {
            return false;
        }

        let is_enabled = config
            .enabled_passes
            .iter()
            .any(|p| p.to_lowercase() == name);
        if is_enabled {
            return true;
        }

        config.enabled_passes.is_empty() && opt_level <= config.opt_level
    };

    let mut result = expr.clone();

    for pass in &module_passes {
        let name = pass_name(pass);
        if check_pass(&name, pass.opt_level()) {
            log::info!("  Module Pass: {name}");
            result = run_timed(&name, || pass.run(&result));
        }
    }

    let active_function_passes: Vec<FP> = function_passes
        .into_iter()
        .filter(|p| {
            let name = pass_name(p);
            check_pass(&name, p.opt_level())
        })
        .collect();

    if !active_function_passes.is_empty() {
        use rayon::prelude::*;
        use std::collections::HashMap;

        // Collect all function containers and extract their bodies.
        let mut function_items = Vec::new();
        for (i, node) in result.as_ref().iter().enumerate() {
            if let Some(body_id) = node.get_function_body() {
                let body_expr = result[body_id].build_recexpr(|id| result[id].clone());
                function_items.push((i, body_id, body_expr));
            }
        }

        if !function_items.is_empty() {
            let start = std::time::Instant::now();
            let pass_names: Vec<String> = active_function_passes.iter().map(pass_name).collect();
            log::info!(
                "  Running function passes [{}] in parallel on {} handlers...",
                pass_names.join(", "),
                function_items.len()
            );

            let max_rounds = 3;
            let optimized_items: Vec<(Id, RecExpr<L>)> = function_items
                .into_par_iter()
                .map(|(_node_idx, body_id, mut body_expr)| {
                    for _round in 0..max_rounds {
                        let mut changed = false;
                        for pass in &active_function_passes {
                            let next = pass.run(&body_expr);
                            if next != body_expr {
                                body_expr = next;
                                changed = true;
                            }
                        }
                        if !changed {
                            break;
                        }
                    }
                    (body_id, body_expr)
                })
                .collect();

            let body_map: HashMap<Id, RecExpr<L>> = optimized_items.into_iter().collect();

            // Splice all optimized bodies back into the program in a single linear pass.
            result = splice_function_bodies(&result, &body_map);
            log::info!(
                "  Function passes complete in {:.2}ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    result
}

fn splice_function_bodies<L>(
    expr: &RecExpr<L>,
    body_map: &std::collections::HashMap<Id, RecExpr<L>>,
) -> RecExpr<L>
where
    L: Language + FunctionContainer,
{
    let mut new_nodes = NodeArena::with_capacity(expr.len());
    let mut id_map = vec![Id::from(0); expr.len()];
    let mut body_id_map = Vec::new();

    for (i, node) in expr.as_ref().iter().enumerate() {
        let old_id = Id::from(i);

        if let Some(body_id) = node.get_function_body() {
            let optimized_body_expr = &body_map[&body_id];

            body_id_map.clear();
            body_id_map.resize(optimized_body_expr.len(), Id::from(0));
            for (j, b_node) in optimized_body_expr.as_ref().iter().enumerate() {
                let mapped_node = b_node.clone().map_children(|c| body_id_map[usize::from(c)]);
                let new_id = new_nodes.add(mapped_node);
                body_id_map[j] = new_id;
            }
            let new_body_id = *body_id_map.last().expect("optimized body cannot be empty");

            let new_node = node.clone().map_children(|c| {
                if c == body_id {
                    new_body_id
                } else {
                    id_map[usize::from(c)]
                }
            });

            let new_id = new_nodes.add(new_node);
            id_map[usize::from(old_id)] = new_id;
        } else {
            let new_node = node.clone().map_children(|c| id_map[usize::from(c)]);

            let new_id = new_nodes.add(new_node);
            id_map[usize::from(old_id)] = new_id;
        }
    }

    new_nodes.into_recexpr()
}

/// Pass name for `--passes` / `--disable-passes` and logging.
fn pass_name(pass: &impl std::fmt::Debug) -> String {
    format!("{pass:?}")
        .split('(')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// Executes a closure and logs elapsed time.
fn run_timed<L>(name: &str, f: impl FnOnce() -> RecExpr<L>) -> RecExpr<L> {
    let start = std::time::Instant::now();
    let result = f();
    log::info!(
        "  {name} done in {:.2}ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    result
}

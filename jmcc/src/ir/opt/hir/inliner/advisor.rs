use crate::ir::hir::Hir;
use egg::{Id, Language as _, RecExpr};
use std::collections::{HashMap, HashSet};

use super::cost::{InlineCost, InlineParams, inline_constants};
use super::utils::{
    collect_calls_in_subtree, detect_recursive, eval_const_arg, extract_name, extract_params,
};

#[derive(Debug, Clone)]
pub struct FuncInfo {
    pub name: String,
    pub params: Vec<String>,
    pub body_id: Id,
    /// Whether the parameter list is variadic (`*args`/`**kwargs`). Such parameters are not
    /// bound positionally, so the inliner cannot substitute an argument for them.
    pub variadic: bool,
}

pub struct CostAnalyzer<'a> {
    expr: &'a RecExpr<Hir>,
    cost: i32,
    single_bb: bool,
    has_call: bool,
    visited: HashSet<Id>,
}

impl<'a> CostAnalyzer<'a> {
    fn new(expr: &'a RecExpr<Hir>) -> Self {
        Self {
            expr,
            cost: 0,
            single_bb: true,
            has_call: false,
            visited: HashSet::new(),
        }
    }

    fn analyze(&mut self, id: Id) {
        if !self.visited.insert(id) {
            return;
        }

        let node = &self.expr[id];
        log::trace!("CostAnalyzer::analyze: id={id:?}, node={node:?}");

        match node {
            Hir::Num(_) | Hir::Bool(_) | Hir::Str(_) | Hir::Var(_) | Hir::Nop => {}

            Hir::Let([_var, val, body]) => {
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*val);
                self.analyze(*body);
            }

            Hir::If([cond, then, els]) => {
                self.single_bb = false;
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*cond);
                self.analyze(*then);
                self.analyze(*els);
            }

            Hir::While([cond, body]) => {
                self.single_bb = false;
                self.cost += inline_constants::INSTR_COST + inline_constants::LOOP_PENALTY;
                self.analyze(*cond);
                self.analyze(*body);
            }

            Hir::Return(val) => {
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*val);
            }

            Hir::FuncCall([target, args]) => {
                self.cost += inline_constants::CALL_PENALTY;
                self.has_call = true;
                self.analyze(*target);
                self.analyze(*args);
            }

            Hir::Action(ids) => {
                self.cost += inline_constants::CALL_PENALTY;
                self.has_call = true;
                let children = ids.to_vec();
                for c in children {
                    self.analyze(c);
                }
            }

            Hir::Text([fmt, args]) => {
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*fmt);
                self.analyze(*args);
            }

            Hir::Add([a, b])
            | Hir::Sub([a, b])
            | Hir::Mul([a, b])
            | Hir::Div([a, b])
            | Hir::Mod([a, b])
            | Hir::Pow([a, b])
            | Hir::Eq([a, b])
            | Hir::Ne([a, b])
            | Hir::Lt([a, b])
            | Hir::Le([a, b])
            | Hir::Gt([a, b])
            | Hir::Ge([a, b])
            | Hir::BitAnd([a, b])
            | Hir::BitOr([a, b])
            | Hir::BitXor([a, b])
            | Hir::Shl([a, b])
            | Hir::Shr([a, b])
            | Hir::And([a, b])
            | Hir::Or([a, b])
            | Hir::In([a, b]) => {
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*a);
                self.analyze(*b);
            }

            Hir::Not(a) | Hir::Neg(a) | Hir::Inc(a) | Hir::Dec(a) => {
                self.cost += inline_constants::INSTR_COST;
                self.analyze(*a);
            }

            _ => {
                self.cost += inline_constants::INSTR_COST;
                let children: Vec<Id> = node.children().to_vec();
                for c in children {
                    self.analyze(c);
                }
            }
        }
    }
}

pub struct InlineAdvisor<'a> {
    expr: &'a RecExpr<Hir>,
    pub func_table: HashMap<String, FuncInfo>,
    pub recursive: HashSet<String>,
    call_counts: HashMap<String, usize>,
    params: &'a InlineParams,
}

impl<'a> InlineAdvisor<'a> {
    #[must_use]
    pub fn new(expr: &'a RecExpr<Hir>, params: &'a InlineParams) -> Self {
        log::trace!("InlineAdvisor::new: initializing advisor");
        let mut func_table = HashMap::new();
        let mut call_counts: HashMap<String, usize> = HashMap::new();

        for (i, node) in expr.as_ref().iter().enumerate() {
            if let Hir::FuncDecl([name_id, params_id, body_id]) = node
                && let Some(name) = extract_name(expr, *name_id)
            {
                log::trace!("  Found FuncDecl: {name} (id={i})");
                let params = extract_params(expr, *params_id);
                let variadic = params
                    .iter()
                    .any(|param| crate::utils::split_param_spread(param).1 != 0);
                func_table.insert(
                    name.clone(),
                    FuncInfo {
                        name,
                        params,
                        body_id: *body_id,
                        variadic,
                    },
                );
            }
        }

        let mut call_graph: HashMap<String, HashSet<String>> = HashMap::new();

        for (name, info) in &func_table {
            let mut callees = HashSet::new();
            log::trace!("  Building call graph for function: {name}");
            collect_calls_in_subtree(
                expr,
                info.body_id,
                &func_table,
                &mut call_counts,
                &mut callees,
            );
            call_graph.insert(name.clone(), callees);
        }

        let root_id = Id::from(expr.as_ref().len() - 1);
        log::trace!("  Building call graph for global scope");
        collect_calls_in_subtree(
            expr,
            root_id,
            &func_table,
            &mut call_counts,
            &mut HashSet::new(),
        );

        let recursive = detect_recursive(&call_graph, &func_table);

        Self {
            expr,
            func_table,
            recursive,
            call_counts,
            params,
        }
    }

    #[must_use]
    pub fn get_advice(&self, callee_name: &str, arg_ids: &[Id], depth: u32) -> InlineCost {
        log::trace!("InlineAdvisor::get_advice: callee='{callee_name}', depth={depth}");

        let Some(callee) = self.func_table.get(callee_name) else {
            return InlineCost::Never("unknown function");
        };

        if self.recursive.contains(callee_name) {
            return InlineCost::Never("recursive function");
        }

        if depth >= inline_constants::MAX_INLINE_DEPTH {
            return InlineCost::Never("max inline depth reached");
        }

        // Inlining substitutes args positionally, so counts must match: default-arg calls
        // pass fewer and variadic params do not fit — those stay `code::call_function`.
        if callee.variadic || arg_ids.len() != callee.params.len() {
            return InlineCost::Never("arity mismatch or variadic parameters");
        }

        if has_early_or_multiple_returns(self.expr, callee.body_id) {
            return InlineCost::Never("callee contains early or multiple returns");
        }

        for (i, param_name) in callee.params.iter().enumerate() {
            if let Some(&arg_id) = arg_ids.get(i)
                && let Some(c) = eval_const_arg(self.expr, arg_id)
            {
                log::trace!("  Param '{param_name}' has constant value: {c:?}");
            }
        }

        let mut analyzer = CostAnalyzer::new(self.expr);
        analyzer.analyze(callee.body_id);

        let mut threshold = self.params.default_threshold;
        let call_count = self.call_counts.get(callee_name).copied().unwrap_or(0);

        if call_count == 1 {
            threshold += inline_constants::LAST_CALL_TO_STATIC_BONUS;
        }
        if analyzer.single_bb {
            threshold += threshold * inline_constants::SINGLE_BB_BONUS_PERCENT / 100;
        }
        if !analyzer.has_call {
            threshold += 50;
        }
        if call_count > 20 {
            threshold += 50;
        } else if call_count > 5 {
            threshold += 25;
        }

        let param_penalty = callee.params.len() as i32 * inline_constants::INSTR_COST;
        let total_cost = analyzer.cost + param_penalty;

        if total_cost > inline_constants::NEVER_INLINE_SIZE {
            return InlineCost::Never("body too large (absolute limit)");
        }

        if total_cost <= inline_constants::ALWAYS_INLINE_SIZE {
            return InlineCost::Always("trivially small body");
        }

        if total_cost < threshold {
            InlineCost::Cost {
                cost: total_cost,
                threshold,
                reason: "cost below threshold",
            }
        } else {
            InlineCost::Cost {
                cost: total_cost,
                threshold,
                reason: "cost exceeds threshold",
            }
        }
    }
}

fn has_early_or_multiple_returns(expr: &RecExpr<Hir>, body_id: Id) -> bool {
    let mut return_count = 0;
    count_returns(expr, body_id, &mut return_count);
    if return_count == 0 {
        return false;
    }
    if return_count > 1 {
        return true;
    }
    // Exactly 1 return. Check if it is in tail position.
    !is_tail_return(expr, body_id)
}

fn count_returns(expr: &RecExpr<Hir>, id: Id, count: &mut usize) {
    let node = &expr[id];
    if matches!(node, Hir::Return(_)) {
        *count += 1;
    }
    for child in node.children() {
        count_returns(expr, *child, count);
    }
}

fn is_tail_return(expr: &RecExpr<Hir>, id: Id) -> bool {
    match &expr[id] {
        Hir::Return(_) => true,
        Hir::Block(stmts) => {
            if let Some(&last) = stmts.last() {
                // Ensure all earlier statements have 0 returns
                let mut earlier_returns = 0;
                for &s in &stmts[..stmts.len() - 1] {
                    count_returns(expr, s, &mut earlier_returns);
                }
                if earlier_returns > 0 {
                    return false;
                }
                is_tail_return(expr, last)
            } else {
                false
            }
        }
        Hir::Let([_var, val, body]) => {
            let mut val_returns = 0;
            count_returns(expr, *val, &mut val_returns);
            if val_returns > 0 {
                return false;
            }
            is_tail_return(expr, *body)
        }
        _ => false,
    }
}

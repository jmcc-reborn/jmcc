//! Module name mangling and import renaming map.

use std::collections::HashMap;

use lasso::Rodeo;
use tracing::{Level, debug, instrument, span, trace};

use super::*;

impl ImportResolver {
    #[instrument(skip(ast), fields(module_name = module_name), level = "debug")]
    pub(super) fn apply_mangling(ast: &mut Ast, module_name: &str) {
        let _span = span!(Level::TRACE, "apply_mangling", module = module_name).entered();
        let mut name_map = HashMap::new();
        mangle_stmts(
            &mut ast.statements,
            module_name,
            &mut ast.strings,
            &mut name_map,
        );
        if !name_map.is_empty() {
            debug!(renamed = name_map.len(), "Rewriting calls after mangling");
            let call_str = ast.strings.get_or_intern("call");
            Self::rewrite_calls(ast, &name_map, move |_| call_str);
        }
    }

    #[instrument(skip(ast, import_map), level = "debug")]
    pub(super) fn apply_import_map(ast: &mut Ast, import_map: &HashMap<String, String>) {
        let str_map: HashMap<StrId, StrId> = import_map
            .iter()
            .map(|(l, m)| (ast.strings.get_or_intern(l), ast.strings.get_or_intern(m)))
            .collect();
        debug!(
            mapped = str_map.len(),
            "Built StrId map for import resolution"
        );
        Self::rewrite_calls(ast, &str_map, |n| n);
    }

    fn rewrite_calls(
        ast: &mut Ast,
        map: &HashMap<StrId, StrId>,
        method_fn: impl Fn(StrId) -> StrId,
    ) {
        let _span = span!(Level::TRACE, "rewrite_calls", candidates = map.len()).entered();
        let updates: Vec<(ExprId, StrId, std::ops::Range<usize>, bool)> = ast
            .exprs
            .iter()
            .filter_map(|(id, e)| {
                let Expr::Call(c) = e else { return None };
                let Expr::Ident(n, sp) = &ast.exprs[c.target] else {
                    return None;
                };
                let is_direct = c.method == *n;
                map.get(n).map(|&new| (id, new, sp.clone(), is_direct))
            })
            .collect();

        trace!(matches = updates.len(), "Found call sites to rewrite");

        for (id, new, sp, is_direct) in updates {
            let new_id = ast.exprs.alloc(Expr::Ident(new, sp));
            if let Expr::Call(c) = &mut ast.exprs[id] {
                c.target = new_id;
                if is_direct {
                    c.method = method_fn(new);
                }
            }
        }
    }
}

#[instrument(skip(stmts, strings, map), fields(prefix = prefix, statements = stmts.len()), level = "trace")]
fn mangle_stmts(
    stmts: &mut [Statement],
    prefix: &str,
    strings: &mut Rodeo,
    map: &mut HashMap<StrId, StrId>,
) {
    for s in stmts.iter_mut() {
        match s {
            Statement::Function(f) => {
                mangle_one(&mut f.name, prefix, strings, map);
                for alias in &f.aliases {
                    map.insert(*alias, f.name);
                }
                mangle_stmts(&mut f.body, prefix, strings, map);
            }
            Statement::Process(p) => {
                mangle_one(&mut p.name, prefix, strings, map);
                for alias in &p.aliases {
                    map.insert(*alias, p.name);
                }
                mangle_stmts(&mut p.body, prefix, strings, map);
            }
            Statement::Class(c) => {
                mangle_one(&mut c.name, prefix, strings, map);
                for alias in &c.aliases {
                    map.insert(*alias, c.name);
                }
                let new_prefix = strings.resolve(&c.name).to_owned();
                let mut method_map = HashMap::new();
                for s in &mut c.body {
                    if let Statement::Function(f) = s {
                        mangle_one(&mut f.name, &new_prefix, strings, &mut method_map);
                        mangle_stmts(&mut f.body, prefix, strings, map);
                    } else if let Statement::Process(p) = s {
                        mangle_one(&mut p.name, &new_prefix, strings, &mut method_map);
                        mangle_stmts(&mut p.body, prefix, strings, map);
                    }
                }
            }
            Statement::Event(e) => mangle_stmts(&mut e.body, prefix, strings, map),
            Statement::If(i) => {
                mangle_stmts(&mut i.then_body, prefix, strings, map);
                for (_, b) in &mut i.elif_branches {
                    mangle_stmts(b, prefix, strings, map);
                }
                if let Some(b) = &mut i.else_body {
                    mangle_stmts(b, prefix, strings, map);
                }
            }
            Statement::While(w) => mangle_stmts(&mut w.body, prefix, strings, map),
            Statement::For(f) => mangle_stmts(&mut f.body, prefix, strings, map),
            Statement::TypeAlias(ta) => {
                mangle_one(&mut ta.name, prefix, strings, map);
                for alias in &ta.aliases {
                    map.insert(*alias, ta.name);
                }
            }
            Statement::Match(m) => {
                for arm in &mut m.arms {
                    mangle_stmts(&mut arm.body, prefix, strings, map);
                }
            }
            Statement::TryCatch(tc) => {
                mangle_stmts(&mut tc.try_body, prefix, strings, map);
                mangle_stmts(&mut tc.catch_body, prefix, strings, map);
            }
            Statement::Interface(i) => {
                mangle_one(&mut i.name, prefix, strings, map);
                for alias in &i.aliases {
                    map.insert(*alias, i.name);
                }
                let new_prefix = strings.resolve(&i.name).to_owned();
                let mut method_map = HashMap::new();
                for s in &mut i.body {
                    if let Statement::Function(f) = s {
                        mangle_one(&mut f.name, &new_prefix, strings, &mut method_map);
                        mangle_stmts(&mut f.body, prefix, strings, map);
                    }
                }
            }
            _ => {}
        }
    }
}

#[instrument(skip(strings, map), level = "trace")]
fn mangle_one(
    name: &mut StrId,
    prefix: &str,
    strings: &mut Rodeo,
    map: &mut HashMap<StrId, StrId>,
) {
    let s = strings.resolve(name);
    if s.contains("::") {
        trace!(name = %s, "Skipping already-mangled name");
        return;
    }
    let new = strings.get_or_intern(format!("{prefix}::{s}"));
    map.insert(*name, new);
    *name = new;
}

use super::common::{get_variable_name, node_str_eq, references_any_variable};
use crate::ir::StrLit;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use egg::*;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};

fn extract_enum_value(expr: &RecExpr<Mir>, id: Id) -> Option<String> {
    std::iter::once(id)
        .chain(expr[id].children().iter().copied())
        .find_map(|i| {
            if let Mir::Str(s) = &expr[i] {
                let v = s.0.to_string();
                if matches!(v.as_str(), "X" | "Y" | "Z" | "YAW" | "PITCH") {
                    return Some(v);
                }
            }
            None
        })
}

fn structural_eq(expr: &RecExpr<Mir>, a: Id, b: Id) -> bool {
    if a == b {
        return true;
    }
    let (na, nb) = (&expr[a], &expr[b]);
    if std::mem::discriminant(na) != std::mem::discriminant(nb) {
        return false;
    }
    let (ca, cb) = (na.children(), nb.children());
    if ca.len() != cb.len() {
        return false;
    }
    if ca.is_empty() {
        return na == nb;
    }
    ca.iter().zip(cb).all(|(&x, &y)| structural_eq(expr, x, y))
}

fn create_zero_number(nn: &mut NodeArena<Mir>) -> Id {
    nn.add(Mir::Num(OrderedFloat(0.0)))
}

fn extract_action_args_map(expr: &RecExpr<Mir>, id: Id) -> Option<HashMap<String, Id>> {
    let ids = match &expr[id] {
        Mir::Action(ids) if ids.len() >= 4 => ids,
        _ => return None,
    };
    let Mir::List(args) = &expr[ids[3]] else {
        return None;
    };
    let mut map = HashMap::new();
    for &a in args {
        if let Mir::Named([n, v]) = &expr[a]
            && let Mir::Str(s) = &expr[*n]
        {
            map.insert(s.0.as_str().to_owned(), *v);
        }
    }
    Some(map)
}

#[derive(Clone, Copy)]
enum Strategy {
    Read,
    Shift,
    Set,
}

#[derive(Clone, Copy)]
struct PassSpec {
    single: &'static str,
    all: &'static str,
    var: &'static str,
    src: &'static str,
    val: Option<&'static str>,
    typ: &'static str,
    comps: &'static [(&'static str, &'static str)],
    strat: Strategy,
}

const C: &[(&str, &str)] = &[
    ("X", "x"),
    ("Y", "y"),
    ("Z", "z"),
    ("YAW", "yaw"),
    ("PITCH", "pitch"),
];
const V: &[(&str, &str)] = &[("X", "x"), ("Y", "y"), ("Z", "z")];

const PASSES: &[PassSpec] = &[
    PassSpec {
        single: "get_coordinate",
        all: "get_all_coordinates",
        var: "variable",
        src: "location",
        val: None,
        typ: "type",
        comps: C,
        strat: Strategy::Read,
    },
    PassSpec {
        single: "shift_coordinate",
        all: "shift_all_coordinates",
        var: "variable",
        src: "location",
        val: Some("distance"),
        typ: "type",
        comps: C,
        strat: Strategy::Shift,
    },
    PassSpec {
        single: "set_coordinate",
        all: "set_all_coordinates",
        var: "variable",
        src: "location",
        val: Some("coordinate"),
        typ: "type",
        comps: C,
        strat: Strategy::Set,
    },
    PassSpec {
        single: "get_vector_component",
        all: "get_vector_all_components",
        var: "variable",
        src: "vector",
        val: None,
        typ: "vector_component",
        comps: V,
        strat: Strategy::Read,
    },
    PassSpec {
        single: "set_vector_component",
        all: "set_vector",
        var: "variable",
        src: "vector",
        val: Some("value"),
        typ: "vector_component",
        comps: V,
        strat: Strategy::Set,
    },
];

fn is_match(spec: &PassSpec, expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Action(ids) = &expr[id] {
        ids.len() >= 4
            && node_str_eq(&expr[ids[0]], "variable")
            && node_str_eq(&expr[ids[1]], spec.single)
    } else {
        false
    }
}

fn extract_args(
    spec: &PassSpec,
    expr: &RecExpr<Mir>,
    id: Id,
) -> Option<(Id, Id, Option<Id>, String)> {
    let m = extract_action_args_map(expr, id)?;
    let var = m.get(spec.var).copied()?;
    let src = m.get(spec.src).copied()?;
    let typ = m.get(spec.typ).and_then(|&v| extract_enum_value(expr, v))?;
    let val = spec.val.and_then(|f| m.get(f).copied());
    if !matches!(spec.strat, Strategy::Read) && val.is_none() {
        return None;
    }
    Some((var, src, val, typ))
}

fn find_group_end(spec: &PassSpec, expr: &RecExpr<Mir>, ch: &[Id], start: usize) -> usize {
    let Some((fv, fs, fval, ft)) = extract_args(spec, expr, ch[start]) else {
        return start + 1;
    };
    let is_write = !matches!(spec.strat, Strategy::Read);
    let fvn = if is_write {
        let Some(n) = get_variable_name(expr, fv) else {
            return start + 1;
        };
        Some(n)
    } else {
        get_variable_name(expr, fv)
    };

    let mut tv: HashSet<String> = HashSet::new();
    if let Some(n) = &fvn {
        tv.insert(n.to_string());
    }
    if is_write && fval.is_some_and(|v| references_any_variable(expr, v, &tv)) {
        return start + 1;
    }

    let mut seen = HashSet::new();
    seen.insert(ft);
    let mut end = start + 1;

    while end < ch.len() && is_match(spec, expr, ch[end]) {
        let Some((v, s, val, t)) = extract_args(spec, expr, ch[end]) else {
            break;
        };
        if is_write {
            if get_variable_name(expr, v) != fvn {
                break;
            }
            if val.is_some_and(|x| references_any_variable(expr, x, &tv)) {
                break;
            }
            if !(get_variable_name(expr, s) == fvn || structural_eq(expr, fs, s)) {
                break;
            }
        } else {
            if !structural_eq(expr, fs, s) {
                break;
            }
            if references_any_variable(expr, s, &tv) {
                break;
            }
            let Some(vn) = get_variable_name(expr, v) else {
                break;
            };
            if tv.contains(vn.as_str()) {
                break;
            }
            tv.insert(vn.to_string());
        }
        if seen.contains(&t) {
            break;
        }
        seen.insert(t);
        end += 1;
    }
    end
}

fn create_merged_action(
    spec: &PassSpec,
    expr: &RecExpr<Mir>,
    group: &[Id],
    m: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Option<Id> {
    let is_read = matches!(spec.strat, Strategy::Read);
    let is_shift = matches!(spec.strat, Strategy::Shift);

    let mut tv: HashMap<String, Id> = HashMap::new();
    let (mut fv, mut fs) = (None, None);
    for &a in group {
        if let Some((v, s, val, t)) = extract_args(spec, expr, a) {
            let val = if is_read { v } else { val.unwrap() };
            tv.insert(t, val);
            if fv.is_none() {
                fv = Some(v);
                fs = Some(s);
            }
        }
    }

    let fv = fv?;
    let fs = fs?;

    if matches!(spec.strat, Strategy::Set) {
        for &(c, _) in spec.comps {
            if !tv.contains_key(c) {
                return None;
            }
        }
    }

    let mut na = Vec::new();
    if !is_read {
        let ni = nn.add(Mir::Str(StrLit(Symbol::from(spec.var))));
        na.push(nn.add(Mir::Named([ni, m[usize::from(fv)]])));
    }
    if is_shift {
        let ni = nn.add(Mir::Str(StrLit(Symbol::from(spec.src))));
        na.push(nn.add(Mir::Named([ni, m[usize::from(fs)]])));
    }
    for &(c, an) in spec.comps {
        if let Some(&vid) = tv.get(c) {
            let ni = nn.add(Mir::Str(StrLit(Symbol::from(an))));
            na.push(nn.add(Mir::Named([ni, m[usize::from(vid)]])));
        } else if is_shift {
            let z = create_zero_number(nn);
            let ni = nn.add(Mir::Str(StrLit(Symbol::from(an))));
            na.push(nn.add(Mir::Named([ni, z])));
        }
    }
    if is_read {
        let ni = nn.add(Mir::Str(StrLit(Symbol::from(spec.src))));
        na.push(nn.add(Mir::Named([ni, m[usize::from(fs)]])));
    }

    let al = nn.add(Mir::List(na.into_boxed_slice()));
    let oi = nn.add(Mir::Str(StrLit(Symbol::from("variable"))));
    let ni = nn.add(Mir::Str(StrLit(Symbol::from(spec.all))));
    let npi = nn.add(Mir::Nop);

    Some(nn.add(Mir::Action(
        vec![oi, ni, npi, al, npi, npi, npi].into_boxed_slice(),
    )))
}

fn copy_child(expr: &RecExpr<Mir>, c: Id, m: &[Id], nn: &mut NodeArena<Mir>) -> Id {
    let n = &expr[c];
    nn.add(n.clone().map_children(|x| m[usize::from(x)]))
}

fn fold_coordinate_block_children(
    expr: &RecExpr<Mir>,
    ch: &[Id],
    m: &[Id],
    nn: &mut NodeArena<Mir>,
) -> Vec<Id> {
    let mut res = Vec::new();
    let mut i = 0;
    while i < ch.len() {
        if let Some(spec) = PASSES.iter().find(|p| is_match(p, expr, ch[i])) {
            let ge = find_group_end(spec, expr, ch, i);
            if ge - i >= 2 {
                if let Some(na) = create_merged_action(spec, expr, &ch[i..ge], m, nn) {
                    res.push(na);
                } else {
                    for &c in &ch[i..ge] {
                        res.push(copy_child(expr, c, m, nn));
                    }
                }
            } else {
                res.push(copy_child(expr, ch[i], m, nn));
            }
            i = ge;
        } else {
            res.push(copy_child(expr, ch[i], m, nn));
            i += 1;
        }
    }
    res
}

#[must_use]
pub fn fold_coordinates(expr: &RecExpr<Mir>) -> RecExpr<Mir> {
    if expr.is_empty() {
        return expr.clone();
    }
    let mut nn = NodeArena::with_capacity(expr.len());
    let mut m = vec![Id::from(0); expr.len()];
    for (i, n) in expr.as_ref().iter().enumerate() {
        let oid = Id::from(i);
        let nid = if let Mir::Block(c) = n {
            let f = fold_coordinate_block_children(expr, c, &m, &mut nn);
            nn.add(Mir::Block(f.into_boxed_slice()))
        } else {
            nn.add(n.clone().map_children(|x| m[usize::from(x)]))
        };
        m[usize::from(oid)] = nid;
    }
    nn.into_recexpr()
}

#[derive(Debug, Clone, Default)]
pub struct CoordinateFoldingPass;

impl FunctionPass<Mir> for CoordinateFoldingPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        fold_coordinates(expr)
    }
}

use std::collections::{BTreeMap, BTreeSet};

use super::MBASolver;
use crate::{
    expr::{Expr, VarId},
    utils::cache::LinearCache,
};

type Monomial = BTreeSet<VarId>;
type Terms = BTreeMap<Monomial, u64>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct HiddenLiteral {
    var: VarId,
    complemented: bool,
}

fn literal(var: VarId, complemented: bool) -> HiddenLiteral {
    HiddenLiteral { var, complemented }
}

fn literal_views(var: VarId, hidden: &BTreeSet<VarId>) -> impl Iterator<Item = HiddenLiteral> {
    std::iter::once(literal(var, false)).chain(hidden.contains(&var).then_some(literal(var, true)))
}

fn addc(e: Expr, c: u64, mask: u64) -> Expr {
    (e + Expr::make_const(c & mask)).reduce_masked(mask)
}

fn comp(e: Expr, mask: u64) -> Expr {
    (-e - Expr::make_const(1)).reduce_masked(mask)
}

fn add_term(out: &mut Terms, monomial: Monomial, coefficient: u64, mask: u64) {
    let value = out
        .get(&monomial)
        .copied()
        .unwrap_or_default()
        .wrapping_add(coefficient)
        & mask;
    if value == 0 {
        out.remove(&monomial);
    } else {
        out.insert(monomial, value);
    }
}

fn add_terms(out: &mut Terms, other: &Terms, scale: u64, mask: u64) {
    for (monomial, coefficient) in other {
        add_term(out, monomial.clone(), coefficient.wrapping_mul(scale), mask);
    }
}

fn sub_terms(mut out: Terms, other: &Terms, mask: u64) -> Terms {
    add_terms(&mut out, other, mask, mask);
    out
}

fn and_literal(terms: &Terms, literal: HiddenLiteral, mask: u64) -> Terms {
    let mut direct = Terms::new();
    for (monomial, coefficient) in terms {
        let mut product = monomial.clone();
        product.insert(literal.var);
        add_term(&mut direct, product, *coefficient, mask);
    }
    if literal.complemented {
        let mut out = terms.clone();
        add_terms(&mut out, &direct, mask, mask);
        out
    } else {
        direct
    }
}

fn literal_terms(literal: HiddenLiteral, mask: u64) -> Terms {
    let mut out = Terms::new();
    add_term(
        &mut out,
        BTreeSet::from([literal.var]),
        if literal.complemented { mask } else { 1 },
        mask,
    );
    if literal.complemented {
        add_term(&mut out, BTreeSet::new(), 1, mask);
    }
    out
}

fn even_nonconstant_coefficients(e: &Expr, mask: u64) -> bool {
    let e = e.clone().reduce_masked(mask);
    let ts: &[Expr] = match &e {
        Expr::Add(xs) => xs,
        _ => std::slice::from_ref(&e),
    };
    ts.iter().all(|t| match t {
        Expr::Const(_) => true,
        Expr::Scale(c, _) => c & 1 == 0,
        _ => false,
    })
}

fn coeffs(e: &Expr, mask: u64) -> BTreeMap<Expr, u64> {
    let mut out = BTreeMap::<Expr, u64>::new();
    let ts: &[Expr] = match e {
        Expr::Add(xs) => xs,
        _ => std::slice::from_ref(e),
    };
    for t in ts {
        let (c, core) = match t {
            Expr::Scale(c, x) => (*c & mask, x.as_ref().clone()),
            Expr::Const(c) => (*c & mask, Expr::make_const(1)),
            _ => (1, t.clone()),
        };
        let value = out.entry(core).or_default();
        *value = value.wrapping_add(c) & mask;
    }
    out.retain(|_, c| *c != 0);
    out
}

fn odd_inverse(c: u64, mask: u64) -> u64 {
    let c = c & mask;
    debug_assert!(c & 1 == 1);
    let mut inv = c;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u64.wrapping_sub(c.wrapping_mul(inv))) & mask;
    }
    inv
}

fn scale_relation(candidate: &BTreeMap<Expr, u64>, base: &Expr, mask: u64) -> Option<u64> {
    let base = match base {
        Expr::Not(x) => (-x.as_ref().clone() - Expr::make_const(1)).reduce_masked(mask),
        _ => base.clone(),
    };
    let b = coeffs(&base, mask);
    if candidate.keys().ne(b.keys()) {
        return None;
    }
    let (pivot, &coefficient) = b.iter().find(|(_, c)| **c & 1 == 1)?;
    let k = candidate[pivot].wrapping_mul(odd_inverse(coefficient, mask)) & mask;
    b.iter()
        .all(|(x, c)| candidate[x] == c.wrapping_mul(k) & mask)
        .then_some(k)
}

fn monomial(e: &Expr) -> Option<Monomial> {
    match e {
        Expr::Const(1) => Some(Default::default()),
        Expr::Var(v) => Some(BTreeSet::from([*v])),
        Expr::And(xs) => xs
            .iter()
            .map(|x| match x {
                Expr::Var(v) => Some(*v),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn term_map(e: &Expr, mask: u64) -> Option<Terms> {
    let mut out = Terms::new();
    for (core, coefficient) in coeffs(&e.clone().reduce_masked(mask), mask) {
        let monomial = monomial(&core)?;
        let coefficient = if monomial.is_empty() {
            coefficient.wrapping_neg()
        } else {
            coefficient
        };
        add_term(&mut out, monomial, coefficient, mask);
    }
    Some(out)
}

fn linear_terms<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Terms> {
    s.solve_linear(e.reduce_masked(s.mask), false)
        .ok()
        .and_then(|q| term_map(&q, s.mask))
}

fn rank<'a>(m: &'a Monomial, hidden: &BTreeSet<VarId>) -> (usize, usize, &'a Monomial) {
    (m.iter().filter(|v| hidden.contains(v)).count(), m.len(), m)
}

fn quotient_once(
    root: &Terms,
    mut relation: Terms,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) -> Option<(Monomial, Terms, Terms)> {
    let (pivot, coefficient) = relation
        .iter()
        .filter(|(candidate, c)| {
            !candidate.is_empty()
                && **c & 1 == 1
                && relation
                    .keys()
                    .all(|other| *other == **candidate || !candidate.is_subset(other))
        })
        .max_by_key(|(m, _)| rank(m, hidden))
        .map(|(m, c)| (m.clone(), *c))?;
    let inverse = odd_inverse(coefficient, mask);
    for coefficient in relation.values_mut() {
        *coefficient = coefficient.wrapping_mul(inverse) & mask;
    }
    if !root.keys().any(|m| pivot.is_subset(m)) {
        return None;
    }

    let mut candidate = root.clone();
    for (monomial, coefficient) in root {
        if !pivot.is_subset(monomial) {
            continue;
        }
        candidate.remove(monomial);
        let context: Monomial = monomial.difference(&pivot).copied().collect();
        for (replacement, replacement_coefficient) in relation.iter().filter(|(m, _)| **m != pivot)
        {
            add_term(
                &mut candidate,
                context.union(replacement).copied().collect(),
                coefficient
                    .wrapping_mul(*replacement_coefficient)
                    .wrapping_neg(),
                mask,
            );
        }
    }
    Some((pivot, relation, candidate))
}

struct BestCandidate {
    pivot: Monomial,
    relation: Terms,
    terms: Terms,
}

fn consider(
    best: &mut Option<BestCandidate>,
    relation: Terms,
    root: &Terms,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) {
    let Some((pivot, relation, candidate)) = quotient_once(root, relation, hidden, mask) else {
        return;
    };
    let replace = best.as_ref().is_none_or(|old| {
        candidate.len() < old.terms.len()
            || (candidate.len() == old.terms.len()
                && (rank(&pivot, hidden), &relation) > (rank(&old.pivot, hidden), &old.relation))
    });
    if replace {
        *best = Some(BestCandidate {
            pivot,
            relation,
            terms: candidate,
        });
    }
}

fn build(terms: Terms, mask: u64) -> Expr {
    let mut out = Vec::new();
    for (monomial, coefficient) in terms {
        if monomial.is_empty() {
            out.push(Expr::make_const(coefficient.wrapping_neg() & mask));
        } else {
            out.push(Expr::scale(
                coefficient,
                Expr::And(monomial.into_iter().map(Expr::Var).collect()),
            ));
        }
    }
    Expr::Add(out).reduce_masked(mask)
}

fn known<C: LinearCache>(s: &MBASolver<'_, C>, e: &Expr) -> Option<Expr> {
    if let Some(var) = s.non_linear_components.get_by_right(e) {
        return Some(Expr::Var(*var));
    }
    s.non_linear_components
        .get_by_right(&comp(e.clone(), s.mask))
        .map(|var| (!Expr::Var(*var)).reduce_masked(s.mask))
}

fn fold_known<C: LinearCache>(s: &MBASolver<'_, C>, e: Expr) -> Expr {
    let e = e.map(|x| fold_known(s, x)).reduce_masked(s.mask);
    known(s, &e).unwrap_or(e)
}

fn bitwise_view<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = e.reduce_masked(s.mask);
    known(s, &e).or_else(|| {
        s.is_linear(&e)
            .then(|| s.is_linear_bitwise(e.clone(), s.mask))
            .flatten()
            .map(|q| q.reduce_masked(s.mask))
    })
}

fn bitwise_view_refolded<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = e.reduce_masked(s.mask);
    bitwise_view(s, e.clone()).or_else(|| bitwise_view(s, fold_known(s, e)))
}

fn definition<C: LinearCache>(s: &MBASolver<'_, C>, var: VarId) -> Expr {
    s.non_linear_components
        .get_by_left(&var)
        .cloned()
        .unwrap_or(Expr::Var(var))
}

fn resident_literals<C: LinearCache>(
    s: &MBASolver<'_, C>,
    root: &Expr,
) -> Vec<(HiddenLiteral, Expr)> {
    let hidden: BTreeSet<_> = s.non_linear_components.iter().map(|(v, _)| *v).collect();
    let mut vars: BTreeSet<_> = root.get_vars().into_iter().collect();
    for (var, e) in s.non_linear_components.iter() {
        vars.insert(*var);
        vars.extend(e.get_vars());
    }
    vars.into_iter()
        .flat_map(|var| literal_views(var, &hidden))
        .map(|literal| {
            let d = definition(s, literal.var);
            let d = if literal.complemented {
                comp(d, s.mask)
            } else {
                d
            };
            (literal, d)
        })
        .collect()
}

fn valuation_relation<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    observer: HiddenLiteral,
    base: HiddenLiteral,
    predecessor: Expr,
) -> Option<Terms> {
    let mask = s.mask;
    let left = and_literal(&literal_terms(base, mask), observer, mask);
    let predecessor = linear_terms(s, predecessor)?;
    let right = and_literal(&predecessor, observer, mask);
    Some(sub_terms(left, &right, mask))
}

fn subset_premise<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    observer: VarId,
    upper: Expr,
) -> Option<bool> {
    let upper = linear_terms(s, upper)?;
    let observer = literal(observer, false);
    let difference = sub_terms(
        and_literal(&upper, observer, s.mask),
        &literal_terms(observer, s.mask),
        s.mask,
    );
    Some(difference.is_empty())
}

fn subset_relation(observer: VarId, z: VarId, partner: HiddenLiteral, mask: u64) -> Terms {
    let observer = literal(observer, false);
    let z = literal_terms(literal(z, false), mask);
    let lhs = and_literal(&and_literal(&z, partner, mask), observer, mask);
    let rhs = and_literal(&z, observer, mask);
    sub_terms(lhs, &rhs, mask)
}

fn collect_valuation<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    root: &Expr,
    rt: &Terms,
    hidden: &BTreeSet<VarId>,
    residents: &[(HiddenLiteral, Expr)],
    best: &mut Option<BestCandidate>,
) {
    let mask = s.mask;
    let mut root_vars: Vec<_> = root.get_vars().into_iter().collect();
    root_vars.sort_unstable();
    for y in root_vars {
        let dy = definition(s, y);
        if !even_nonconstant_coefficients(&dy, mask) {
            continue;
        }
        for (pred, candidate) in [(false, dy.clone()), (true, addc(dy.clone(), 1, mask))] {
            let candidate_coeffs = coeffs(
                &match &candidate {
                    Expr::Not(x) => (-x.as_ref().clone() - Expr::make_const(1)).reduce_masked(mask),
                    _ => candidate.clone(),
                },
                mask,
            );
            let mut valid = BTreeMap::<VarId, (HiddenLiteral, Expr)>::new();
            for (literal, definition) in residents {
                let Some(k) = scale_relation(&candidate_coeffs, definition, mask) else {
                    continue;
                };
                if k & 1 != 0 {
                    continue;
                }
                valid
                    .entry(literal.var)
                    .and_modify(|best| {
                        if *literal < best.0 {
                            *best = (*literal, definition.clone());
                        }
                    })
                    .or_insert((*literal, definition.clone()));
            }
            for (_, (base, definition)) in valid {
                let Some(predecessor) = bitwise_view_refolded(s, addc(definition, mask, mask))
                else {
                    continue;
                };
                let Some(relation) = valuation_relation(s, literal(y, pred), base, predecessor)
                else {
                    continue;
                };
                consider(best, relation, rt, hidden, mask);
            }
        }
    }
}

fn collect_subset_cuts<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    rt: &Terms,
    hidden: &BTreeSet<VarId>,
    residents: &[(HiddenLiteral, Expr)],
    best: &mut Option<BestCandidate>,
) {
    let mask = s.mask;
    let mut view_cache = BTreeMap::<VarId, Option<Expr>>::new();
    let mut subset_cache = BTreeMap::<(VarId, VarId), bool>::new();
    for small in rt.keys() {
        for x in small {
            let dx = definition(s, *x);
            let Some((n, _)) = residents
                .iter()
                .filter(|(_, d)| *d == (-dx.clone()).reduce_masked(mask))
                .filter(|(literal, _)| {
                    let mut big = small.clone();
                    big.insert(literal.var);
                    rt.contains_key(&big)
                })
                .min_by_key(|(literal, _)| *literal)
            else {
                continue;
            };
            for z in small.intersection(hidden).copied() {
                let b = view_cache
                    .entry(z)
                    .or_insert_with(|| {
                        bitwise_view_refolded(s, (-definition(s, z)).reduce_masked(mask))
                    })
                    .clone();
                let Some(b) = b else {
                    continue;
                };
                let subset = *subset_cache
                    .entry((*x, z))
                    .or_insert_with(|| subset_premise(s, *x, b).unwrap_or(false));
                if subset {
                    consider(best, subset_relation(*x, z, *n, mask), rt, hidden, mask);
                }
            }
        }
    }
}

pub(super) fn close<C: LinearCache>(s: &mut MBASolver<'_, C>, root: Expr) -> Expr {
    let Some(root_terms) = term_map(&root, s.mask) else {
        return root;
    };
    let hidden: BTreeSet<_> = s.non_linear_components.iter().map(|(v, _)| *v).collect();
    let residents = resident_literals(s, &root);
    let mut best = None;
    collect_valuation(s, &root, &root_terms, &hidden, &residents, &mut best);
    collect_subset_cuts(s, &root_terms, &hidden, &residents, &mut best);
    best.map(|candidate| build(candidate.terms, s.mask))
        .unwrap_or(root)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{utils::cache::LocalCache, varint::make_mask};

    fn literal_expr(literal: HiddenLiteral, mask: u64) -> Expr {
        if literal.complemented {
            (!Expr::Var(literal.var)).reduce_masked(mask)
        } else {
            Expr::Var(literal.var)
        }
    }

    #[test]
    fn empty_monomial_convention_is_stable() {
        let mask = make_mask(8);
        let x = Expr::Var(VarId(0));
        let c = 3u64;
        let relation = (7u64 * x + Expr::make_const(c)).reduce_masked(mask);
        let terms = term_map(&relation, mask).unwrap();
        let empty = BTreeSet::new();

        assert_eq!(terms[&empty], c.wrapping_neg() & mask);
        assert_eq!(build(terms.clone(), mask), relation);
        assert_eq!(
            term_map(&build(terms, mask), mask).unwrap()[&empty],
            c.wrapping_neg() & mask
        );
    }

    #[test]
    fn free_relation_does_not_use_hidden_definitions() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let h = Expr::Var(VarId(1));
        let original = h.clone() - (x.clone() - Expr::make_const(1));
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &original, 4);
        solver.non_linear_components.insert(
            VarId(1),
            (x.clone() - Expr::make_const(1)).reduce_masked(mask),
        );

        let result = build(linear_terms(&mut solver, original.clone()).unwrap(), mask);
        assert_ne!(result, Expr::zero());
        for xv in 0..16u64 {
            for hv in 0..16u64 {
                let vars = [xv, hv];
                assert_eq!(
                    result.eval(&vars, 4),
                    original.eval(&vars, 4),
                    "x={xv}, h={hv}"
                );
            }
        }
    }

    #[test]
    fn rule_selection_is_discovery_order_invariant() {
        let hidden = BTreeSet::from([VarId(2)]);
        let pivot = BTreeSet::from([VarId(2)]);
        let extra = BTreeSet::from([VarId(0)]);
        let a = BTreeMap::from([(BTreeSet::new(), 1), (pivot.clone(), 1)]);
        let b = BTreeMap::from([(BTreeSet::new(), 1), (pivot.clone(), 1), (extra, 1)]);

        let root = BTreeMap::from([(pivot.clone(), 1)]);
        let mut ab = None;
        consider(&mut ab, a.clone(), &root, &hidden, make_mask(4));
        consider(&mut ab, b.clone(), &root, &hidden, make_mask(4));
        let mut ba = None;
        consider(&mut ba, b, &root, &hidden, make_mask(4));
        consider(&mut ba, a, &root, &hidden, make_mask(4));

        assert_eq!(ab.unwrap().terms, ba.unwrap().terms);
    }

    #[test]
    fn cut_congruence_is_sound_under_nonempty_context() {
        let mask = make_mask(4);
        let xm1 = Expr::Var(VarId(0));
        let x = Expr::Var(VarId(1));
        let y = Expr::Var(VarId(2));
        let z = Expr::Var(VarId(3));
        let w = Expr::Var(VarId(4));
        let context = z.clone() & w.clone();
        // `xm1` is the bitwise X-1 view supplied by the valuation producer;
        // only X, Z, and W are independent in the exhaustive check below.
        let root = (context & y.clone() & x.clone()).reduce_masked(mask);
        let root_terms = term_map(&root, mask).unwrap();
        let hidden = BTreeSet::from([VarId(2)]);
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &root, 4);
        let mut best = None;

        let observer = literal(VarId(2), false);
        let base = literal(VarId(1), false);
        let relation = valuation_relation(&mut solver, observer, base, xm1).unwrap();
        consider(&mut best, relation, &root_terms, &hidden, mask);
        let applied = build(best.unwrap().terms, mask);
        assert_ne!(applied, root);

        for xv in 0..16u64 {
            for zv in 0..16u64 {
                for wv in 0..16u64 {
                    let vars = [xv.wrapping_sub(1) & mask, xv, (2 * xv) & mask, zv, wv];
                    assert_eq!(
                        root.eval(&vars, 4),
                        applied.eval(&vars, 4),
                        "x={xv}, z={zv}, w={wv}"
                    );
                }
            }
        }
    }

    #[test]
    fn empty_monomial_contextualization_is_sound() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let z = Expr::Var(VarId(2));
        let w = Expr::Var(VarId(3));
        let context = x.clone() & z.clone() & w.clone();
        let root = context & y.clone();
        let root_terms = term_map(&root, mask).unwrap();
        let hidden = BTreeSet::from([VarId(1)]);
        let mut relation = literal_terms(literal(1.into(), false), mask);
        add_term(&mut relation, BTreeSet::new(), mask, mask);
        let mut best = None;

        consider(&mut best, relation, &root_terms, &hidden, mask);
        let best = best.unwrap();
        assert!(
            best.relation
                .get(&BTreeSet::new())
                .is_some_and(|coefficient| *coefficient != 0)
        );
        let applied = build(best.terms, mask);

        for xv in 0..16u64 {
            for zv in 0..16u64 {
                for wv in 0..16u64 {
                    let yv = mask;
                    let vars = [xv, yv, zv, wv];
                    assert_eq!(
                        root.eval(&vars, 4),
                        applied.eval(&vars, 4),
                        "x={xv}, z={zv}, w={wv}"
                    );
                }
            }
        }
    }

    #[test]
    fn lower_ranked_admissible_pivot_is_selected() {
        let mask = make_mask(4);
        let lower = BTreeSet::from([VarId(4)]);
        let relation = (Expr::Var(VarId(1)) & Expr::Var(VarId(2)))
            + (2u64 * (Expr::Var(VarId(1)) & Expr::Var(VarId(2)) & Expr::Var(VarId(3))))
            + Expr::Var(VarId(4));
        let root = BTreeMap::from([(lower.clone(), 1)]);
        let hidden = BTreeSet::from([VarId(1), VarId(2), VarId(4)]);

        let terms = term_map(&relation, mask).unwrap();
        let (pivot, _, _) = quotient_once(&root, terms, &hidden, mask).unwrap();
        assert_eq!(pivot, lower);
    }

    #[test]
    fn valuation_pred_true_is_sound() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let context = Expr::Var(VarId(2)) & Expr::Var(VarId(3));
        let observer = (!y.clone()).reduce_masked(mask);
        let lhs = x.clone();
        let rhs = (x.clone() - Expr::make_const(1)).reduce_masked(mask);
        let left = context.clone() & observer.clone() & lhs;
        let right = context & observer & rhs;

        for xv in 0..16u64 {
            for zv in 0..16u64 {
                for wv in 0..16u64 {
                    let yv = xv.wrapping_mul(2).wrapping_sub(1) & mask;
                    let vars = [xv, yv, zv, wv];
                    assert_eq!(left.eval(&vars, 4), right.eval(&vars, 4));
                }
            }
        }
    }

    #[test]
    fn subset_cut_survives_hidden_polarity_flip() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let v2 = Expr::Var(VarId(2));
        let v3 = Expr::Var(VarId(3));
        let v4 = Expr::Var(VarId(4));
        let hidden = BTreeSet::from([VarId(2), VarId(3), VarId(4)]);
        let root =
            (Expr::make_const(mask) + x.clone() - v4.clone() + (v2 & v4 & v3)).reduce_masked(mask);
        let root_terms = term_map(&root, mask).unwrap();

        let run = |v4_definition| {
            let defs = BTreeMap::from([
                (VarId(0), x.clone()),
                (VarId(1), Expr::Var(VarId(1))),
                (VarId(2), x.clone()),
                (VarId(3), Expr::make_const(1)),
                (VarId(4), v4_definition),
            ]);
            let cache = LocalCache::new();
            let mut solver = MBASolver::new(&cache, &root, 4);
            for (variable, definition) in defs
                .iter()
                .filter(|(variable, _)| hidden.contains(variable))
            {
                solver
                    .non_linear_components
                    .insert(*variable, definition.clone());
            }
            let residents = resident_literals(&solver, &root);
            let mut best = None;
            collect_subset_cuts(&mut solver, &root_terms, &hidden, &residents, &mut best);
            best.map(|candidate| build(candidate.terms, mask))
        };

        let pre = run((x.clone() - Expr::make_const(1)).reduce_masked(mask))
            .expect("pre-gauge subset cut must be found");
        let gauge = run(comp(x.clone() - Expr::make_const(1), mask))
            .expect("gauge subset cut must be found");
        assert_ne!(pre, root);
        assert_ne!(gauge, root);

        for xv in 0..16u64 {
            let pre_vars = [xv, 0, xv, 1, xv.wrapping_sub(1) & mask];
            let gauge_vars = [xv, 0, xv, 1, xv.wrapping_neg() & mask];
            assert_eq!(
                pre.eval(&pre_vars, 4),
                root.eval(&pre_vars, 4),
                "pre x={xv}"
            );
            assert_eq!(
                gauge.eval(&gauge_vars, 4),
                root.eval(&gauge_vars, 4),
                "gauge x={xv}"
            );
        }
    }

    #[test]
    fn subset_partner_lookup_is_hidden_polarity_invariant() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let defs = BTreeMap::from([
            (VarId(2), x.clone()),
            (
                VarId(4),
                (x.clone() - Expr::make_const(1)).reduce_masked(mask),
            ),
            (VarId(7), (-x.clone()).reduce_masked(mask)),
        ]);
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &x, 4);
        for (variable, definition) in &defs {
            solver
                .non_linear_components
                .insert(*variable, definition.clone());
        }
        let literals = resident_literals(&solver, &x)
            .iter()
            .filter(|(_, definition)| *definition == (-defs[&VarId(2)].clone()).reduce_masked(mask))
            .map(|(literal, _)| *literal)
            .collect::<Vec<_>>();

        assert_eq!(
            literals,
            vec![literal(VarId(4), true), literal(VarId(7), false)]
        );
    }

    #[test]
    fn valuation_cut_survives_hidden_polarity_flip() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        let v2 = Expr::Var(VarId(2));
        let v3 = Expr::Var(VarId(3));
        let root =
            ((v2.clone() & v3.clone()) + (v3.clone() & Expr::Var(VarId(4)))).reduce_masked(mask);
        let root_terms = term_map(&root, mask).unwrap();
        let hidden = BTreeSet::from([VarId(2), VarId(3), VarId(4)]);

        let run = |base_definition| {
            let defs = BTreeMap::from([
                (VarId(0), x.clone()),
                (VarId(2), base_definition),
                (VarId(3), 2u64 * x.clone()),
                (
                    VarId(4),
                    (x.clone() - Expr::make_const(1)).reduce_masked(mask),
                ),
            ]);
            let cache = LocalCache::new();
            let mut solver = MBASolver::new(&cache, &root, 4);
            for (variable, definition) in &defs {
                solver
                    .non_linear_components
                    .insert(*variable, definition.clone());
            }
            let residents = resident_literals(&solver, &root);
            let mut best = None;
            collect_valuation(
                &mut solver,
                &root,
                &root_terms,
                &hidden,
                &residents,
                &mut best,
            );
            best.map(|candidate| build(candidate.terms, mask))
        };

        let pre = run(x.clone()).expect("pre-gauge valuation cut must be found");
        let gauge = run(comp(x.clone(), mask)).expect("gauge valuation cut must be found");
        assert_ne!(pre, root);
        assert_ne!(gauge, root);

        for xv in 0..16u64 {
            let pre_vars = [xv, 0, xv, (2 * xv) & mask, xv.wrapping_sub(1) & mask];
            let gauge_vars = [
                xv,
                0,
                (!xv) & mask,
                (2 * xv) & mask,
                xv.wrapping_sub(1) & mask,
            ];
            assert_eq!(
                pre.eval(&pre_vars, 4),
                root.eval(&pre_vars, 4),
                "pre x={xv}"
            );
            assert_eq!(
                gauge.eval(&gauge_vars, 4),
                root.eval(&gauge_vars, 4),
                "gauge x={xv}"
            );
        }
    }

    #[test]
    fn and_literal_handles_direct_complement_and_collisions() {
        let mask = make_mask(8);
        let empty = BTreeSet::new();
        let x = BTreeSet::from([VarId(0)]);
        let y = BTreeSet::from([VarId(1)]);
        let xy = BTreeSet::from([VarId(0), VarId(1)]);
        let terms = BTreeMap::from([(empty.clone(), 5), (x.clone(), 7), (y.clone(), 3)]);

        assert_eq!(
            and_literal(&terms, literal(VarId(0), false), mask),
            BTreeMap::from([(x.clone(), 12), (xy.clone(), 3)])
        );
        assert_eq!(
            and_literal(&terms, literal(VarId(0), true), mask),
            BTreeMap::from([(empty, 5), (x, 251), (y, 3), (xy, 253)])
        );
    }

    #[test]
    fn native_valuation_lowering_matches_solver_on_hostile_forms() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let z = Expr::Var(VarId(2));
        let forms = [
            x.clone(),
            !x.clone(),
            x.clone() & y.clone(),
            !((x.clone() ^ !y.clone()) | (y & z)),
        ];

        for (mask, bits) in [(15, 4), (u64::MAX, 64)] {
            for observer_complemented in [false, true] {
                for base_complemented in [false, true] {
                    for predecessor in &forms {
                        let observer = literal(VarId(0), observer_complemented);
                        let base = literal(VarId(1), base_complemented);
                        let free_expr = (literal_expr(observer, mask) & literal_expr(base, mask))
                            - (literal_expr(observer, mask) & predecessor.clone());
                        let free_cache = LocalCache::new();
                        let mut free_solver = MBASolver::new(&free_cache, &free_expr, bits);
                        let free = free_solver
                            .solve_linear(free_expr, false)
                            .ok()
                            .and_then(|e| term_map(&e, mask));

                        let new = valuation_relation(
                            &mut free_solver,
                            observer,
                            base,
                            predecessor.clone(),
                        );
                        assert_eq!(free, new, "mask={mask:#x}, predecessor={predecessor}");
                    }
                }
            }
        }
    }

    #[test]
    fn native_subset_lowering_handles_polarity_and_collisions() {
        let cases = [
            (VarId(0), VarId(0), VarId(1)),
            (VarId(0), VarId(1), VarId(0)),
            (VarId(0), VarId(1), VarId(1)),
        ];

        for (mask, bits) in [(15, 4), (u64::MAX, 64)] {
            for (observer, z, partner_var) in cases {
                for complemented in [false, true] {
                    let partner = literal(partner_var, complemented);
                    let free_expr = (Expr::Var(observer)
                        & (Expr::Var(z) & literal_expr(partner, mask)))
                        - (Expr::Var(observer) & Expr::Var(z));
                    let free_cache = LocalCache::new();
                    let mut free_solver = MBASolver::new(&free_cache, &free_expr, bits);
                    let free = free_solver
                        .solve_linear(free_expr, false)
                        .ok()
                        .and_then(|e| term_map(&e, mask));
                    let new = subset_relation(observer, z, partner, mask);
                    assert_eq!(free, Some(new), "mask={mask:#x}, partner={partner:?}");
                }
            }
        }
    }
}

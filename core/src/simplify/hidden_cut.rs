//! Hidden Cut simplifies the current expression using relations carried by hidden variables.
//!
//! It finds a bitwise equality guaranteed by a hidden definition—either because one value
//! is a non-zero even multiple of another, or because every set bit of one value is also
//! present in another. The equality is turned into an ordinary linear relation, applied
//! to the current query, and then normal simplification continues.

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

impl HiddenLiteral {
    fn expr(self, mask: u64) -> Expr {
        if self.complemented {
            (!Expr::Var(self.var)).reduce_masked(mask)
        } else {
            Expr::Var(self.var)
        }
    }

    fn definition(self, defs: &BTreeMap<VarId, Expr>, mask: u64) -> Option<Expr> {
        defs.get(&self.var).map(|d| {
            if self.complemented {
                comp(d.clone(), mask)
            } else {
                d.clone()
            }
        })
    }
}

fn literal_views(var: VarId, hidden: &BTreeSet<VarId>) -> impl Iterator<Item = HiddenLiteral> {
    let direct = HiddenLiteral {
        var,
        complemented: false,
    };
    let complemented = HiddenLiteral {
        var,
        complemented: true,
    };
    std::iter::once(direct).chain(hidden.contains(&var).then_some(complemented))
}

struct LiteralEntry {
    literal: HiddenLiteral,
    definition: Expr,
}

struct LiteralIndexes {
    population: Vec<LiteralEntry>,
    by_definition: BTreeMap<Expr, Vec<usize>>,
    by_support_key: BTreeMap<Vec<Expr>, Vec<usize>>,
}

// `Terms` lives in Z/2^n[x]/(x^2=x). The empty monomial maps to ~0 = -1;
// `term_map` and `build` use opposite signs for that coordinate.
//
// Hidden Cut has one semantic primitive:
//
//     O & (U xor V) = 0  =>  O & U = O & V.
//
// Discovery has two independent certificates for that invariant:
//
// - valuation/predecessor:
//       U = X, V = X-1,
//       U xor V = C_X = X xor (X-1) = 2*(X & -X)-1;
//
// - Boolean order/subset:
//       A subset B  =>  A&(-B) = A&(-A)&(-B).
//
// `emit_cut_relation` is deliberately agnostic to which certificate produced
// the `MaskStableCongruence`.
//
// Hidden Cut is a generic engine for these mask-stable congruences. It currently
// has two certificate producers: valuation/even-multiple predecessors and
// Boolean subset order. A future theorem must first prove a
// `MaskStableCongruence` and can then reuse the same contextual quotient.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    pivot: Monomial,
    terms: Terms,
}

/// A congruence that remains valid under every WORD mask/context:
///
/// ```text
/// for every context C: C & observer & lhs == C & observer & rhs
/// ```
///
/// Equivalently, `observer & (lhs xor rhs) == 0`. Construction is restricted
/// to the certified hidden-cut valuation/order producers. In particular, the
/// empty monomial is safe in contextual substitution because `~0 & C == C`.
struct MaskStableCongruence {
    observer: Expr,
    lhs: Expr,
    rhs: Expr,
}

fn addc(e: Expr, c: u64, mask: u64) -> Expr {
    (e + Expr::make_const(c & mask)).reduce_masked(mask)
}

fn neg(e: Expr, mask: u64) -> Expr {
    (-e).reduce_masked(mask)
}

fn comp(e: Expr, mask: u64) -> Expr {
    (-e - Expr::make_const(1)).reduce_masked(mask)
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
    let mut out = BTreeMap::new();
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
        let q = out.entry(core).or_insert(0u64);
        *q = q.wrapping_add(c) & mask;
    }
    out.retain(|_, c| *c != 0);
    out
}

fn arithmetic(e: &Expr, mask: u64) -> Expr {
    match e {
        Expr::Not(x) => (-x.as_ref().clone() - Expr::make_const(1)).reduce_masked(mask),
        _ => e.clone(),
    }
}

fn support_key(e: &Expr, mask: u64) -> Vec<Expr> {
    coeffs(&arithmetic(e, mask), mask).into_keys().collect()
}

fn odd_inverse(c: u64, mask: u64) -> u64 {
    let c = c & mask;
    debug_assert!(c & 1 == 1);
    let mut inv = c;
    // For odd c, c*c is already 1 modulo 8. Each Newton step doubles the
    // number of correct bits, so five steps cover at least 96 bits.
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u64.wrapping_sub(c.wrapping_mul(inv))) & mask;
    }
    inv
}

/// The candidate coefficient map equals `k*base` modulo 2^n. An odd base
/// coefficient supplies the pivot.
fn scale_relation(candidate: &BTreeMap<Expr, u64>, base: &Expr, mask: u64) -> Option<u64> {
    let a = candidate;
    let b = coeffs(&arithmetic(base, mask), mask);
    if a.keys().ne(b.keys()) {
        return None;
    }
    let (pivot, &c) = b.iter().find(|(_, c)| **c & 1 == 1)?;
    let inv = odd_inverse(c, mask);
    let k = a[pivot].wrapping_mul(inv) & mask;
    b.iter()
        .all(|(x, c)| a[x] == c.wrapping_mul(k) & mask)
        .then_some(k)
}

fn monomial(e: &Expr) -> Option<Monomial> {
    match e {
        Expr::Const(1) => Some(BTreeSet::new()),
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
    let mut out = BTreeMap::new();
    for (core, c) in coeffs(&e.clone().reduce_masked(mask), mask) {
        let m = monomial(&core)?;
        // Terms use the substitution convention in which the empty monomial
        // is the negated arithmetic constant. `build` applies the inverse
        // conversion below; both signs are intentional and must stay paired.
        let c = if m.is_empty() {
            c.wrapping_neg() & mask
        } else {
            c & mask
        };
        let v = out.get(&m).copied().unwrap_or(0u64).wrapping_add(c) & mask;
        if v == 0 {
            out.remove(&m);
        } else {
            out.insert(m, v);
        }
    }
    Some(out)
}

fn rank<'a>(m: &'a Monomial, hidden: &BTreeSet<VarId>) -> (usize, usize, &'a Monomial) {
    (m.iter().filter(|v| hidden.contains(v)).count(), m.len(), m)
}

fn normalize_rule(rel: Expr, root: &Terms, hidden: &BTreeSet<VarId>, mask: u64) -> Option<Rule> {
    let mut ts = term_map(&rel, mask)?;
    let (pivot, pc) = ts
        .iter()
        .filter(|(candidate, c)| {
            !candidate.is_empty()
                && **c & 1 == 1
                && ts
                    .keys()
                    .all(|other| *other == **candidate || !candidate.is_subset(other))
        })
        .max_by_key(|(m, _)| rank(m, hidden))
        .map(|(m, c)| (m.clone(), *c))?;
    let inv = odd_inverse(pc, mask);
    for c in ts.values_mut() {
        *c = c.wrapping_mul(inv) & mask;
    }
    root.keys()
        .any(|m| pivot.is_subset(m))
        .then_some(Rule { pivot, terms: ts })
}

fn rule_key<'a>(
    rule: &'a Rule,
    hidden: &BTreeSet<VarId>,
) -> ((usize, usize, &'a Monomial), &'a Terms) {
    (rank(&rule.pivot, hidden), &rule.terms)
}

fn consider(rules: &mut Vec<Rule>, rel: Expr, root: &Terms, hidden: &BTreeSet<VarId>, mask: u64) {
    let Some(rule) = normalize_rule(rel, root, hidden, mask) else {
        return;
    };
    rules.push(rule);
}

fn build(ts: Terms, mask: u64) -> Expr {
    let mut out = Vec::new();
    for (m, c) in ts {
        if m.is_empty() {
            // Inverse of the empty-monomial convention used by `term_map`.
            let c = c.wrapping_neg() & mask;
            out.push(Expr::make_const(c));
            continue;
        }
        let xs = m.into_iter().map(Expr::Var).collect();
        out.push(Expr::scale(c, Expr::And(xs)));
    }
    Expr::Add(out).reduce_masked(mask)
}

/// One finite substitution only; there is no hidden-cut reduction loop or work budget.
fn apply_rule(root_terms: &Terms, rule: &Rule, mask: u64) -> Expr {
    let mut out = root_terms.clone();
    for (m, c) in root_terms {
        if !rule.pivot.is_subset(m) {
            continue;
        }
        out.remove(m);
        let ctx: Monomial = m.difference(&rule.pivot).copied().collect();
        for (rm, rc) in &rule.terms {
            if *rm == rule.pivot {
                continue;
            }
            let nm: Monomial = ctx.union(rm).copied().collect();
            let v = out
                .get(&nm)
                .copied()
                .unwrap_or(0u64)
                .wrapping_sub(c.wrapping_mul(*rc))
                & mask;
            if v == 0 {
                out.remove(&nm);
            } else {
                out.insert(nm, v);
            }
        }
    }
    build(out, mask)
}

fn choose_rule(
    root_terms: &Terms,
    rules: Vec<Rule>,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) -> Option<Expr> {
    let mut best: Option<(usize, Rule, Expr)> = None;
    for rule in rules {
        let applied = apply_rule(root_terms, &rule, mask);
        let size = term_map(&applied, mask).map_or(usize::MAX, |terms| terms.len());
        let replace = best.as_ref().is_none_or(|(old_size, old_rule, _)| {
            size < *old_size
                || size == *old_size && rule_key(&rule, hidden) > rule_key(old_rule, hidden)
        });
        if replace {
            best = Some((size, rule, applied));
        }
    }
    best.map(|(_, _, applied)| applied)
}

fn known<C: LinearCache>(s: &MBASolver<'_, C>, e: &Expr) -> Option<Expr> {
    if let Some(v) = s.non_linear_components.get_by_right(e) {
        return Some(Expr::Var(*v));
    }
    s.non_linear_components
        .get_by_right(&comp(e.clone(), s.mask))
        .map(|v| (!Expr::Var(*v)).reduce_masked(s.mask))
}

fn fold_known<C: LinearCache>(s: &MBASolver<'_, C>, e: Expr) -> Expr {
    let e = e.map(|x| fold_known(s, x)).reduce_masked(s.mask);
    known(s, &e).unwrap_or(e)
}

fn bitwise_view<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = e.reduce_masked(s.mask);
    if let Some(q) = known(s, &e) {
        return Some(q);
    }
    if s.is_linear(&e) {
        if let Some(q) = s.is_linear_bitwise(e.clone(), s.mask) {
            return Some(q.reduce_masked(s.mask));
        }
    }
    None
}

fn bitwise_view_refolded<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = e.reduce_masked(s.mask);
    if let Some(q) = bitwise_view(s, e.clone()) {
        return Some(q);
    }
    let e = fold_known(s, e);
    bitwise_view(s, e)
}

/// SOUNDNESS BOUNDARY
///
/// Upstream producers may inspect `non_linear_components`. `known`,
/// `fold_known`, `bitwise_view`, and `is_linear_bitwise` may use resident
/// hidden definitions to construct valid representatives `O`, `U`, and `V`
/// on the current hidden variety.
///
/// This function is different: it treats every `VarId` as an independent
/// free WORD variable and must not use `non_linear_components` as equations
/// when proving the relation. The quotient later contextualizes the identity
/// under arbitrary AND contexts, so a relation valid only on the hidden
/// variety is not sufficient.
fn certify_free_relation<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    s.solve_linear(e.reduce_masked(s.mask), false)
        .ok()
        .map(|q| q.reduce_masked(s.mask))
}

fn emit_cut_relation<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    cut: MaskStableCongruence,
    rt: &Terms,
    hidden: &BTreeSet<VarId>,
    rules: &mut Vec<Rule>,
) {
    // Only a proven hidden-cut MaskStableCongruence may be contextualized by `apply_rule`.
    // A generic WORD equality returned by `solve_linear` is not sufficient:
    // AND-contextualization is valid here because Hidden Cut established
    // O & (U xor V) = 0 and emits O&U = O&V.
    let MaskStableCongruence { observer, lhs, rhs } = cut;
    if let Some(r) = certify_free_relation(s, (observer.clone() & lhs) - (observer & rhs)) {
        consider(rules, r, rt, hidden, s.mask);
    }
}

fn definitions<C: LinearCache>(
    s: &MBASolver<'_, C>,
    root: &Expr,
) -> (BTreeMap<VarId, Expr>, BTreeSet<VarId>) {
    let hidden: BTreeSet<_> = s.non_linear_components.iter().map(|(v, _)| *v).collect();
    let mut defs: BTreeMap<_, _> = s
        .non_linear_components
        .iter()
        .map(|(v, e)| (*v, e.clone().reduce_masked(s.mask)))
        .collect();
    let mut vars: BTreeSet<_> = root.get_vars().into_iter().collect();
    for e in defs.values() {
        vars.extend(e.get_vars());
    }
    for v in vars {
        defs.entry(v).or_insert_with(|| Expr::Var(v));
    }
    (defs, hidden)
}

fn literal_indexes(
    defs: &BTreeMap<VarId, Expr>,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) -> LiteralIndexes {
    let mut indexes = LiteralIndexes {
        population: Vec::new(),
        by_definition: BTreeMap::new(),
        by_support_key: BTreeMap::new(),
    };
    for (v, _d) in defs {
        for literal in literal_views(*v, hidden) {
            let definition = literal
                .definition(defs, mask)
                .expect("literal has definition");
            let id = indexes.population.len();
            indexes
                .by_definition
                .entry(definition.clone())
                .or_default()
                .push(id);
            let key = support_key(&definition, mask);
            if !key.is_empty() {
                indexes.by_support_key.entry(key).or_default().push(id);
            }
            indexes.population.push(LiteralEntry {
                literal,
                definition,
            });
        }
    }
    indexes
}

fn collect_valuation_from_buckets<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    root: &Expr,
    rt: &Terms,
    defs: &BTreeMap<VarId, Expr>,
    hidden: &BTreeSet<VarId>,
    indexes: &LiteralIndexes,
    rules: &mut Vec<Rule>,
) {
    let mask = s.mask;
    let mut root_vars: Vec<_> = root.get_vars().into_iter().collect();
    root_vars.sort_unstable();
    for y in root_vars {
        let Some(dy) = defs.get(&y).cloned() else {
            continue;
        };
        if !even_nonconstant_coefficients(&dy, mask) {
            continue;
        }
        for (pred, candidate) in [(false, dy.clone()), (true, addc(dy.clone(), 1, mask))] {
            let candidate_coeffs = coeffs(&arithmetic(&candidate, mask), mask);
            let candidate_key = candidate_coeffs.keys().cloned().collect::<Vec<_>>();
            let Some(xs) = indexes.by_support_key.get(&candidate_key) else {
                continue;
            };
            let mut valid_by_var = BTreeMap::<VarId, &LiteralEntry>::new();
            #[cfg(debug_assertions)]
            let mut applicable_by_var = BTreeMap::<VarId, usize>::new();
            for id in xs {
                let base = &indexes.population[*id];
                let Some(k) = scale_relation(&candidate_coeffs, &base.definition, mask) else {
                    continue;
                };
                if k & 1 != 0 {
                    continue;
                }
                #[cfg(debug_assertions)]
                {
                    *applicable_by_var.entry(base.literal.var).or_default() += 1;
                }
                valid_by_var
                    .entry(base.literal.var)
                    .and_modify(|best| {
                        if base.literal < best.literal {
                            *best = base;
                        }
                    })
                    .or_insert(base);
            }
            #[cfg(debug_assertions)]
            for (var, count) in applicable_by_var {
                debug_assert!(
                    count <= 1,
                    "hidden-gauge complement-orbit uniqueness violated for v{}: {} applicable representatives",
                    var,
                    count
                );
            }
            for (_var, base) in valid_by_var {
                let Some(predecessor) =
                    bitwise_view_refolded(s, addc(base.definition.clone(), mask, mask))
                else {
                    continue;
                };
                let observer = if pred {
                    (!Expr::Var(y)).reduce_masked(mask)
                } else {
                    Expr::Var(y)
                };
                emit_cut_relation(
                    s,
                    MaskStableCongruence {
                        observer,
                        lhs: base.literal.expr(mask),
                        rhs: predecessor,
                    },
                    rt,
                    hidden,
                    rules,
                );
            }
        }
    }
}

fn collect_subset_cuts<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    rt: &Terms,
    defs: &BTreeMap<VarId, Expr>,
    hidden: &BTreeSet<VarId>,
    indexes: &LiteralIndexes,
    rules: &mut Vec<Rule>,
) {
    let mask = s.mask;
    let mut view_cache = BTreeMap::<VarId, Option<Expr>>::new();
    let mut subset_cache = BTreeMap::<(VarId, VarId), bool>::new();

    for small in rt.keys() {
        for x in small {
            let Some(dx) = defs.get(x) else {
                continue;
            };
            let Some(candidates) = indexes.by_definition.get(&neg(dx.clone(), mask)) else {
                continue;
            };
            let Some(n) = candidates
                .iter()
                .map(|id| &indexes.population[*id])
                .filter(|entry| {
                    let mut big = small.clone();
                    big.insert(entry.literal.var);
                    rt.contains_key(&big)
                })
                .min_by_key(|entry| entry.literal)
            else {
                continue;
            };

            for z in small.intersection(hidden).copied() {
                let b = if let Some(view) = view_cache.get(&z) {
                    view.clone()
                } else {
                    let view = defs
                        .get(&z)
                        .and_then(|dz| bitwise_view_refolded(s, neg(dz.clone(), mask)));
                    view_cache.insert(z, view.clone());
                    view
                };
                let Some(b) = b else {
                    continue;
                };
                // Boolean support order: A subset B iff A&B=A.
                let subset = if let Some(subset) = subset_cache.get(&(*x, z)) {
                    *subset
                } else {
                    let observer = Expr::Var(*x);
                    let subset =
                        certify_free_relation(s, (observer.clone() & b.clone()) - observer)
                            .is_some_and(|q| q == Expr::zero());
                    subset_cache.insert((*x, z), subset);
                    subset
                };
                if !subset {
                    continue;
                }
                // A subset B => A&(-B) = A&(-A)&(-B).
                let observer = Expr::Var(*x);
                emit_cut_relation(
                    s,
                    MaskStableCongruence {
                        observer,
                        lhs: Expr::Var(z) & n.literal.expr(mask),
                        rhs: Expr::Var(z),
                    },
                    rt,
                    hidden,
                    rules,
                );
            }
        }
    }
}

pub(super) fn close<C: LinearCache>(s: &mut MBASolver<'_, C>, root: Expr) -> Expr {
    let Some(rt) = term_map(&root, s.mask) else {
        return root;
    };
    let (defs, hidden) = definitions(s, &root);
    let indexes = literal_indexes(&defs, &hidden, s.mask);
    let mut rules = Vec::new();
    collect_valuation_from_buckets(s, &root, &rt, &defs, &hidden, &indexes, &mut rules);
    collect_subset_cuts(s, &rt, &defs, &hidden, &indexes, &mut rules);
    choose_rule(&rt, rules, &hidden, s.mask).unwrap_or(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{utils::cache::LocalCache, varint::make_mask};

    #[test]
    fn term_map_merges_distinct_cores_with_same_monomial() {
        let mask = make_mask(4);
        let x = Expr::Var(VarId(0));
        // De Morgan can leave And([x, x]) beside Var(x) after one reduction.
        let expr = x.clone() + !(!x.clone() | !x);
        assert_eq!(
            term_map(&expr, mask).unwrap(),
            BTreeMap::from([(BTreeSet::from([VarId(0)]), 2)])
        );
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

        let result = certify_free_relation(&mut solver, original.clone()).unwrap();
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
        let a = Rule {
            pivot: pivot.clone(),
            terms: BTreeMap::from([(BTreeSet::new(), 1), (pivot.clone(), 1)]),
        };
        let b = Rule {
            pivot: pivot.clone(),
            terms: BTreeMap::from([(BTreeSet::new(), 1), (pivot.clone(), 1), (extra, 1)]),
        };

        let root = BTreeMap::from([(pivot.clone(), 1)]);
        let ab = choose_rule(&root, vec![a.clone(), b.clone()], &hidden, make_mask(4));
        let ba = choose_rule(&root, vec![b, a], &hidden, make_mask(4));

        assert_eq!(ab, ba);
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
        let cut = MaskStableCongruence {
            observer: y.clone(),
            lhs: x.clone(),
            rhs: xm1,
        };
        let hidden = BTreeSet::from([VarId(2)]);
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &root, 4);
        let mut rules = Vec::new();

        emit_cut_relation(&mut solver, cut, &root_terms, &hidden, &mut rules);
        let applied = choose_rule(&root_terms, rules, &hidden, mask)
            .expect("valuation cut must normalize through hidden cut");
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
        let cut = MaskStableCongruence {
            // The resident hidden coordinate is the all-ones WORD. The
            // resulting relation is `y - (-1) = 0`, whose rule has a real
            // empty-monomial coefficient.
            observer: Expr::make_const(mask),
            lhs: y.clone(),
            rhs: Expr::make_const(mask),
        };
        let hidden = BTreeSet::from([VarId(1)]);
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &root, 4);
        let mut rules = Vec::new();

        emit_cut_relation(&mut solver, cut, &root_terms, &hidden, &mut rules);
        assert!(!rules.is_empty(), "rules: {rules:?}");
        assert!(rules.iter().any(|rule| {
            rule.terms
                .get(&BTreeSet::new())
                .is_some_and(|coefficient| *coefficient != 0)
        }));
        let applied = choose_rule(&root_terms, rules, &hidden, mask).unwrap();

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
        let high = BTreeSet::from([VarId(1), VarId(2)]);
        let high_extension = BTreeSet::from([VarId(1), VarId(2), VarId(3)]);
        let lower = BTreeSet::from([VarId(4)]);
        let relation = (Expr::Var(VarId(1)) & Expr::Var(VarId(2)))
            + (2u64 * (Expr::Var(VarId(1)) & Expr::Var(VarId(2)) & Expr::Var(VarId(3))))
            + Expr::Var(VarId(4));
        let root = BTreeMap::from([(lower.clone(), 1)]);
        let hidden = BTreeSet::from([VarId(1), VarId(2), VarId(4)]);

        let terms = term_map(&relation, mask).unwrap();
        let rule = normalize_rule(build(terms, mask), &root, &hidden, mask).unwrap();
        assert_eq!(rule.pivot, lower);
        assert!(high.is_subset(&high_extension));
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
            for (variable, definition) in &defs {
                if hidden.contains(variable) {
                    solver
                        .non_linear_components
                        .insert(*variable, definition.clone());
                }
            }
            let mut rules = Vec::new();
            let indexes = literal_indexes(&defs, &hidden, mask);
            collect_subset_cuts(
                &mut solver,
                &root_terms,
                &defs,
                &hidden,
                &indexes,
                &mut rules,
            );
            choose_rule(&root_terms, rules, &hidden, mask)
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
            (VarId(7), neg(x.clone(), mask)),
        ]);
        let hidden = BTreeSet::from([VarId(2), VarId(4), VarId(7)]);
        let index = literal_indexes(&defs, &hidden, mask);
        let literals = index
            .by_definition
            .get(&neg(defs[&VarId(2)].clone(), mask))
            .unwrap()
            .iter()
            .map(|id| index.population[*id].literal)
            .collect::<Vec<_>>();

        assert_eq!(
            literals,
            vec![
                HiddenLiteral {
                    var: VarId(4),
                    complemented: true,
                },
                HiddenLiteral {
                    var: VarId(7),
                    complemented: false,
                },
            ]
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
            let mut rules = Vec::new();
            let indexes = literal_indexes(&defs, &hidden, mask);
            collect_valuation_from_buckets(
                &mut solver,
                &root,
                &root_terms,
                &defs,
                &hidden,
                &indexes,
                &mut rules,
            );
            choose_rule(&root_terms, rules, &hidden, mask)
        };

        let pre = run(x.clone());
        let gauge = run(comp(x.clone(), mask));

        assert!(pre.is_some());
        assert!(gauge.is_some());
        let pre = pre.unwrap();
        let gauge = gauge.unwrap();
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
}

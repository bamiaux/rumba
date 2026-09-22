//! Projector–Defect closure simplifies the current expression using relations
//! carried by hidden variables.
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
// Projector–Defect closure has one semantic primitive:
//
//     ObsEq(O, U, V) := O & (U xor V) = 0
//                         => O & U = O & V.
//
// The valuation/predecessor and Boolean-order routines are certificate
// producers for this same observed equality; they are not separate semantic
// laws. `emit_observed_eq` deliberately consumes only the common certificate.
//
// The valuation producer uses:
//       U = X, V = X-1,
//       U xor V = C_X = X xor (X-1) = 2*(X & -X)-1;
//
// The Boolean-order producer uses:
//       A subset B  =>  A&(-B) = A&(-A)&(-B).
//
// Projector–Defect closure is a generic engine for these mask-stable
// congruences. It currently has two certificate producers:
// valuation/even-multiple predecessors and
// Boolean subset order. Each producer must establish `ObsEq` before the
// contextual quotient is allowed to consume it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    pivot: Monomial,
    terms: Terms,
}

/// An observed equality under one WORD projector:
///
/// ```text
/// ObsEq(observer, lhs, rhs) iff observer & (lhs xor rhs) == 0
/// ```
///
/// Construction is restricted to the certified Projector–Defect producers. Once
/// established, the equality may be lowered to a free linear relation and
/// used by the one finite contextual quotient. In particular, the empty
/// monomial is safe in contextual substitution because `~0 & C == C`.
struct ObservedEq {
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
    coeffs(&arithmetic(&e.clone().reduce_masked(mask), mask), mask)
        .into_iter()
        .all(|(term, coefficient)| term == Expr::Const(1) || coefficient & 1 == 0)
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

#[cfg(debug_assertions)]
fn observer_literal(e: &Expr, mask: u64) -> Option<HiddenLiteral> {
    match e.clone().reduce_masked(mask) {
        Expr::Var(var) => Some(HiddenLiteral {
            var,
            complemented: false,
        }),
        Expr::Not(child) => match *child {
            Expr::Var(var) => Some(HiddenLiteral {
                var,
                complemented: true,
            }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(debug_assertions)]
fn restrict_terms(terms: &Terms, observer: HiddenLiteral, mask: u64) -> Terms {
    let mut restricted = BTreeMap::new();
    for (monomial, coefficient) in terms {
        if !observer.complemented && monomial.contains(&observer.var) {
            continue;
        }

        let mut restricted_monomial = monomial.clone();
        let observer_factor = if observer.complemented
            && restricted_monomial.remove(&observer.var)
            && restricted_monomial.is_empty()
        {
            // A complemented observer is zero when v is the all-ones WORD.
            // A monomial containing other variables simply loses v; the
            // singleton monomial `{v}` becomes the constant `mask`.
            mask
        } else {
            1
        };
        let semantic_coefficient = if monomial.is_empty() {
            coefficient.wrapping_neg() & mask
        } else {
            *coefficient & mask
        };
        let semantic_coefficient = semantic_coefficient.wrapping_mul(observer_factor) & mask;
        let stored_coefficient = if restricted_monomial.is_empty() {
            semantic_coefficient.wrapping_neg() & mask
        } else {
            semantic_coefficient
        };
        let coefficient = restricted.entry(restricted_monomial).or_insert(0u64);
        *coefficient = coefficient.wrapping_add(stored_coefficient) & mask;
        if *coefficient == 0 {
            // Keep the restricted Terms representation normalized, including
            // its negated empty-monomial convention.
            restricted.retain(|_, value| *value != 0);
        }
    }
    restricted
}

#[cfg(debug_assertions)]
fn assert_observer_restriction(terms: &Terms, observer: HiddenLiteral, mask: u64) {
    let restricted = restrict_terms(terms, observer, mask);
    debug_assert!(
        restricted.is_empty(),
        "free Projector–Defect lowering violates observer restriction for v{}{}: terms={:?}, restricted={:?}",
        observer.var,
        if observer.complemented {
            " complemented"
        } else {
            ""
        },
        terms,
        restricted
    );
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

/// One finite substitution only; there is no Projector–Defect reduction loop or work budget.
fn apply_rule(root_terms: &Terms, rule: &Rule, mask: u64) -> Terms {
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
    out
}

fn choose_rule(
    root_terms: &Terms,
    rules: Vec<Rule>,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) -> Option<Terms> {
    let mut best: Option<(usize, Rule, Terms)> = None;
    for rule in rules {
        let applied = apply_rule(root_terms, &rule, mask);
        let size = applied.len();
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
    if s.is_linear(&e)
        && let Some(q) = s.is_linear_bitwise(e.clone(), s.mask)
    {
        return Some(q.reduce_masked(s.mask));
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
fn linearize_free_relation<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    s.solve_linear(e.reduce_masked(s.mask))
        .ok()
        .map(|q| q.reduce_masked(s.mask))
}

fn emit_observed_eq<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    observed: ObservedEq,
    rt: &Terms,
    hidden: &BTreeSet<VarId>,
    rules: &mut Vec<Rule>,
) {
    // The producer establishes ObsEq. This free linearization treats every
    // VarId as independent and only lowers the already-proven relation; it is
    // not itself a proof using hidden definitions.
    let ObservedEq { observer, lhs, rhs } = observed;
    if let Some(r) = linearize_free_relation(s, (observer.clone() & lhs) - (observer.clone() & rhs))
    {
        #[cfg(debug_assertions)]
        if let (Some(observer), Some(terms)) =
            (observer_literal(&observer, s.mask), term_map(&r, s.mask))
        {
            assert_observer_restriction(&terms, observer, s.mask);
        }
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
    for v in defs.keys() {
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

fn collect_predecessor_certificates<C: LinearCache>(
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
        for observer_literal in literal_views(y, hidden) {
            let Some(candidate) = observer_literal.definition(defs, mask) else {
                continue;
            };
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
                emit_observed_eq(
                    s,
                    ObservedEq {
                        observer: observer_literal.expr(mask),
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

fn collect_order_certificates<C: LinearCache>(
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
                        linearize_free_relation(s, (observer.clone() & b.clone()) - observer)
                            .is_some_and(|q| q == Expr::zero());
                    subset_cache.insert((*x, z), subset);
                    subset
                };
                if !subset {
                    continue;
                }
                // A subset B => A&(-B) = A&(-A)&(-B).
                let observer = Expr::Var(*x);
                emit_observed_eq(
                    s,
                    ObservedEq {
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
    collect_predecessor_certificates(s, &root, &rt, &defs, &hidden, &indexes, &mut rules);
    collect_order_certificates(s, &rt, &defs, &hidden, &indexes, &mut rules);
    choose_rule(&rt, rules, &hidden, s.mask)
        .map(|terms| build(terms, s.mask))
        .unwrap_or(root)
}

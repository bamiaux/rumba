//! R23-MIN hidden quotient: exactly two certificate geometries.
//!
//! The predecessor annihilator formula `Ann_&(X xor (X-1)) = (2X)` is
//! implemented through width-typed affine projective rays.  The Boolean /
//! associated-graded order is the independent chart required by the
//! historical 17380 family.
//!
//! The module performs one finite contextual substitution.  It has no local
//! fixed point, no worklist, no TARGET read, no pattern catalogue, and no
//! global state.  Further progress belongs to RUMBA's ordinary outer passes.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::MBASolver;
use crate::{
    expr::{Expr, VarId},
    utils::cache::LinearCache,
};

type Monomial = BTreeSet<VarId>;
type Terms = BTreeMap<Monomial, u64>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Literal {
    var: VarId,
    complement: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Ray {
    width: u8,
    grade: u8,
    coefficients: Vec<(Expr, u64)>,
}

#[derive(Clone, Debug)]
pub(super) struct HiddenMeta {
    pub(super) width: u8,
    direct: Option<Ray>,
    complement: Option<Ray>,
}

fn width_mask(width: u8) -> u64 {
    match width {
        0 => 0,
        64.. => u64::MAX,
        _ => (1u64 << width) - 1,
    }
}

fn comp(e: Expr, mask: u64) -> Expr {
    (-e - Expr::make_const(1)).reduce_masked(mask)
}

fn coeffs(e: &Expr, mask: u64) -> BTreeMap<Expr, u64> {
    let e = e.clone().reduce_masked(mask);
    let terms: &[Expr] = match &e {
        Expr::Add(xs) => xs,
        _ => std::slice::from_ref(&e),
    };
    let mut out = BTreeMap::new();
    for term in terms {
        let (coefficient, core) = match term {
            Expr::Scale(c, x) => (*c & mask, x.as_ref().clone()),
            Expr::Const(c) => (*c & mask, Expr::make_const(1)),
            _ => (1, term.clone()),
        };
        let value = out.entry(core).or_insert(0u64);
        *value = value.wrapping_add(coefficient) & mask;
    }
    out.retain(|_, c| *c != 0);
    out
}

fn v2(c: u64, width: u8) -> u8 {
    let c = c & width_mask(width);
    if c == 0 {
        width
    } else {
        (c.trailing_zeros() as u8).min(width)
    }
}

fn odd_inverse(c: u64, mask: u64) -> u64 {
    debug_assert!(c & 1 == 1);
    let mut x = c & mask;
    // Six doublings cover WORD64 from the mod-2 odd seed.
    for _ in 0..6 {
        x = x.wrapping_mul(2u64.wrapping_sub(c.wrapping_mul(x))) & mask;
    }
    x
}

fn affine_ray(e: &Expr, width: u8) -> Option<Ray> {
    let mask = width_mask(width);
    let coefficients = coeffs(e, mask);
    if coefficients.is_empty() {
        return Some(Ray {
            width,
            grade: width,
            coefficients: Vec::new(),
        });
    }
    let grade = coefficients.values().copied().map(|c| v2(c, width)).min()?;
    if grade >= width {
        return Some(Ray {
            width,
            grade: width,
            coefficients: Vec::new(),
        });
    }
    let precision = width - grade;
    let local_mask = width_mask(precision);
    let mut divided = coefficients
        .into_iter()
        .filter_map(|(e, c)| {
            let c = (c >> grade) & local_mask;
            (c != 0).then_some((e, c))
        })
        .collect::<BTreeMap<_, _>>();
    let (pivot, pivot_coefficient) = divided
        .iter()
        .find(|(_, c)| **c & 1 == 1)
        .map(|(e, c)| (e.clone(), *c))?;
    let inverse = odd_inverse(pivot_coefficient, local_mask);
    for c in divided.values_mut() {
        *c = c.wrapping_mul(inverse) & local_mask;
    }
    debug_assert_eq!(divided.get(&pivot).copied(), Some(1));
    Some(Ray {
        width,
        grade,
        coefficients: divided.into_iter().collect(),
    })
}

fn ray_mod(ray: &Ray, precision: u8) -> Vec<(Expr, u64)> {
    let mask = width_mask(precision);
    ray.coefficients
        .iter()
        .filter_map(|(e, c)| {
            let c = *c & mask;
            (c != 0).then_some((e.clone(), c))
        })
        .collect()
}

/// Exact membership O in (2X) in the affine projective chart.
fn principal_contains(observer: &Ray, base: &Ray) -> bool {
    if observer.width != base.width {
        return false;
    }
    let width = base.width;
    if observer.grade >= width {
        return true;
    }
    if base.grade >= width || observer.grade < base.grade.saturating_add(1) {
        return false;
    }
    let precision = width - observer.grade;
    ray_mod(observer, precision) == ray_mod(base, precision)
}

pub(super) fn hidden_meta(definition: &Expr, width: u8, mask: u64) -> HiddenMeta {
    HiddenMeta {
        width,
        direct: affine_ray(definition, width),
        complement: affine_ray(&comp(definition.clone(), mask), width),
    }
}

fn literal_expr(literal: Literal, mask: u64) -> Expr {
    let e = Expr::Var(literal.var);
    if literal.complement {
        (!e).reduce_masked(mask)
    } else {
        e
    }
}

fn definition<C: LinearCache>(s: &MBASolver<'_, C>, var: VarId) -> Expr {
    s.non_linear_components
        .get_by_left(&var)
        .cloned()
        .unwrap_or(Expr::Var(var))
}

fn ray<C: LinearCache>(s: &MBASolver<'_, C>, literal: Literal) -> Option<Ray> {
    if let Some(meta) = s.r23_hidden_meta.get(&literal.var) {
        // Critical soundness boundary: a receipt made in R_k may not be consumed
        // as a WORD in R_n without an explicit transport.
        if meta.width != s.n {
            return None;
        }
        return if literal.complement {
            meta.complement.clone()
        } else {
            meta.direct.clone()
        };
    }
    let e = literal_expr(literal, s.mask);
    affine_ray(&e, s.n)
}

fn resident_literals<C: LinearCache>(s: &MBASolver<'_, C>, root: &Expr) -> Vec<(Literal, Expr)> {
    let hidden = s
        .non_linear_components
        .iter()
        .map(|(v, _)| *v)
        .collect::<BTreeSet<_>>();
    let mut vars = root.get_vars().into_iter().collect::<BTreeSet<_>>();
    for (v, e) in s.non_linear_components.iter() {
        vars.insert(*v);
        vars.extend(e.get_vars());
    }
    let mut out = Vec::new();
    for var in vars {
        let d = definition(s, var);
        out.push((
            Literal {
                var,
                complement: false,
            },
            d.clone(),
        ));
        if hidden.contains(&var) {
            out.push((
                Literal {
                    var,
                    complement: true,
                },
                comp(d, s.mask),
            ));
        }
    }
    out
}

fn known<C: LinearCache>(s: &MBASolver<'_, C>, e: &Expr) -> Option<Expr> {
    if let Some(var) = s.non_linear_components.get_by_right(e) {
        return Some(Expr::Var(*var));
    }
    s.non_linear_components
        .get_by_right(&comp(e.clone(), s.mask))
        .map(|var| (!Expr::Var(*var)).reduce_masked(s.mask))
}

fn fold_known<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Expr {
    let mapped = e.map(|x| fold_known(s, x));
    let e = s.reduce(mapped, s.mask);
    known(s, &e).unwrap_or(e)
}

fn bitwise_view<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = s.reduce(e, s.mask);
    known(s, &e).or_else(|| {
        s.is_linear(&e)
            .then(|| s.is_linear_bitwise(e.clone(), s.mask))
            .flatten()
            .map(|q| s.reduce(q, s.mask))
    })
}

fn refold<C: LinearCache>(s: &mut MBASolver<'_, C>, e: Expr) -> Option<Expr> {
    let e = s.reduce(e, s.mask);
    if let Some(view) = bitwise_view(s, e.clone()) {
        return Some(view);
    }
    let folded = fold_known(s, e);
    bitwise_view(s, folded)
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

fn add_term(out: &mut Terms, monomial: Monomial, coefficient: u64, mask: u64) {
    let value = out
        .get(&monomial)
        .copied()
        .unwrap_or(0)
        .wrapping_add(coefficient)
        & mask;
    if value == 0 {
        out.remove(&monomial);
    } else {
        out.insert(monomial, value);
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
    let started = Instant::now();
    let reduced = s.reduce(e, s.mask);
    let result = s
        .solve_linear(reduced, false)
        .ok()
        .and_then(|q| term_map(&q, s.mask));
    s.stats.filtered_cut.relation_proof_nanos +=
        started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    result
}

fn rank<'a>(m: &'a Monomial, hidden: &BTreeSet<VarId>) -> (usize, usize, &'a Monomial) {
    (m.iter().filter(|v| hidden.contains(v)).count(), m.len(), m)
}

fn measure(terms: &Terms, hidden: &BTreeSet<VarId>) -> (usize, usize, usize) {
    let mut incidences = 0usize;
    let mut max_degree = 0usize;
    for monomial in terms.keys() {
        let degree = monomial.iter().filter(|v| hidden.contains(v)).count();
        incidences += degree;
        max_degree = max_degree.max(degree);
    }
    (incidences, max_degree, terms.len())
}

fn quotient_once(
    root: &Terms,
    mut relation: Terms,
    hidden: &BTreeSet<VarId>,
    mask: u64,
) -> Option<Terms> {
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
    let mut out = root.clone();
    for (monomial, coefficient) in root {
        if !pivot.is_subset(monomial) {
            continue;
        }
        out.remove(monomial);
        let context = monomial.difference(&pivot).copied().collect::<Monomial>();
        for (replacement, rc) in relation.iter().filter(|(m, _)| **m != pivot) {
            add_term(
                &mut out,
                context.union(replacement).copied().collect(),
                coefficient.wrapping_mul(*rc).wrapping_neg(),
                mask,
            );
        }
    }
    Some(out)
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

struct Best {
    key: ((usize, usize, usize), usize, Expr),
    terms: Terms,
    producer: Producer,
}

#[derive(Clone, Copy)]
enum Producer {
    Predecessor,
    Order,
}

#[allow(clippy::too_many_arguments)]
fn consider<C: LinearCache>(
    best: &mut Option<Best>,
    s: &mut MBASolver<'_, C>,
    root: &Terms,
    hidden: &BTreeSet<VarId>,
    observer: Expr,
    lhs: Expr,
    rhs: Expr,
    producer: Producer,
) {
    // The relation is certified as a free-WORD equality before contextual
    // quotienting. Hidden definitions are used only to construct representatives.
    let relation_expr = s.reduce((observer.clone() & lhs) - (observer & rhs), s.mask);
    let Some(relation) = linear_terms(s, relation_expr) else {
        return;
    };
    match producer {
        Producer::Predecessor => s.stats.filtered_cut.predecessor_certified_relations += 1,
        Producer::Order => s.stats.filtered_cut.order_certified_relations += 1,
    }
    let Some(candidate) = quotient_once(root, relation, hidden, s.mask) else {
        return;
    };
    let candidate_measure = measure(&candidate, hidden);
    if candidate_measure >= measure(root, hidden) {
        return;
    }
    match producer {
        Producer::Predecessor => s.stats.filtered_cut.predecessor_improving_quotients += 1,
        Producer::Order => s.stats.filtered_cut.order_improving_quotients += 1,
    }
    let expression = build(candidate.clone(), s.mask);
    let key = (candidate_measure, expression.size(), expression.clone());
    if best.as_ref().is_none_or(|old| key < old.key.clone()) {
        *best = Some(Best {
            key,
            terms: candidate,
            producer,
        });
    }
}

fn predecessor<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    root: &Expr,
    root_terms: &Terms,
    hidden: &BTreeSet<VarId>,
    best: &mut Option<Best>,
) {
    let residents = resident_literals(s, root);
    for (base_literal, base_definition) in &residents {
        if !root_terms.keys().any(|m| m.contains(&base_literal.var)) {
            continue;
        }
        let Some(base_ray) = ray(s, *base_literal) else {
            continue;
        };
        let predecessor = s.reduce(base_definition.clone() - Expr::make_const(1), s.mask);
        let Some(pred) = refold(s, predecessor) else {
            continue;
        };
        let lhs = literal_expr(*base_literal, s.mask);
        for (observer_literal, _) in &residents {
            let Some(observer_ray) = ray(s, *observer_literal) else {
                continue;
            };
            if !principal_contains(&observer_ray, &base_ray) {
                continue;
            }
            s.stats.filtered_cut.predecessor_containment_successes += 1;
            s.stats.filtered_cut.predecessor_candidates += 1;
            consider(
                best,
                s,
                root_terms,
                hidden,
                literal_expr(*observer_literal, s.mask),
                lhs.clone(),
                pred.clone(),
                Producer::Predecessor,
            );
        }
    }
}

fn subset<C: LinearCache>(s: &mut MBASolver<'_, C>, a: Expr, b: Expr) -> bool {
    let relation = s.reduce((a.clone() & b) - a, s.mask);
    let result = linear_terms(s, relation);
    let success = result.as_ref().is_some_and(|terms| terms.is_empty());
    if success {
        s.stats.filtered_cut.order_subset_successes += 1;
    }
    success
}

fn order<C: LinearCache>(
    s: &mut MBASolver<'_, C>,
    root: &Expr,
    root_terms: &Terms,
    hidden: &BTreeSet<VarId>,
    best: &mut Option<Best>,
) {
    let mut vars = root.get_vars().into_iter().collect::<BTreeSet<_>>();
    for (_, d) in s.non_linear_components.iter() {
        vars.extend(d.get_vars());
    }

    let mut words = BTreeSet::<Expr>::new();
    for var in vars {
        if s.non_linear_components.get_by_left(&var).is_none() {
            words.insert(Expr::Var(var));
        }
    }
    let definitions = s
        .non_linear_components
        .iter()
        .map(|(_, d)| d.clone())
        .collect::<Vec<_>>();
    for d in definitions {
        let d = s.reduce(d.clone(), s.mask);
        words.insert(d.clone());
        words.insert(s.reduce(-d, s.mask));
    }
    let words = words.into_iter().collect::<Vec<_>>();
    let mut views = BTreeMap::<Expr, Option<Expr>>::new();

    for a_word in &words {
        let a = views
            .entry(a_word.clone())
            .or_insert_with(|| refold(s, a_word.clone()))
            .clone();
        let neg_a_word = s.reduce(-a_word.clone(), s.mask);
        let neg_a = views
            .entry(neg_a_word.clone())
            .or_insert_with(|| refold(s, neg_a_word))
            .clone();
        let (Some(a), Some(neg_a)) = (a, neg_a) else {
            continue;
        };
        let p0a = s.reduce((!a.clone()) & (!neg_a.clone()), s.mask);
        let la = s.reduce(a.clone() & neg_a.clone(), s.mask);

        for b_word in &words {
            let b = views
                .entry(b_word.clone())
                .or_insert_with(|| refold(s, b_word.clone()))
                .clone();
            let neg_b_word = s.reduce(-b_word.clone(), s.mask);
            let neg_b = views
                .entry(neg_b_word.clone())
                .or_insert_with(|| refold(s, neg_b_word))
                .clone();
            let (Some(b), Some(neg_b)) = (b, neg_b) else {
                continue;
            };
            s.stats.filtered_cut.order_candidate_comparisons += 1;
            if !subset(s, b.clone(), a.clone()) {
                continue;
            }
            let p0b = s.reduce((!neg_b.clone()) & (!b.clone()), s.mask);
            let one = Expr::make_const(s.mask);
            let zero = Expr::zero();
            for (observer, lhs, rhs) in [
                (p0a.clone(), neg_b.clone(), zero.clone()),
                (p0a.clone(), p0b, one.clone()),
                (la.clone(), neg_b.clone(), b.clone()),
                (
                    s.reduce(b.clone() & neg_a.clone(), s.mask),
                    neg_b.clone(),
                    one.clone(),
                ),
            ] {
                consider(
                    best,
                    s,
                    root_terms,
                    hidden,
                    observer,
                    lhs,
                    rhs,
                    Producer::Order,
                );
            }
        }
    }
}

pub(super) fn close<C: LinearCache>(s: &mut MBASolver<'_, C>, root: Expr) -> Expr {
    s.stats.filtered_cut.close_calls += 1;
    let Some(root_terms) = term_map(&root, s.mask) else {
        s.stats.filtered_cut.no_root_term_map += 1;
        return root;
    };
    let hidden = s
        .non_linear_components
        .iter()
        .map(|(v, _)| *v)
        .collect::<BTreeSet<_>>();
    let mut best = None;
    if s.settings.cut_predecessor {
        predecessor(s, &root, &root_terms, &hidden, &mut best);
    }
    if s.settings.cut_order {
        order(s, &root, &root_terms, &hidden, &mut best);
    }
    let Some(candidate) = best else {
        s.stats.filtered_cut.close_no_improvement += 1;
        s.stats.filtered_cut.term_before += root_terms.len() as u64;
        s.stats.filtered_cut.term_after += root_terms.len() as u64;
        s.stats.filtered_cut.hidden_before += hidden.len() as u64;
        s.stats.filtered_cut.hidden_after += hidden.len() as u64;
        return root;
    };
    match candidate.producer {
        Producer::Predecessor => s.stats.filtered_cut.predecessor_winning_candidates += 1,
        Producer::Order => s.stats.filtered_cut.order_winning_candidates += 1,
    }
    let terms_after = candidate.terms.len() as u64;
    s.stats.filtered_cut.term_before += root_terms.len() as u64;
    s.stats.filtered_cut.term_after += terms_after;
    s.stats.filtered_cut.hidden_before += hidden.len() as u64;
    s.stats.filtered_cut.hidden_after += hidden.len() as u64;
    s.stats.filtered_cut.close_changed += 1;
    build(candidate.terms, s.mask)
}

//! Cosmetic rewriting of a solved expression back into bitwise form.
//!
//! The solver works in the linear-MBA basis, where `a | b` is carried as
//! `a + b - (a & b)`. That form is what the linear algebra needs, but it is not
//! what a reader wants to see, so the three encodings that have an exact
//! bitwise counterpart are folded back on the way out:
//!
//! | solved form            | prettified |
//! |------------------------|------------|
//! | `a + b - (a & b)`      | `a \| b`   |
//! | `a + b - 2 * (a & b)`  | `a ^ b`    |
//! | `-1 - a`               | `~a`       |
//!
//! Algebraic identities are recognised from coefficients on canonical meet
//! supports inside a larger `Add`. A contraction is retained only when it
//! strictly reduces `Expr::size`.
//!
//! This runs once, on the expression handed back to the caller. It must never
//! run inside the solver's own recursion: the bitwise nodes it produces are
//! larger in [`Expr::size`] terms than the linear forms they replace, so a
//! prettified intermediate would perturb the fixed-point loop's size-based
//! stopping rule.

use rustc_hash::FxHashMap as HashMap;

use crate::{expr::Expr, varint::make_mask};

/// Rewrites the exact linear encodings of `|`, `^` and `~` back into bitwise
/// nodes, bottom-up, on `n` bits.
pub(crate) fn prettify(e: Expr, n: u8) -> Expr {
    prettify_masked(e, make_mask(n))
}

fn prettify_masked(e: Expr, mask: u64) -> Expr {
    // Children first: an outer pattern is recognised in terms of the operands
    // its children have already settled on, so `a` and `b` stay comparable
    // whether or not they were themselves rewritten.
    let e = canonical_node(e.map(|child| prettify_masked(child, mask)), mask);

    let e = rewrite_add_algebraic(e, mask);

    canonical_node(e, mask)
}

/// Contracts a solved sparse sum, considering one higher-order section before
/// the ordinary binary normalization as well as the binary-first result.
/// The smaller of these two deterministic paths is retained.
fn rewrite_add_algebraic(e: Expr, mask: u64) -> Expr {
    let Expr::Add(terms) = &e else {
        return as_not(&e, mask).unwrap_or(e);
    };
    // The smallest higher-order section needs four resident terms.
    if terms.len() < 4 {
        return rewrite_add_path(e, mask);
    }
    let mut factor_sets = vec![None; terms.len()];
    let higher_first = higher_order_algebraic_in_add(&e, mask, &mut factor_sets);
    let binary_first = rewrite_add_path(e, mask);
    let Some(higher_first) = higher_first else {
        return binary_first;
    };
    let higher_first = rewrite_add_path(canonical_node(higher_first, mask), mask);
    if higher_first.size() < binary_first.size() {
        higher_first
    } else {
        binary_first
    }
}

/// Each contraction removes terms. A binary step may temporarily increase
/// size before a complement step shrinks it, so keep the smallest expression
/// encountered along the path.
fn rewrite_add_path(mut e: Expr, mask: u64) -> Expr {
    let Expr::Add(_) = &e else {
        return e;
    };
    let mut best = e.clone();
    loop {
        let Expr::Add(terms) = &e else {
            return best;
        };
        let mut factor_sets = vec![None; terms.len()];
        let replacement = union_bitwise_in_add(&e, mask, &mut factor_sets)
            .or_else(|| difference_in_add(&e, mask, &mut factor_sets))
            .or_else(|| as_not_in_add(&e, mask))
            .or_else(|| as_not(&e, mask))
            .or_else(|| higher_order_algebraic_in_add(&e, mask, &mut factor_sets));
        let Some(next) = replacement else {
            return best;
        };
        e = canonical_node(next, mask);
        if e.size() < best.size() {
            best = e.clone();
        }
    }
}

#[derive(Clone, Copy)]
enum NaryOperator {
    And,
    Or,
    Xor,
    Add,
    Mul,
}

fn make_nary(operator: NaryOperator, children: Vec<Expr>) -> Expr {
    match operator {
        NaryOperator::And => Expr::And(children),
        NaryOperator::Or => Expr::Or(children),
        NaryOperator::Xor => Expr::Xor(children),
        NaryOperator::Add => Expr::Add(children),
        NaryOperator::Mul => Expr::Mul(children),
    }
}

fn flatten_nary(operator: NaryOperator, children: Vec<Expr>) -> Expr {
    let mut flat = Vec::with_capacity(children.len());
    for child in children {
        match (operator, child) {
            (NaryOperator::And, Expr::And(inner))
            | (NaryOperator::Or, Expr::Or(inner))
            | (NaryOperator::Xor, Expr::Xor(inner))
            | (NaryOperator::Add, Expr::Add(inner))
            | (NaryOperator::Mul, Expr::Mul(inner)) => flat.extend(inner),
            (_, child) => flat.push(child),
        }
    }

    match flat.len() {
        1 => flat.pop().expect("one-child node must contain its child"),
        _ => make_nary(operator, flat),
    }
}

fn demorgan(operator: NaryOperator, children: Vec<Expr>) -> Expr {
    let opposite = match operator {
        NaryOperator::And => NaryOperator::Or,
        NaryOperator::Or => NaryOperator::And,
        _ => unreachable!("De Morgan applies only to Boolean operators"),
    };

    if children.iter().all(|child| matches!(child, Expr::Not(_))) {
        let operands = children
            .into_iter()
            .map(|child| match child {
                Expr::Not(inner) => *inner,
                _ => unreachable!("all operands were checked as complements"),
            })
            .collect();
        return Expr::Not(Box::new(flatten_nary(opposite, operands)));
    }

    make_nary(operator, children)
}

fn canonical_boolean(operator: NaryOperator, children: Vec<Expr>) -> Expr {
    match flatten_nary(operator, children) {
        Expr::And(children) => demorgan(NaryOperator::And, children),
        Expr::Or(children) => demorgan(NaryOperator::Or, children),
        e => e,
    }
}

/// Keeps the output tree canonical without invoking the arithmetic reducer.
/// These are constructor identities only: associative n-ary nodes are flattened,
/// one-child wrappers disappear, and a unit scale is transparent at the current
/// word width.
fn canonical_node(e: Expr, mask: u64) -> Expr {
    match e {
        Expr::And(children) => canonical_boolean(NaryOperator::And, children),
        Expr::Or(children) => canonical_boolean(NaryOperator::Or, children),
        Expr::Xor(children) => flatten_nary(NaryOperator::Xor, children),
        Expr::Add(children) => flatten_nary(NaryOperator::Add, children),
        Expr::Mul(children) => flatten_nary(NaryOperator::Mul, children),
        Expr::Scale(coefficient, child) if coefficient & mask == 1 => *child,
        e => e,
    }
}

/// Peels the single-operand n-ary wrappers the solver leaves behind, so that
/// the `a` of a bare term and the `a` inside `a & b` compare equal: the solver
/// emits `a + b - (a & b)` as `And([a]) + And([b]) - (a & b)`.
fn peel(e: &Expr) -> &Expr {
    match e {
        Expr::And(inner)
        | Expr::Or(inner)
        | Expr::Xor(inner)
        | Expr::Add(inner)
        | Expr::Mul(inner)
            if inner.len() == 1 =>
        {
            peel(&inner[0])
        }
        _ => e,
    }
}

fn scaled_term(e: &Expr, mask: u64) -> (u64, &Expr) {
    match peel(e) {
        Expr::Scale(coefficient, inner) => (coefficient & mask, peel(inner)),
        e => (1, e),
    }
}

fn scaled_output(coefficient: u64, e: Expr, mask: u64) -> Expr {
    let e = canonical_node(e, mask);
    match coefficient & mask {
        0 => Expr::Const(0),
        1 => e,
        coefficient => Expr::Scale(coefficient, Box::new(e)),
    }
}

fn support(mut factors: Vec<Expr>) -> Vec<Expr> {
    factors.sort_unstable();
    factors.dedup();
    factors
}

fn constant_coefficient(e: &Expr, mask: u64) -> Option<u64> {
    match peel(e) {
        Expr::Const(c) => Some(c & mask),
        Expr::Scale(k, inner) => match peel(inner) {
            Expr::Const(c) => Some(k.wrapping_mul(*c) & mask),
            _ => None,
        },
        _ => None,
    }
}

fn find_support(
    supports: &HashMap<Vec<Expr>, (usize, u64)>,
    wanted: &[Expr],
    coefficient: u64,
) -> Option<usize> {
    supports
        .get(wanted)
        .and_then(|&(index, found)| (found == coefficient).then_some(index))
}

fn find_support_any(
    supports: &HashMap<Vec<Expr>, (usize, u64)>,
    wanted: &[Expr],
) -> Option<(usize, u64)> {
    supports.get(wanted).copied()
}

fn replace_indices(terms: &[Expr], removed: &[usize], replacement: Expr, mask: u64) -> Expr {
    if removed
        .iter()
        .enumerate()
        .any(|(position, index)| removed[..position].contains(index))
    {
        return Expr::Add(terms.to_vec());
    }
    let mut result = Vec::with_capacity(terms.len() + 1 - removed.len());
    for (index, term) in terms.iter().enumerate() {
        if !removed.contains(&index) {
            result.push(term.clone());
        }
    }
    match replacement {
        Expr::Add(inner) => result.extend(inner),
        Expr::Const(0) => {}
        other => result.push(other),
    }
    result.sort_unstable();
    match result.len() {
        0 => Expr::Const(0),
        1 => result.pop().expect("single result term"),
        _ => canonical_node(Expr::Add(result), mask),
    }
}

fn sum2(first: Expr, second: Expr, mask: u64) -> Expr {
    match (&first, &second) {
        (Expr::Const(0), _) => second,
        (_, Expr::Const(0)) => first,
        _ => canonical_node(Expr::Add(vec![first, second]), mask),
    }
}

fn keep_best(current: &Expr, best: &mut Option<Expr>, candidate: Expr) {
    let size = candidate.size();
    if size >= current.size() {
        return;
    }
    let take = match best {
        None => true,
        Some(previous) => {
            size < previous.size()
                || (size == previous.size() && candidate.cmp(previous) == std::cmp::Ordering::Less)
        }
    };
    if take {
        *best = Some(candidate);
    }
}

/// Exact local contractions on the coefficient function of the canonical meet
/// polynomial.  Terms outside the selected support face are untouched.
///
/// No rule depends on source syntax.  There is no enumeration of subsets:
/// each degree-2/3 resident monomial induces only a constant number of support
/// lookups.
fn higher_order_algebraic_in_add(
    e: &Expr,
    mask: u64,
    factor_sets: &mut [Option<Option<FactorSet>>],
) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() < 4
        || !terms.iter().any(|term| {
            matches!(scaled_term(term, mask).1, Expr::And(factors) if matches!(factors.len(), 2 | 3))
        })
    {
        return None;
    }
    let mut supports = HashMap::default();
    let mut constants = HashMap::default();
    for (index, term) in terms.iter().enumerate() {
        ensure_factor_set(term, mask, &mut factor_sets[index]);
        if let Some((coefficient, factors)) = factor_sets[index].as_ref().and_then(Option::as_ref) {
            supports
                .entry(factors.clone())
                .or_insert((index, *coefficient));
        }
        if let Some(coefficient) = constant_coefficient(term, mask) {
            constants.entry(coefficient).or_insert(index);
        }
    }
    let mut best = None;

    // c(a|b|d) = c(a+b+d-ab-ad-bd+abd).
    for top_index in 0..terms.len() {
        ensure_factor_set(&terms[top_index], mask, &mut factor_sets[top_index]);
        let Some(top) = factor_sets[top_index]
            .as_ref()
            .and_then(Option::as_ref)
            .cloned()
        else {
            continue;
        };
        if top.1.len() != 3 || top.0 == 0 {
            continue;
        }
        let c = top.0;
        let neg = c.wrapping_neg() & mask;
        let a = top.1[0].clone();
        let b = top.1[1].clone();
        let d = top.1[2].clone();
        let sa = vec![a.clone()];
        let sb = vec![b.clone()];
        let sd = vec![d.clone()];
        let sab = support(vec![a.clone(), b.clone()]);
        let sad = support(vec![a.clone(), d.clone()]);
        let sbd = support(vec![b.clone(), d.clone()]);
        if let (Some(ia), Some(ib), Some(id), Some(iab), Some(iad), Some(ibd)) = (
            find_support(&supports, &sa, c),
            find_support(&supports, &sb, c),
            find_support(&supports, &sd, c),
            find_support(&supports, &sab, neg),
            find_support(&supports, &sad, neg),
            find_support(&supports, &sbd, neg),
        ) {
            let section = scaled_output(c, Expr::Or(vec![a.clone(), b.clone(), d.clone()]), mask);
            let candidate = replace_indices(
                terms,
                &[top_index, ia, ib, id, iab, iad, ibd],
                section,
                mask,
            );
            keep_best(e, &mut best, candidate);
        }

        // c*((a&~b)|d) = c(a+d-ab-ad+abd).
        let u = [a.clone(), b.clone(), d.clone()];
        for bi in 0..3 {
            for di in 0..3 {
                if bi == di {
                    continue;
                }
                let ai = 3 - bi - di;
                let a = u[ai].clone();
                let b = u[bi].clone();
                let d = u[di].clone();
                let sa = vec![a.clone()];
                let sd = vec![d.clone()];
                let sab = support(vec![a.clone(), b.clone()]);
                let sad = support(vec![a.clone(), d.clone()]);
                if let (Some(ia), Some(id), Some(iab), Some(iad)) = (
                    find_support(&supports, &sa, c),
                    find_support(&supports, &sd, c),
                    find_support(&supports, &sab, neg),
                    find_support(&supports, &sad, neg),
                ) {
                    let left = Expr::And(vec![a.clone(), !b.clone()]);
                    let section = scaled_output(c, Expr::Or(vec![left, d.clone()]), mask);
                    let candidate =
                        replace_indices(terms, &[top_index, ia, id, iab, iad], section, mask);
                    keep_best(e, &mut best, candidate);
                }
            }
        }

        // c*(~a ^ (~b&d)) =
        //   -c - c*a - c*d + 2c*(a&d) + c*(b&d) - 2c*(a&b&d).
        for ai in 0..3 {
            for bi in 0..3 {
                if ai == bi {
                    continue;
                }
                let di = 3 - ai - bi;
                let a = u[ai].clone();
                let b = u[bi].clone();
                let d = u[di].clone();
                let sa = vec![a.clone()];
                let sd = vec![d.clone()];
                let sad = support(vec![a.clone(), d.clone()]);
                let sbd = support(vec![b.clone(), d.clone()]);
                let Some((ibd, c)) = find_support_any(&supports, &sbd) else {
                    continue;
                };
                if c == 0 || top.0 != c.wrapping_mul(2).wrapping_neg() & mask {
                    continue;
                }
                let neg = c.wrapping_neg() & mask;
                let two = c.wrapping_mul(2) & mask;
                if let (Some(iconst), Some(ia), Some(id), Some(iad)) = (
                    constants.get(&neg).copied(),
                    find_support(&supports, &sa, neg),
                    find_support(&supports, &sd, neg),
                    find_support(&supports, &sad, two),
                ) {
                    let rhs = Expr::And(vec![!b.clone(), d.clone()]);
                    let section = scaled_output(c, Expr::Xor(vec![!a.clone(), rhs]), mask);
                    let candidate = replace_indices(
                        terms,
                        &[top_index, iconst, ia, id, iad, ibd],
                        section,
                        mask,
                    );
                    keep_best(e, &mut best, candidate);
                }
            }
        }
    }

    // cx*x + cy*y - 2cx*(x&y) + cy
    //   = (-cx)*~(x^y) + (cx-cy)*~y.
    for pair_index in 0..terms.len() {
        ensure_factor_set(&terms[pair_index], mask, &mut factor_sets[pair_index]);
        let Some(pair) = factor_sets[pair_index]
            .as_ref()
            .and_then(Option::as_ref)
            .cloned()
        else {
            continue;
        };
        if pair.1.len() != 2 || pair.0 == 0 {
            continue;
        }
        for flip in 0..2 {
            let x = pair.1[flip].clone();
            let y = pair.1[1 - flip].clone();
            let sx = vec![x.clone()];
            let sy = vec![y.clone()];
            let Some((ix, cx)) = find_support_any(&supports, &sx) else {
                continue;
            };
            let Some((iy, cy)) = find_support_any(&supports, &sy) else {
                continue;
            };
            if cx == 0 || pair.0 != cx.wrapping_mul(2).wrapping_neg() & mask {
                continue;
            }
            let Some(iconst) = constants.get(&cy).copied() else {
                continue;
            };
            let xnor = !Expr::Xor(vec![x.clone(), y.clone()]);
            let first = scaled_output(cx.wrapping_neg() & mask, xnor, mask);
            let second = scaled_output(cx.wrapping_sub(cy) & mask, !y.clone(), mask);
            let section = sum2(first, second, mask);
            let candidate = replace_indices(terms, &[pair_index, ix, iy, iconst], section, mask);
            keep_best(e, &mut best, candidate);
        }
    }

    best
}

/// `kX + kY - k(X&Y)` -> `k*(X|Y)`, and
/// `kX + kY - 2*k(X&Y)` -> `k*(X^Y)`.
///
/// `X`, `Y`, and `X&Y` are compared as factor sets, so the identity does not
/// depend on how an associative conjunction happened to be grouped.
type FactorSet = (u64, Vec<Expr>);

fn union_replacement(
    first: &FactorSet,
    second: &FactorSet,
    relation: &FactorSet,
    mask: u64,
) -> Option<Expr> {
    let first_coefficient = first.0;
    let second_coefficient = second.0;
    let relation_coefficient = relation.0;
    let minus_k = first_coefficient.wrapping_neg() & mask;
    let minus_two_k = first_coefficient.wrapping_mul(2).wrapping_neg() & mask;
    if first_coefficient == 0
        || first_coefficient != second_coefficient
        || (relation_coefficient != minus_k && relation_coefficient != minus_two_k)
    {
        return None;
    }

    let first_set = &first.1;
    let second_set = &second.1;
    let relation_set = &relation.1;

    let mut expected_relation = first_set.clone();
    expected_relation.extend(second_set.iter().cloned());
    expected_relation.sort_unstable();
    expected_relation.dedup();
    if relation_set != &expected_relation {
        return None;
    }

    let common = factor_intersection(first_set, second_set);
    let first_unique = factor_difference(first_set, &common);
    let second_unique = factor_difference(second_set, &common);
    if first_unique.is_empty() || second_unique.is_empty() {
        return None;
    }

    let first_unique = make_conjunction(first_unique);
    let second_unique = make_conjunction(second_unique);
    let operands = if common.is_empty() {
        None
    } else {
        Some(make_conjunction(common))
    };
    let coefficient = first_coefficient;
    let output = if relation_coefficient == minus_k {
        Expr::Or(vec![first_unique, second_unique])
    } else if relation_coefficient == minus_two_k {
        Expr::Xor(vec![first_unique, second_unique])
    } else {
        return None;
    };

    Some(scaled_output(
        coefficient,
        operands.map_or(output.clone(), |common| Expr::And(vec![common, output])),
        mask,
    ))
}

/// Finds and replaces one fixed three-term union tuple in an `Add`.
fn union_bitwise_in_add(
    e: &Expr,
    mask: u64,
    factor_sets: &mut [Option<Option<FactorSet>>],
) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() < 3 {
        return None;
    }

    for relation in 0..terms.len() {
        for first in 0..terms.len() {
            if first == relation {
                continue;
            }
            for second in (first + 1)..terms.len() {
                if second == relation {
                    continue;
                }

                let (first_coefficient, _) = scaled_term(&terms[first], mask);
                let (second_coefficient, _) = scaled_term(&terms[second], mask);
                let (relation_coefficient, _) = scaled_term(&terms[relation], mask);
                let minus_k = first_coefficient.wrapping_neg() & mask;
                let minus_two_k = first_coefficient.wrapping_mul(2).wrapping_neg() & mask;
                if first_coefficient == 0
                    || first_coefficient != second_coefficient
                    || (relation_coefficient != minus_k && relation_coefficient != minus_two_k)
                {
                    continue;
                }

                ensure_factor_set(&terms[first], mask, &mut factor_sets[first]);
                ensure_factor_set(&terms[second], mask, &mut factor_sets[second]);
                ensure_factor_set(&terms[relation], mask, &mut factor_sets[relation]);
                let Some(first_factor_set) = factor_sets[first].as_ref().and_then(Option::as_ref)
                else {
                    continue;
                };
                let Some(second_factor_set) = factor_sets[second].as_ref().and_then(Option::as_ref)
                else {
                    continue;
                };
                let Some(relation_factor_set) =
                    factor_sets[relation].as_ref().and_then(Option::as_ref)
                else {
                    continue;
                };
                let Some(replacement) = union_replacement(
                    first_factor_set,
                    second_factor_set,
                    relation_factor_set,
                    mask,
                ) else {
                    continue;
                };

                if terms.len() == 3 {
                    return Some(replacement);
                }

                let mut result = Vec::with_capacity(terms.len() - 2);
                for (index, term) in terms.iter().enumerate() {
                    if index != relation && index != first && index != second {
                        result.push(term.clone());
                    }
                }
                result.push(replacement);
                result.sort_unstable();
                return Some(Expr::Add(result));
            }
        }
    }

    None
}

fn factor_set(e: &Expr, mask: u64) -> Option<(u64, Vec<Expr>)> {
    let (coefficient, term) = scaled_term(e, mask);
    let factors = match term {
        Expr::And(operands) => {
            let mut normalized: Vec<_> = operands
                .iter()
                .map(|operand| peel(operand).clone())
                .collect();
            normalized.sort_unstable();
            if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
                return None;
            }
            normalized
        }
        term => vec![term.clone()],
    };
    Some((coefficient, factors))
}

fn ensure_factor_set(term: &Expr, mask: u64, slot: &mut Option<Option<FactorSet>>) {
    if slot.is_none() {
        *slot = Some(factor_set(term, mask));
    }
}

fn factor_intersection(first: &[Expr], second: &[Expr]) -> Vec<Expr> {
    first
        .iter()
        .filter(|factor| second.binary_search(factor).is_ok())
        .cloned()
        .collect()
}

fn factor_difference(first: &[Expr], second: &[Expr]) -> Vec<Expr> {
    first
        .iter()
        .filter(|factor| second.binary_search(factor).is_err())
        .cloned()
        .collect()
}

fn make_conjunction(mut operands: Vec<Expr>) -> Expr {
    match operands.len() {
        1 => operands.pop().expect("one factor must contain its operand"),
        _ => Expr::And(operands),
    }
}

fn complement_operand(operand: Expr) -> Expr {
    match operand {
        Expr::Not(inner) => *inner,
        operand => Expr::Not(Box::new(operand)),
    }
}

/// Selects the smaller of `~(A op B)` and its De Morgan form.
fn not_smaller(inner: Expr) -> Expr {
    let direct = Expr::Not(Box::new(inner.clone()));
    let (operator, children) = match inner {
        Expr::And(children) => (NaryOperator::Or, children),
        Expr::Or(_) => return direct,
        _ => return direct,
    };
    let distributed = flatten_nary(
        operator,
        children.into_iter().map(complement_operand).collect(),
    );
    if distributed.size() < direct.size() {
        distributed
    } else {
        direct
    }
}

/// `k + k*a` -> `-k * ~a`, including the historical `-1 - a` form.
fn complement_replacement(
    constant: &Expr,
    term: &Expr,
    mask: u64,
    choose_smaller: bool,
) -> Option<Expr> {
    let Expr::Const(constant) = peel(constant) else {
        return None;
    };
    let (coefficient, inner) = scaled_term(term, mask);
    let constant = constant & mask;
    (constant == coefficient).then(|| {
        let complement = if choose_smaller {
            not_smaller(inner.clone())
        } else {
            !inner.clone()
        };
        scaled_output(coefficient.wrapping_neg(), complement, mask)
    })
}

/// `k + k*a` -> `-k * ~a` in an exact two-term sum.
fn as_not(e: &Expr, mask: u64) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() != 2 {
        return None;
    }

    complement_replacement(&terms[0], &terms[1], mask, true)
        .or_else(|| complement_replacement(&terms[1], &terms[0], mask, true))
}

/// Finds one scalar complement pair inside a larger sum and folds it once.
fn as_not_in_add(e: &Expr, mask: u64) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() <= 2 {
        return None;
    }

    for first in 0..terms.len() {
        for second in (first + 1)..terms.len() {
            let replacement = complement_replacement(&terms[first], &terms[second], mask, true)
                .or_else(|| complement_replacement(&terms[second], &terms[first], mask, true));
            let Some(replacement) = replacement else {
                continue;
            };

            let mut result = Vec::with_capacity(terms.len() - 1);
            for (index, term) in terms.iter().enumerate() {
                if index != first && index != second {
                    result.push(term.clone());
                }
            }
            result.push(replacement);
            result.sort_unstable();
            return Some(Expr::Add(result));
        }
    }

    None
}

/// `k*a - k*(a & b)` -> `k*(a & ~b)`.
fn difference_replacement(first: &FactorSet, second: &FactorSet, mask: u64) -> Option<Expr> {
    let first_coefficient = first.0;
    let first_factors = &first.1;
    let second_coefficient = second.0;
    let second_factors = &second.1;
    if first_coefficient == 0
        || second_coefficient != (first_coefficient.wrapping_neg() & mask)
        || first_factors.is_empty()
        || !first_factors
            .iter()
            .all(|factor| second_factors.binary_search(factor).is_ok())
        || first_factors.len() == second_factors.len()
    {
        return None;
    }

    let remainder = factor_difference(second_factors, first_factors);
    let mut operands = first_factors.clone();
    operands.push(!make_conjunction(remainder));
    Some(scaled_output(first_coefficient, Expr::And(operands), mask))
}

fn difference_in_add(
    e: &Expr,
    mask: u64,
    factor_sets: &mut [Option<Option<FactorSet>>],
) -> Option<Expr> {
    let Expr::Add(terms) = e else { return None };
    if terms.len() < 2 {
        return None;
    }

    for first in 0..terms.len() {
        for second in 0..terms.len() {
            if first == second {
                continue;
            }

            let (first_coefficient, _) = scaled_term(&terms[first], mask);
            let (second_coefficient, _) = scaled_term(&terms[second], mask);
            if first_coefficient == 0
                || second_coefficient != (first_coefficient.wrapping_neg() & mask)
            {
                continue;
            }

            ensure_factor_set(&terms[first], mask, &mut factor_sets[first]);
            ensure_factor_set(&terms[second], mask, &mut factor_sets[second]);
            let Some(first_factor_set) = factor_sets[first].as_ref().and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(second_factor_set) = factor_sets[second].as_ref().and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(replacement) =
                difference_replacement(first_factor_set, second_factor_set, mask)
            else {
                continue;
            };

            if terms.len() == 2 {
                return Some(replacement);
            }

            let mut result = Vec::with_capacity(terms.len() - 1);
            for (index, term) in terms.iter().enumerate() {
                if index != first && index != second {
                    result.push(term.clone());
                }
            }
            result.push(replacement);
            result.sort_unstable();
            return Some(Expr::Add(result));
        }
    }

    None
}

#[cfg(test)]
mod algebraic_tests {
    use super::*;
    use crate::expr::VarId;

    fn var(id: usize) -> Expr {
        Expr::Var(VarId(id))
    }

    fn monomial(coefficient: u64, factors: Vec<Expr>, mask: u64) -> Expr {
        scaled_output(coefficient, make_conjunction(support(factors)), mask)
    }

    fn sum(mut terms: Vec<Expr>) -> Expr {
        terms.sort_unstable();
        Expr::Add(terms)
    }

    fn check(width: u8, terms: Vec<Expr>, section: Expr, unrelated: Vec<Expr>) {
        let mut source_terms = terms;
        source_terms.extend(unrelated.iter().cloned());
        let source = sum(source_terms);
        let actual = prettify(source.clone(), width);
        let mut expected_terms = unrelated;
        match section {
            Expr::Add(inner) => expected_terms.extend(inner),
            other => expected_terms.push(other),
        }
        let expected = sum(expected_terms);
        assert_eq!(actual, expected);
        assert!(actual.size() < source.size());
        assert!(source.sem_equal(&actual, width, 200).is_ok());
    }

    fn unrelated() -> Vec<Expr> {
        let mut terms = (70..84).map(var).collect::<Vec<_>>();
        terms.push(Expr::And((90..95).map(var).collect()));
        terms
    }

    #[test]
    fn contracts_three_atom_or_with_unrelated_terms() {
        for (width, coefficient) in [(8, 3u64), (32, 7), (64, 11)] {
            let mask = make_mask(width);
            let [a, b, d] = [var(21), var(22), var(23)];
            let negative = coefficient.wrapping_neg() & mask;
            let terms = vec![
                monomial(coefficient, vec![a.clone()], mask),
                monomial(coefficient, vec![b.clone()], mask),
                monomial(coefficient, vec![d.clone()], mask),
                monomial(negative, vec![a.clone(), b.clone()], mask),
                monomial(negative, vec![a.clone(), d.clone()], mask),
                monomial(negative, vec![b.clone(), d.clone()], mask),
                monomial(coefficient, vec![a.clone(), b.clone(), d.clone()], mask),
            ];
            let section = scaled_output(coefficient, Expr::Or(vec![a, b, d]), mask);
            check(width, terms, section, unrelated());
        }
    }

    #[test]
    fn contracts_filtered_or_with_composite_atoms() {
        for (width, coefficient) in [(8, 5u64), (32, 17), (64, 23)] {
            let mask = make_mask(width);
            let a = Expr::Or(vec![var(21), var(22)]);
            let b = Expr::Xor(vec![var(23), var(24)]);
            let d = var(25);
            let negative = coefficient.wrapping_neg() & mask;
            let terms = vec![
                monomial(coefficient, vec![a.clone()], mask),
                monomial(coefficient, vec![d.clone()], mask),
                monomial(negative, vec![a.clone(), b.clone()], mask),
                monomial(negative, vec![a.clone(), d.clone()], mask),
                monomial(coefficient, vec![a.clone(), b.clone(), d.clone()], mask),
            ];
            let section =
                scaled_output(coefficient, Expr::Or(vec![Expr::And(vec![a, !b]), d]), mask);
            check(width, terms, section, unrelated());
        }
    }

    #[test]
    fn contracts_complement_xor_section() {
        for (width, coefficient) in [(8, 3u64), (32, 7), (64, 11)] {
            let mask = make_mask(width);
            let [a, b, d] = [var(21), var(22), var(23)];
            let negative = coefficient.wrapping_neg() & mask;
            let twice = coefficient.wrapping_mul(2) & mask;
            let negative_twice = twice.wrapping_neg() & mask;
            let terms = vec![
                Expr::Const(negative),
                monomial(negative, vec![a.clone()], mask),
                monomial(negative, vec![d.clone()], mask),
                monomial(twice, vec![a.clone(), d.clone()], mask),
                monomial(coefficient, vec![b.clone(), d.clone()], mask),
                monomial(negative_twice, vec![a.clone(), b.clone(), d.clone()], mask),
            ];
            let section = scaled_output(
                coefficient,
                Expr::Xor(vec![!a, Expr::And(vec![!b, d])]),
                mask,
            );
            check(width, terms, section, unrelated());
        }
    }

    #[test]
    fn contracts_xnor_affine_section_only_when_smaller() {
        for (width, cx, cy) in [(8, 5u64, 3u64), (32, 17, 7), (64, 23, 11)] {
            let mask = make_mask(width);
            let x = Expr::Or(vec![var(21), var(22)]);
            let y = Expr::Xor(vec![var(23), var(24)]);
            let terms = vec![
                monomial(cx, vec![x.clone()], mask),
                monomial(cy, vec![y.clone()], mask),
                monomial(
                    cx.wrapping_mul(2).wrapping_neg() & mask,
                    vec![x.clone(), y.clone()],
                    mask,
                ),
                Expr::Const(cy),
            ];
            let section = sum(vec![
                scaled_output(
                    cx.wrapping_neg() & mask,
                    !Expr::Xor(vec![x, y.clone()]),
                    mask,
                ),
                scaled_output(cx.wrapping_sub(cy) & mask, !y, mask),
            ]);
            check(width, terms, section, unrelated());
        }

        let mask = make_mask(8);
        let x = var(21);
        let y = var(22);
        let source = sum(vec![
            monomial(1, vec![x.clone()], mask),
            monomial(2, vec![y.clone()], mask),
            monomial(254, vec![x, y], mask),
            Expr::Const(2),
        ]);
        let mut factor_sets = vec![None; 4];
        assert!(higher_order_algebraic_in_add(&source, mask, &mut factor_sets).is_none());
        assert!(prettify(source.clone(), 8).size() <= source.size());
    }

    #[test]
    fn contracts_difference_with_multifactor_left_side() {
        let mask = make_mask(64);
        let [a, b, d, e] = [var(21), var(22), var(23), var(24)];
        let terms = vec![
            monomial(7, vec![a.clone(), b.clone()], mask),
            monomial(
                7u64.wrapping_neg(),
                vec![a.clone(), b.clone(), d.clone(), e.clone()],
                mask,
            ),
        ];
        let section = scaled_output(7, Expr::And(vec![a, b, !Expr::And(vec![d, e])]), mask);
        check(64, terms, section, unrelated());
    }
}

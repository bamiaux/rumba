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
//! The same fixed-size identities are also recognised inside a larger `Add`.
//! Only one algebraically complete tuple is replaced at a time; there is no
//! arbitrary subset factoring or rewrite search. Results are built through
//! small canonical constructors so this pass does not recreate redundant
//! associative or singleton wrappers.
//!
//! This runs once, on the expression handed back to the caller. It must never
//! run inside the solver's own recursion: the bitwise nodes it produces are
//! larger in [`Expr::size`] terms than the linear forms they replace, so a
//! prettified intermediate would perturb the fixed-point loop's size-based
//! stopping rule.

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

    let e = rewrite_add_tuples(e, mask);

    canonical_node(e, mask)
}

const MAX_REWRITE_ADD_TERMS: usize = 10;

/// Repeatedly contracts the fixed-size identities recognized by this pass.
///
/// Each successful rewrite removes at least one term from the current sum, so
/// this is a bounded normalization of a complete `Add`, not a general rewrite
/// search. Larger sums are left alone; keeping the window explicit prevents
/// prettification from becoming an unbounded simplifier.
fn rewrite_add_tuples(mut e: Expr, mask: u64) -> Expr {
    let Expr::Add(terms) = &e else {
        return as_not(&e, mask).unwrap_or(e);
    };
    if terms.len() > MAX_REWRITE_ADD_TERMS {
        return e;
    }

    loop {
        let Expr::Add(terms) = &e else {
            return e;
        };
        let mut factor_sets = vec![None; terms.len()];
        let replacement = union_bitwise_in_add(&e, mask, &mut factor_sets)
            .or_else(|| difference_in_add(&e, mask, &mut factor_sets))
            .or_else(|| as_not_in_add(&e, mask))
            .or_else(|| as_not(&e, mask));
        let Some(next) = replacement else {
            return e;
        };
        e = canonical_node(next, mask);
        if !matches!(e, Expr::Add(_)) {
            return e;
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
        || first_factors.len() != 1
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

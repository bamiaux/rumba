//! Contextual P9-L substitution in a locally affine multiplication context.

use crate::{
    expr::Expr,
    p9::{Certification, P9Limits, P9Proof, analyze_linear, certify_equivalent},
    simplify::simplify_mba,
    varint::make_mask,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P10bLimits {
    pub p9: P9Limits,
    pub max_mul_nodes: usize,
    pub max_factor_occurrences: usize,
    pub max_pct_attempts: usize,
}

impl Default for P10bLimits {
    fn default() -> Self {
        Self {
            p9: P9Limits::default(),
            max_mul_nodes: 64,
            max_factor_occurrences: 256,
            max_pct_attempts: 64,
        }
    }
}

#[derive(Clone, Debug)]
pub enum P10bProof {
    ZeroCoefficient,
    PctZero,
    ReducedWidth {
        discarded_low_bits: u8,
        proof: P9Proof,
    },
}

#[derive(Clone, Debug)]
pub struct P10bRewrite {
    pub original: Expr,
    pub replacement: Expr,
    pub coefficient: Expr,
    pub proof: P10bProof,
}

#[derive(Clone, Debug)]
pub struct P10bAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub mul_nodes: usize,
    pub factor_occurrences: usize,
    pub nonlinear_factor_projections: usize,
    pub pct_attempts: usize,
    pub reduced_width_attempts: usize,
    pub rewrites: Vec<P10bRewrite>,
    pub budget_exceeded: bool,
}

#[derive(Default)]
struct Stats {
    mul_nodes: usize,
    factor_occurrences: usize,
    nonlinear_factor_projections: usize,
    pct_attempts: usize,
    reduced_width_attempts: usize,
    budget_exceeded: bool,
}

fn contains_mul(expression: &Expr) -> bool {
    match expression {
        Expr::Mul(_) => true,
        Expr::Not(inner) | Expr::Scale(_, inner) => contains_mul(inner),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            terms.iter().any(contains_mul)
        }
        Expr::Var(_) | Expr::Const(_) => false,
    }
}

fn guaranteed_two_adic_valuation(expression: &Expr, width: u8) -> u8 {
    let mask = make_mask(width);
    match expression {
        Expr::Const(constant) => {
            let value = constant.get(mask);
            if value == 0 {
                width
            } else {
                (value.trailing_zeros() as u8).min(width)
            }
        }
        Expr::Scale(coefficient, _) => {
            let value = coefficient.get(mask);
            if value == 0 {
                width
            } else {
                (value.trailing_zeros() as u8).min(width)
            }
        }
        Expr::Mul(terms) => terms.iter().fold(0u8, |valuation, term| {
            valuation
                .saturating_add(guaranteed_two_adic_valuation(term, width))
                .min(width)
        }),
        Expr::Var(_)
        | Expr::Not(_)
        | Expr::And(_)
        | Expr::Or(_)
        | Expr::Xor(_)
        | Expr::Add(_) => 0,
    }
}

fn coefficient_from_other_factors(terms: &[Expr], skipped: usize, mask: u64) -> Expr {
    let mut factors = terms
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != skipped)
        .map(|(_, term)| term.clone())
        .collect::<Vec<_>>();
    match factors.len() {
        0 => Expr::make_const(1),
        1 => factors.pop().unwrap(),
        _ => Expr::Mul(factors),
    }
    .reduce(mask)
}

fn prove_contextual_equality(
    original: &Expr,
    replacement: &Expr,
    coefficient: &Expr,
    width: u8,
    limits: P10bLimits,
    stats: &mut Stats,
) -> Option<P10bProof> {
    let valuation = guaranteed_two_adic_valuation(coefficient, width);
    if valuation == width {
        return Some(P10bProof::ZeroCoefficient);
    }
    if stats.pct_attempts < limits.max_pct_attempts {
        stats.pct_attempts += 1;
        let residual = Expr::Mul(vec![
            coefficient.clone(),
            original.clone() - replacement.clone(),
        ]);
        if simplify_mba(residual, width) == Ok(Expr::zero()) {
            return Some(P10bProof::PctZero);
        }
    } else {
        stats.budget_exceeded = true;
    }
    if valuation == 0 {
        return None;
    }
    stats.reduced_width_attempts += 1;
    let reduced_width = width - valuation;
    match certify_equivalent(
        original,
        replacement,
        reduced_width,
        limits.p9,
    ) {
        Certification::ProvedEquivalent(proof) => Some(P10bProof::ReducedWidth {
            discarded_low_bits: valuation,
            proof,
        }),
        Certification::NotEquivalent(_)
        | Certification::Unknown(_)
        | Certification::Unsupported => None,
    }
}

fn rewrite(
    expression: Expr,
    width: u8,
    limits: P10bLimits,
    stats: &mut Stats,
    rewrites: &mut Vec<P10bRewrite>,
) -> Expr {
    let mask = make_mask(width);
    match expression {
        Expr::Mul(mut terms) => {
            stats.mul_nodes += 1;
            if stats.mul_nodes > limits.max_mul_nodes {
                stats.budget_exceeded = true;
                return Expr::Mul(terms);
            }
            for term in &mut terms {
                if contains_mul(term) {
                    *term = rewrite(term.clone(), width, limits, stats, rewrites);
                }
            }
            for index in 0..terms.len() {
                if contains_mul(&terms[index]) {
                    continue;
                }
                stats.factor_occurrences += 1;
                if stats.factor_occurrences > limits.max_factor_occurrences {
                    stats.budget_exceeded = true;
                    break;
                }
                let analysis = analyze_linear(terms[index].clone(), width, limits.p9);
                let (Some(replacement), Certification::NotEquivalent(_)) =
                    (analysis.projection, analysis.certification)
                else {
                    continue;
                };
                stats.nonlinear_factor_projections += 1;
                let coefficient = coefficient_from_other_factors(&terms, index, mask);
                if let Some(proof) = prove_contextual_equality(
                    &terms[index],
                    &replacement,
                    &coefficient,
                    width,
                    limits,
                    stats,
                ) {
                    let original = std::mem::replace(&mut terms[index], replacement.clone());
                    rewrites.push(P10bRewrite {
                        original,
                        replacement,
                        coefficient,
                        proof,
                    });
                }
            }
            Expr::Mul(terms).reduce(mask)
        }
        other => other.map(|child| rewrite(child, width, limits, stats, rewrites)),
    }
}

pub fn analyze_contextual_linearization(
    expression: Expr,
    width: u8,
    limits: P10bLimits,
) -> P10bAnalysis {
    let mut stats = Stats::default();
    let mut rewrites = Vec::new();
    let rewritten = rewrite(
        expression.clone(),
        width,
        limits,
        &mut stats,
        &mut rewrites,
    );
    let simplified = simplify_mba(rewritten.clone(), width).unwrap_or(rewritten);
    let changed = !rewrites.is_empty() && simplified.size() < expression.size();
    P10bAnalysis {
        result: if changed { simplified } else { expression },
        changed,
        mul_nodes: stats.mul_nodes,
        factor_occurrences: stats.factor_occurrences,
        nonlinear_factor_projections: stats.nonlinear_factor_projections,
        pct_attempts: stats.pct_attempts,
        reduced_width_attempts: stats.reduced_width_attempts,
        rewrites,
        budget_exceeded: stats.budget_exceeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::VarId;

    #[test]
    fn reduced_width_proof_allows_only_the_affine_product_occurrence() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let nonlinear = Expr::And(vec![x.clone() + y.clone(), x.clone()]);
        let projection = analyze_linear(nonlinear.clone(), 8, P9Limits::default())
            .projection
            .unwrap();
        let difference = nonlinear.clone() - projection;
        let expression = Expr::Mul(vec![Expr::make_const(128), difference]);
        let analysis = analyze_contextual_linearization(
            expression.clone(),
            8,
            P10bLimits::default(),
        );
        assert_eq!(analysis.result, Expr::zero());
        assert!(analysis.changed);
        assert_eq!(analysis.rewrites.len(), 1);
        assert!(matches!(
            analysis.rewrites[0].proof,
            P10bProof::ReducedWidth {
                discarded_low_bits: 7,
                ..
            }
        ));
    }
}

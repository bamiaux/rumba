//! Bounded alternation between a free polynomial normal form and a bitwise
//! frontier normal form.

use std::collections::BTreeMap;

use crate::{
    expr::{Expr, VarId},
    p9::{Certification, P9Limits, P9Proof, analyze_linear},
    p9_poly::{P9PolyLimits, polynomial_normal_form},
    simplify::{AlternatingBitwiseNormalForm, bitwise_normal_form_for_alternation},
    varint::make_mask,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P10aLimits {
    pub p9: P9Limits,
    pub polynomial: P9PolyLimits,
    pub max_factor_occurrences: usize,
}

impl Default for P10aLimits {
    fn default() -> Self {
        Self {
            p9: P9Limits::default(),
            polynomial: P9PolyLimits::default(),
            max_factor_occurrences: 256,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinearMbaSemanticKey {
    pub width: u8,
    pub variables: Vec<VarId>,
    pub conjunction_coefficients: Vec<u64>,
    pub proof: P9Proof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticBinding {
    pub original: Expr,
    pub canonical: Expr,
    pub key: LinearMbaSemanticKey,
}

#[derive(Clone, Debug)]
pub struct P10aBitwiseStep {
    pub result: Expr,
    pub changed: bool,
    pub attempts: usize,
    pub normalized: usize,
    pub aborts_atom_limit: usize,
    pub aborts_size_limit: usize,
    pub max_frontier_atoms: usize,
}

impl From<AlternatingBitwiseNormalForm> for P10aBitwiseStep {
    fn from(value: AlternatingBitwiseNormalForm) -> Self {
        Self {
            result: value.result,
            changed: value.changed,
            attempts: value.attempts,
            normalized: value.normalized,
            aborts_atom_limit: value.aborts_atom_limit,
            aborts_size_limit: value.aborts_size_limit,
            max_frontier_atoms: value.max_frontier_atoms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PolyBitwisePolyTrace {
    pub poly_first: Option<Expr>,
    pub poly_first_monomials: usize,
    pub bitwise: Option<P10aBitwiseStep>,
    pub poly_final: Option<Expr>,
    pub poly_final_monomials: usize,
}

#[derive(Clone, Debug)]
pub struct BitwisePolyBitwiseTrace {
    pub bitwise_first: P10aBitwiseStep,
    pub poly: Option<Expr>,
    pub poly_monomials: usize,
    pub bitwise_final: Option<P10aBitwiseStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P10aBlocker {
    FactorBudget,
    FactorNotLinear,
    PolynomialBudget,
    BitwiseAtomBudget,
    BitwiseSizeBudget,
    BitwiseNoChange,
    NoCrossLayerCancellation,
}

#[derive(Clone, Debug)]
pub struct P10aAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub keyed_input: Expr,
    pub bindings: Vec<SemanticBinding>,
    pub factor_occurrences: usize,
    pub factors_keyed: usize,
    pub all_factors_keyed: bool,
    pub poly_bitwise_poly: PolyBitwisePolyTrace,
    pub bitwise_poly_bitwise: BitwisePolyBitwiseTrace,
    pub blocker: Option<P10aBlocker>,
}

#[derive(Clone)]
struct CachedBinding {
    canonical: Expr,
    key: Option<LinearMbaSemanticKey>,
}

#[derive(Default)]
struct KeyStats {
    occurrences: usize,
    keyed: usize,
    factor_budget: bool,
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

fn conjunction_coefficients(expression: &Expr, variables: &[VarId], width: u8) -> Vec<u64> {
    let mask = make_mask(width);
    let max_variable = variables.iter().map(|variable| variable.0).max().unwrap_or(0);
    let mut coefficients = (0..(1usize << variables.len()))
        .map(|assignment| {
            let mut values = vec![0u64; max_variable + 1];
            for (position, variable) in variables.iter().enumerate() {
                values[variable.0] = u64::from(assignment & (1usize << position) != 0);
            }
            expression.eval(&values).get(mask)
        })
        .collect::<Vec<_>>();
    for bit in 0..variables.len() {
        for subset in 0..coefficients.len() {
            if subset & (1usize << bit) != 0 {
                coefficients[subset] = coefficients[subset]
                    .wrapping_sub(coefficients[subset ^ (1usize << bit)])
                    & mask;
            }
        }
    }
    coefficients
}

fn key_factor(
    factor: Expr,
    width: u8,
    limits: P10aLimits,
    cache: &mut BTreeMap<Expr, CachedBinding>,
    bindings: &mut Vec<SemanticBinding>,
    stats: &mut KeyStats,
) -> Expr {
    stats.occurrences += 1;
    if stats.occurrences > limits.max_factor_occurrences {
        stats.factor_budget = true;
        return factor;
    }
    if !cache.contains_key(&factor) {
        let analysis = analyze_linear(factor.clone(), width, limits.p9);
        let (canonical, key) = match (analysis.projection, analysis.certification) {
            (Some(canonical), Certification::ProvedEquivalent(proof)) => {
                let mut variables = factor.get_vars().into_iter().collect::<Vec<_>>();
                variables.sort();
                let key = LinearMbaSemanticKey {
                    width,
                    conjunction_coefficients: conjunction_coefficients(
                        &canonical,
                        &variables,
                        width,
                    ),
                    variables,
                    proof,
                };
                (canonical, Some(key))
            }
            _ => (factor.clone(), None),
        };
        cache.insert(factor.clone(), CachedBinding { canonical, key });
    }
    let cached = cache.get(&factor).unwrap();
    if let Some(key) = &cached.key {
        stats.keyed += 1;
        if !bindings.iter().any(|binding| binding.original == factor) {
            bindings.push(SemanticBinding {
                original: factor,
                canonical: cached.canonical.clone(),
                key: key.clone(),
            });
        }
        cached.canonical.clone()
    } else {
        factor
    }
}

fn apply_semantic_keys(
    expression: Expr,
    width: u8,
    limits: P10aLimits,
    cache: &mut BTreeMap<Expr, CachedBinding>,
    bindings: &mut Vec<SemanticBinding>,
    stats: &mut KeyStats,
) -> Expr {
    match expression {
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|factor| {
                    if contains_mul(&factor) {
                        apply_semantic_keys(factor, width, limits, cache, bindings, stats)
                    } else {
                        key_factor(factor, width, limits, cache, bindings, stats)
                    }
                })
                .collect(),
        ),
        other => other.map(|child| {
            apply_semantic_keys(child, width, limits, cache, bindings, stats)
        }),
    }
}

fn poly_step(expression: Expr, width: u8, limits: P10aLimits) -> Option<(Expr, usize)> {
    polynomial_normal_form(expression, width, limits.polynomial).ok()
}

fn bitwise_step(expression: Expr, width: u8) -> P10aBitwiseStep {
    bitwise_normal_form_for_alternation(expression, width).into()
}

fn blocker(
    stats: &KeyStats,
    all_factors_keyed: bool,
    first: &PolyBitwisePolyTrace,
    second: &BitwisePolyBitwiseTrace,
) -> P10aBlocker {
    if stats.factor_budget {
        return P10aBlocker::FactorBudget;
    }
    if !all_factors_keyed {
        return P10aBlocker::FactorNotLinear;
    }
    if first.poly_first.is_none() || first.poly_final.is_none() || second.poly.is_none() {
        return P10aBlocker::PolynomialBudget;
    }
    let bitwise_steps = [first.bitwise.as_ref(), Some(&second.bitwise_first), second.bitwise_final.as_ref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if bitwise_steps.iter().any(|step| step.aborts_atom_limit != 0) {
        return P10aBlocker::BitwiseAtomBudget;
    }
    if bitwise_steps.iter().any(|step| step.aborts_size_limit != 0) {
        return P10aBlocker::BitwiseSizeBudget;
    }
    if bitwise_steps.iter().all(|step| step.normalized == 0) {
        return P10aBlocker::BitwiseNoChange;
    }
    P10aBlocker::NoCrossLayerCancellation
}

pub fn analyze_alternating_normal_forms(
    expression: Expr,
    width: u8,
    limits: P10aLimits,
) -> P10aAnalysis {
    let mut cache = BTreeMap::new();
    let mut bindings = Vec::new();
    let mut stats = KeyStats::default();
    let keyed_input = apply_semantic_keys(
        expression.clone(),
        width,
        limits,
        &mut cache,
        &mut bindings,
        &mut stats,
    )
    .reduce(make_mask(width));
    let all_factors_keyed = stats.occurrences != 0 && stats.occurrences == stats.keyed;

    let poly_first = poly_step(keyed_input.clone(), width, limits);
    let (poly_first_expression, poly_first_monomials) = poly_first
        .clone()
        .unwrap_or_else(|| (keyed_input.clone(), 0));
    let first_bitwise = bitwise_step(poly_first_expression, width);
    let poly_final = poly_step(first_bitwise.result.clone(), width, limits);
    let poly_bitwise_poly = PolyBitwisePolyTrace {
        poly_first: poly_first.map(|step| step.0),
        poly_first_monomials,
        bitwise: Some(first_bitwise),
        poly_final: poly_final.as_ref().map(|step| step.0.clone()),
        poly_final_monomials: poly_final.as_ref().map_or(0, |step| step.1),
    };

    let bitwise_first = bitwise_step(keyed_input.clone(), width);
    let second_poly = poly_step(bitwise_first.result.clone(), width, limits);
    let (second_poly_expression, second_poly_monomials) = second_poly
        .clone()
        .unwrap_or_else(|| (bitwise_first.result.clone(), 0));
    let bitwise_final = second_poly
        .as_ref()
        .map(|_| bitwise_step(second_poly_expression, width));
    let bitwise_poly_bitwise = BitwisePolyBitwiseTrace {
        bitwise_first,
        poly: second_poly.map(|step| step.0),
        poly_monomials: second_poly_monomials,
        bitwise_final,
    };

    let mut candidates = vec![expression.clone()];
    if let Some(candidate) = &poly_bitwise_poly.poly_final {
        candidates.push(candidate.clone());
    }
    if let Some(candidate) = bitwise_poly_bitwise
        .bitwise_final
        .as_ref()
        .map(|step| &step.result)
    {
        candidates.push(candidate.clone());
    }
    candidates.sort_by_key(|candidate| (candidate.size(), candidate.to_string()));
    let candidate = candidates.into_iter().next().unwrap();
    let changed = candidate.size() < expression.size();
    let result = if changed { candidate } else { expression };
    let blocker = (!changed).then(|| {
        blocker(
            &stats,
            all_factors_keyed,
            &poly_bitwise_poly,
            &bitwise_poly_bitwise,
        )
    });

    P10aAnalysis {
        result,
        changed,
        keyed_input,
        bindings,
        factor_occurrences: stats.occurrences,
        factors_keyed: stats.keyed,
        all_factors_keyed,
        poly_bitwise_poly,
        bitwise_poly_bitwise,
        blocker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternation_exposes_a_global_ring_cancellation_after_bitwise_nf() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let left = Expr::Mul(vec![x.clone(), y.clone()]);
        let right = x + y;
        let expression = Expr::Or(vec![left.clone(), right.clone()])
            - left.clone()
            - right.clone()
            + Expr::And(vec![left, right]);

        for width in 2..=6 {
            let analysis = analyze_alternating_normal_forms(
                expression.clone(),
                width,
                P10aLimits::default(),
            );
            assert_eq!(analysis.result, Expr::zero());
            assert!(analysis.changed);
            assert!(analysis.all_factors_keyed);
            assert!(
                analysis
                    .poly_bitwise_poly
                    .bitwise
                    .as_ref()
                    .is_some_and(|step| step.normalized > 0)
            );
        }
    }

    #[test]
    fn semantic_key_contains_width_ordered_variables_coefficients_and_proof() {
        let x = Expr::Var(VarId(3));
        let y = Expr::Var(VarId(1));
        let expression = Expr::Mul(vec![x & y, Expr::Var(VarId(0))]);
        let analysis = analyze_alternating_normal_forms(
            expression,
            32,
            P10aLimits::default(),
        );
        let binding = analysis
            .bindings
            .iter()
            .find(|binding| binding.key.variables.len() == 2)
            .unwrap();
        assert_eq!(binding.key.width, 32);
        assert_eq!(binding.key.variables, vec![VarId(1), VarId(3)]);
        assert_eq!(binding.key.conjunction_coefficients, vec![0, 0, 0, 1]);
        assert!(binding.key.proof.dag_nodes > 0);
    }
}

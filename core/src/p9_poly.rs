//! Bounded polynomial fallback built from certified P9-L factor projections.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use crate::{
    expr::Expr,
    p9::{
        Certification, P9Limits, analyze_linear, normalize_p9_expression,
    },
    simplify::simplify_mba,
    varint::{VarInt, make_mask},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P9PolyLimits {
    pub p9: P9Limits,
    pub max_mul_nodes: usize,
    pub max_factor_occurrences: usize,
    pub max_monomials: usize,
    pub max_degree: usize,
    pub max_candidate_nodes: usize,
}

impl Default for P9PolyLimits {
    fn default() -> Self {
        Self {
            p9: P9Limits::default(),
            max_mul_nodes: 64,
            max_factor_occurrences: 256,
            max_monomials: 256,
            max_degree: 8,
            max_candidate_nodes: 512,
        }
    }
}

#[derive(Clone, Debug)]
pub struct P9PolyAnalysis {
    pub result: Expr,
    pub local_result: Option<Expr>,
    pub pct_result: Option<Expr>,
    pub changed: bool,
    pub mul_nodes: usize,
    pub factor_occurrences: usize,
    pub unique_factors: usize,
    pub factors_proved: usize,
    pub factor_counterexamples: usize,
    pub factor_unknown: usize,
    pub factor_unsupported: usize,
    pub all_factors_linearized: bool,
    pub sparse_monomials: usize,
    pub sparse_budget_exceeded: bool,
    pub pct_attempted: bool,
    pub max_factor_states: usize,
    pub factor_generation_time: Duration,
    pub factor_verification_time: Duration,
    pub sparse_collection_time: Duration,
    pub pct_time: Duration,
}

#[derive(Clone)]
struct FactorProjection {
    projection: Expr,
    certification: Certification,
}

#[derive(Default)]
struct RewriteStats {
    mul_nodes: usize,
    factor_occurrences: usize,
    factors_proved: usize,
    factor_counterexamples: usize,
    factor_unknown: usize,
    factor_unsupported: usize,
    max_factor_states: usize,
    generation_time: Duration,
    verification_time: Duration,
    limit_exceeded: bool,
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

fn certification_states(certification: &Certification) -> usize {
    match certification {
        Certification::ProvedEquivalent(proof) => proof.max_reachable_states,
        Certification::NotEquivalent(counterexample) => counterexample.max_reachable_states,
        Certification::Unknown(unknown) => unknown.max_reachable_states,
        Certification::Unsupported => 0,
    }
}

fn project_factor(
    factor: Expr,
    width: u8,
    limits: P9PolyLimits,
    cache: &mut BTreeMap<Expr, FactorProjection>,
    stats: &mut RewriteStats,
) -> Expr {
    stats.factor_occurrences += 1;
    if stats.factor_occurrences > limits.max_factor_occurrences {
        stats.limit_exceeded = true;
        return factor;
    }
    if !cache.contains_key(&factor) {
        let analysis = analyze_linear(factor.clone(), width, limits.p9);
        stats.generation_time += analysis.candidate_generation_time;
        stats.verification_time += analysis.verification_time;
        cache.insert(
            factor.clone(),
            FactorProjection {
                projection: analysis.projection.unwrap_or_else(|| factor.clone()),
                certification: analysis.certification,
            },
        );
    }
    let cached = &cache[&factor];
    stats.max_factor_states = stats
        .max_factor_states
        .max(certification_states(&cached.certification));
    match &cached.certification {
        Certification::ProvedEquivalent(_) => {
            stats.factors_proved += 1;
            cached.projection.clone()
        }
        Certification::NotEquivalent(_) => {
            stats.factor_counterexamples += 1;
            factor
        }
        Certification::Unknown(_) => {
            stats.factor_unknown += 1;
            factor
        }
        Certification::Unsupported => {
            stats.factor_unsupported += 1;
            factor
        }
    }
}

fn rewrite_factors(
    expression: Expr,
    width: u8,
    limits: P9PolyLimits,
    cache: &mut BTreeMap<Expr, FactorProjection>,
    stats: &mut RewriteStats,
) -> Expr {
    match expression {
        Expr::Mul(terms) => {
            stats.mul_nodes += 1;
            if stats.mul_nodes > limits.max_mul_nodes {
                stats.limit_exceeded = true;
                return Expr::Mul(terms);
            }
            Expr::Mul(
                terms
                    .into_iter()
                    .map(|factor| {
                        if contains_mul(&factor) {
                            rewrite_factors(factor, width, limits, cache, stats)
                        } else {
                            project_factor(factor, width, limits, cache, stats)
                        }
                    })
                    .collect(),
            )
        }
        other => other.map(|child| rewrite_factors(child, width, limits, cache, stats)),
    }
}

#[derive(Clone)]
struct SparsePolynomial {
    terms: BTreeMap<Vec<Expr>, u64>,
    mask: u64,
    max_monomials: usize,
    max_degree: usize,
}

impl SparsePolynomial {
    fn zero(mask: u64, limits: P9PolyLimits) -> Self {
        Self {
            terms: BTreeMap::new(),
            mask,
            max_monomials: limits.max_monomials,
            max_degree: limits.max_degree,
        }
    }

    fn atom(expression: Expr, mask: u64, limits: P9PolyLimits) -> Self {
        let mut result = Self::zero(mask, limits);
        result.terms.insert(vec![expression], 1);
        result
    }

    fn constant(constant: VarInt, mask: u64, limits: P9PolyLimits) -> Self {
        let mut result = Self::zero(mask, limits);
        let constant = constant.get(mask);
        if constant != 0 {
            result.terms.insert(Vec::new(), constant);
        }
        result
    }

    fn add_term(&mut self, monomial: Vec<Expr>, coefficient: u64) -> Result<(), ()> {
        if monomial.len() > self.max_degree {
            return Err(());
        }
        let coefficient = coefficient & self.mask;
        if coefficient == 0 {
            return Ok(());
        }
        let updated = self
            .terms
            .get(&monomial)
            .copied()
            .unwrap_or(0)
            .wrapping_add(coefficient)
            & self.mask;
        if updated == 0 {
            self.terms.remove(&monomial);
        } else {
            self.terms.insert(monomial, updated);
        }
        if self.terms.len() > self.max_monomials {
            return Err(());
        }
        Ok(())
    }

    fn add(mut self, other: Self) -> Result<Self, ()> {
        for (monomial, coefficient) in other.terms {
            self.add_term(monomial, coefficient)?;
        }
        Ok(self)
    }

    fn scale(mut self, coefficient: VarInt) -> Self {
        let coefficient = coefficient.get(self.mask);
        self.terms.retain(|_, value| {
            *value = value.wrapping_mul(coefficient) & self.mask;
            *value != 0
        });
        self
    }

    fn multiply(self, other: Self) -> Result<Self, ()> {
        let mut result = Self::zero(
            self.mask,
            P9PolyLimits {
                max_monomials: self.max_monomials,
                max_degree: self.max_degree,
                ..P9PolyLimits::default()
            },
        );
        for (left_monomial, left_coefficient) in &self.terms {
            for (right_monomial, right_coefficient) in &other.terms {
                let mut monomial = left_monomial.clone();
                monomial.extend(right_monomial.iter().cloned());
                monomial.sort();
                result.add_term(
                    monomial,
                    left_coefficient.wrapping_mul(*right_coefficient) & self.mask,
                )?;
            }
        }
        Ok(result)
    }

    fn from_expression(
        expression: Expr,
        width: u8,
        limits: P9PolyLimits,
    ) -> Result<Self, ()> {
        let mask = make_mask(width);
        match expression {
            Expr::Const(constant) => Ok(Self::constant(constant, mask, limits)),
            Expr::Scale(coefficient, inner) => {
                Ok(Self::from_expression(*inner, width, limits)?.scale(coefficient))
            }
            Expr::Add(terms) => {
                let mut result = Self::zero(mask, limits);
                for term in terms {
                    result = result.add(Self::from_expression(term, width, limits)?)?;
                }
                Ok(result)
            }
            Expr::Mul(terms) => {
                let mut result = Self::constant(VarInt::ONE, mask, limits);
                for factor in terms {
                    result =
                        result.multiply(Self::from_expression(factor, width, limits)?)?;
                }
                Ok(result)
            }
            atom => Ok(Self::atom(atom, mask, limits)),
        }
    }

    fn into_expression(self) -> Expr {
        let mut terms = Vec::with_capacity(self.terms.len());
        for (monomial, coefficient) in self.terms {
            let term = match monomial.len() {
                0 => Expr::make_const(coefficient),
                1 => Expr::scale(VarInt::from(coefficient), monomial.into_iter().next().unwrap()),
                _ => Expr::scale(VarInt::from(coefficient), Expr::Mul(monomial)),
            };
            terms.push(term);
        }
        match terms.len() {
            0 => Expr::zero(),
            1 => terms.pop().unwrap(),
            _ => Expr::Add(terms),
        }
        .reduce(self.mask)
    }
}

fn expression_cost(expression: &Expr) -> (usize, String) {
    (expression.size(), expression.to_string())
}

pub(crate) fn polynomial_normal_form(
    expression: Expr,
    width: u8,
    limits: P9PolyLimits,
) -> Result<(Expr, usize), ()> {
    fn rewrite(
        expression: Expr,
        width: u8,
        limits: P9PolyLimits,
        total_monomials: &mut usize,
    ) -> Result<Expr, ()> {
        fn prepare_arithmetic(
            expression: Expr,
            width: u8,
            limits: P9PolyLimits,
            total_monomials: &mut usize,
        ) -> Result<Expr, ()> {
            match expression {
                Expr::Scale(coefficient, inner) => Ok(Expr::Scale(
                    coefficient,
                    Box::new(prepare_arithmetic(
                        *inner,
                        width,
                        limits,
                        total_monomials,
                    )?),
                )),
                Expr::Add(terms) => Ok(Expr::Add(
                    terms
                        .into_iter()
                        .map(|term| {
                            prepare_arithmetic(term, width, limits, total_monomials)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                Expr::Mul(terms) => Ok(Expr::Mul(
                    terms
                        .into_iter()
                        .map(|term| {
                            prepare_arithmetic(term, width, limits, total_monomials)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => {
                    rewrite(expression, width, limits, total_monomials)
                }
                Expr::Var(_) | Expr::Const(_) => Ok(expression),
            }
        }

        match expression {
            Expr::Not(inner) => Ok(Expr::Not(Box::new(rewrite(
                *inner,
                width,
                limits,
                total_monomials,
            )?))
            .reduce(make_mask(width))),
            Expr::And(terms) => Ok(Expr::And(
                terms
                    .into_iter()
                    .map(|term| rewrite(term, width, limits, total_monomials))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .reduce(make_mask(width))),
            Expr::Or(terms) => Ok(Expr::Or(
                terms
                    .into_iter()
                    .map(|term| rewrite(term, width, limits, total_monomials))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .reduce(make_mask(width))),
            Expr::Xor(terms) => Ok(Expr::Xor(
                terms
                    .into_iter()
                    .map(|term| rewrite(term, width, limits, total_monomials))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .reduce(make_mask(width))),
            Expr::Scale(_, _) | Expr::Add(_) | Expr::Mul(_) => {
                let prepared = prepare_arithmetic(
                    expression,
                    width,
                    limits,
                    total_monomials,
                )?;
                let polynomial = SparsePolynomial::from_expression(prepared, width, limits)?;
                *total_monomials = total_monomials.saturating_add(polynomial.terms.len());
                let candidate = polynomial.into_expression();
                if candidate.size() > limits.max_candidate_nodes {
                    Err(())
                } else {
                    Ok(candidate)
                }
            }
            Expr::Var(_) | Expr::Const(_) => Ok(expression),
        }
    }

    let mut total_monomials = 0;
    let result = rewrite(expression, width, limits, &mut total_monomials)?;
    Ok((result, total_monomials))
}

pub fn analyze_poly(expression: Expr, width: u8, limits: P9PolyLimits) -> P9PolyAnalysis {
    let original = expression;
    let normalized = normalize_p9_expression(original.clone(), width);
    let mut cache = BTreeMap::new();
    let mut rewrite_stats = RewriteStats::default();
    let replaced = rewrite_factors(
        normalized,
        width,
        limits,
        &mut cache,
        &mut rewrite_stats,
    );

    let sparse_started = Instant::now();
    let sparse = if rewrite_stats.limit_exceeded {
        Err(())
    } else {
        SparsePolynomial::from_expression(replaced.clone(), width, limits)
    };
    let sparse_collection_time = sparse_started.elapsed();
    let mut sparse_budget_exceeded = sparse.is_err() || rewrite_stats.limit_exceeded;
    let (local_result, sparse_monomials) = match sparse {
        Ok(polynomial) => {
            let monomials = polynomial.terms.len();
            let candidate = polynomial.into_expression();
            if candidate.size() <= limits.max_candidate_nodes {
                (Some(candidate), monomials)
            } else {
                sparse_budget_exceeded = true;
                (None, monomials)
            }
        }
        Err(()) => (None, 0),
    };

    let pct_input = local_result.clone().unwrap_or(replaced);
    // PCT is the fallback: do not pay for it when direct sparse collection has
    // already produced a strictly smaller exact representation.
    let pct_attempted = !sparse_budget_exceeded
        && local_result
            .as_ref()
            .is_none_or(|candidate| candidate.size() >= original.size());
    let pct_started = Instant::now();
    let pct_result = pct_attempted
        .then(|| simplify_mba(pct_input.clone(), width).ok())
        .flatten();
    let pct_time = pct_started.elapsed();

    let mut candidates = vec![original.clone()];
    if let Some(candidate) = &local_result {
        candidates.push(candidate.clone());
    }
    if let Some(candidate) = &pct_result {
        candidates.push(candidate.clone());
    }
    candidates.sort_by_key(expression_cost);
    let candidate = candidates.into_iter().next().unwrap();
    let changed = candidate.size() < original.size();
    let result = if changed { candidate } else { original.clone() };

    P9PolyAnalysis {
        result,
        local_result,
        pct_result,
        changed,
        mul_nodes: rewrite_stats.mul_nodes,
        factor_occurrences: rewrite_stats.factor_occurrences,
        unique_factors: cache.len(),
        factors_proved: rewrite_stats.factors_proved,
        factor_counterexamples: rewrite_stats.factor_counterexamples,
        factor_unknown: rewrite_stats.factor_unknown,
        factor_unsupported: rewrite_stats.factor_unsupported,
        all_factors_linearized: rewrite_stats.factor_occurrences != 0
            && rewrite_stats.factors_proved == rewrite_stats.factor_occurrences,
        sparse_monomials,
        sparse_budget_exceeded,
        pct_attempted,
        max_factor_states: rewrite_stats.max_factor_states,
        factor_generation_time: rewrite_stats.generation_time,
        factor_verification_time: rewrite_stats.verification_time,
        sparse_collection_time,
        pct_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::VarId;

    fn variable(index: usize) -> Expr {
        Expr::Var(VarId(index))
    }

    #[test]
    fn sparse_collection_cancels_a_polynomial_identity() {
        let x = variable(0);
        let y = variable(1);
        let expression = Expr::Mul(vec![x.clone() + y.clone(), x.clone() - y.clone()])
            - Expr::Mul(vec![x.clone(), x])
            + Expr::Mul(vec![y.clone(), y]);
        for width in 2..=6 {
            let analysis = analyze_poly(expression.clone(), width, P9PolyLimits::default());
            assert_eq!(analysis.result, Expr::zero());
            assert!(analysis.changed);
            assert!(analysis.all_factors_linearized);
            assert!(!analysis.sparse_budget_exceeded);
            assert!(!analysis.pct_attempted);
        }
    }

    #[test]
    fn a_non_linear_factor_is_never_replaced_without_a_certificate() {
        let x = variable(0);
        let y = variable(1);
        let z = variable(2);
        let nonlinear_factor = Expr::And(vec![x.clone() + y.clone(), z.clone()]);
        let expression = Expr::Mul(vec![nonlinear_factor, x - z]);

        for width in 2..=5 {
            let mask = make_mask(width);
            let analysis = analyze_poly(expression.clone(), width, P9PolyLimits::default());
            assert!(analysis.factor_counterexamples > 0);
            assert!(!analysis.all_factors_linearized);
            for x_value in 0..=mask {
                for y_value in 0..=mask {
                    for z_value in 0..=mask {
                        let values = [x_value, y_value, z_value];
                        assert_eq!(
                            expression.eval(&values).get(mask),
                            analysis.result.eval(&values).get(mask)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sparse_budget_failure_preserves_an_exact_candidate_choice() {
        let x = variable(0);
        let y = variable(1);
        let expression = Expr::Mul(vec![x.clone() + y.clone(), x + y]);
        let analysis = analyze_poly(
            expression.clone(),
            64,
            P9PolyLimits {
                max_monomials: 1,
                ..P9PolyLimits::default()
            },
        );
        assert!(analysis.sparse_budget_exceeded);
        assert_eq!(analysis.result, expression);
        for values in [[0, 0], [1, 2], [u64::MAX, 17]] {
            assert_eq!(
                expression.eval(&values).get(u64::MAX),
                analysis.result.eval(&values).get(u64::MAX)
            );
        }
    }

    #[test]
    fn factor_variable_budget_is_reported_without_projection_expansion() {
        let variables = (0..4).map(variable).collect::<Vec<_>>();
        let expression = Expr::Mul(vec![
            Expr::Add(variables.clone()),
            variables[0].clone(),
        ]);
        let analysis = analyze_poly(expression.clone(), 2, P9PolyLimits::default());
        assert!(analysis.factor_unknown > 0);
        assert!(!analysis.all_factors_linearized);

        for assignment in 0..256usize {
            let values = (0..4)
                .map(|position| ((assignment >> (2 * position)) & 3) as u64)
                .collect::<Vec<_>>();
            assert_eq!(
                expression.eval(&values).get(3),
                analysis.result.eval(&values).get(3)
            );
        }
    }
}

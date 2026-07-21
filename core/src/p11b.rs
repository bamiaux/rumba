//! Bounded observational grammar synthesis for carry-shaped expressions.
//!
//! P11b is an experiment for the no-word-multiplication blockers left after
//! P7e.  The source alone supplies the observation signature and variables.
//! Grammar matches are only a filter: every retained result is certified by
//! the frozen prover below P11 before it may replace the source.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    expr::Expr,
    p11a::{FrozenZeroProof, prove_zero_without_p11},
    p9::P9Limits,
    varint::{VarInt, make_mask},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P11bLimits {
    pub observation_samples: usize,
    pub max_bases: usize,
    pub max_pairs: usize,
    pub max_sums: usize,
    pub max_candidates: usize,
    pub max_certifications: usize,
    pub max_candidate_nodes: usize,
    pub p9: P9Limits,
}

impl Default for P11bLimits {
    fn default() -> Self {
        Self {
            observation_samples: 16,
            max_bases: 16,
            max_pairs: 1_024,
            max_sums: 65_536,
            max_candidates: 2_000_000,
            max_certifications: 32,
            max_candidate_nodes: 64,
            p9: P9Limits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P11bFailure {
    WordMultiplication,
    BaseBudget,
    PairBudget,
    SumBudget,
    CandidateBudget,
    CertificationBudget,
    NoCandidate,
}

#[derive(Clone, Debug)]
pub struct P11bAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub bases: usize,
    pub bitwise_pairs: usize,
    pub additive_surfaces: usize,
    pub candidates_generated: usize,
    pub observation_matches: usize,
    pub certifications_attempted: usize,
    pub certifications_proved: usize,
    pub counterexamples: usize,
    pub unknown: usize,
    pub proof_verified: bool,
    pub failure: Option<P11bFailure>,
}

#[derive(Clone)]
struct ObservedExpr {
    expression: Expr,
    signature: Vec<u64>,
}

#[derive(Clone, Copy)]
enum BitwiseOp {
    And,
    Or,
    Xor,
}

impl BitwiseOp {
    const ALL: [Self; 3] = [Self::And, Self::Or, Self::Xor];

    fn expression(self, left: Expr, right: Expr) -> Expr {
        match self {
            Self::And => surface_and(vec![left, right]),
            Self::Or => surface_or(vec![left, right]),
            Self::Xor => surface_xor(vec![left, right]),
        }
    }

    fn signature(self, left: &[u64], right: &[u64]) -> Vec<u64> {
        left.iter()
            .zip(right)
            .map(|(left, right)| match self {
                Self::And => left & right,
                Self::Or => left | right,
                Self::Xor => left ^ right,
            })
            .collect()
    }
}

fn surface_add(terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for term in terms {
        match term {
            Expr::Add(children) => flat.extend(children),
            Expr::Const(VarInt::ZERO) => {}
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => Expr::zero(),
        1 => flat.pop().unwrap(),
        _ => Expr::Add(flat),
    }
}

fn surface_and(terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for term in terms {
        match term {
            Expr::And(children) => flat.extend(children),
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => Expr::Const(VarInt::MAX),
        1 => flat.pop().unwrap(),
        _ => Expr::And(flat),
    }
}

fn surface_or(terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for term in terms {
        match term {
            Expr::Or(children) => flat.extend(children),
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => Expr::zero(),
        1 => flat.pop().unwrap(),
        _ => Expr::Or(flat),
    }
}

fn surface_xor(terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for term in terms {
        match term {
            Expr::Xor(children) => flat.extend(children),
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => Expr::zero(),
        1 => flat.pop().unwrap(),
        _ => Expr::Xor(flat),
    }
}

fn contains_word_multiplication(expression: &Expr) -> bool {
    match expression {
        Expr::Mul(_) => true,
        Expr::Not(inner) | Expr::Scale(_, inner) => contains_word_multiplication(inner),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            terms.iter().any(contains_word_multiplication)
        }
        Expr::Var(_) | Expr::Const(_) => false,
    }
}

fn deterministic_values(max_variable: usize, sample: usize) -> Vec<u64> {
    let mut state = 0xd1b5_4a32_d192_ed03u64 ^ sample as u64;
    (0..=max_variable)
        .map(|position| {
            state = state
                .wrapping_add(0x9e37_79b9_7f4a_7c15 ^ position as u64)
                .wrapping_mul(0x94d0_49bb_1331_11eb);
            state ^ (state >> 29)
        })
        .collect()
}

fn observation_inputs(expression: &Expr, samples: usize) -> Vec<Vec<u64>> {
    let max_variable = expression
        .get_vars()
        .into_iter()
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    (0..samples)
        .map(|sample| deterministic_values(max_variable, sample))
        .collect()
}

fn signature(expression: &Expr, inputs: &[Vec<u64>], mask: u64) -> Vec<u64> {
    inputs
        .iter()
        .map(|values| expression.eval(values).get(mask))
        .collect()
}

fn retain_cheapest(
    expressions: impl IntoIterator<Item = Expr>,
    inputs: &[Vec<u64>],
    mask: u64,
) -> Vec<ObservedExpr> {
    let mut by_signature = BTreeMap::<Vec<u64>, Expr>::new();
    for expression in expressions {
        let observed = signature(&expression, inputs, mask);
        by_signature
            .entry(observed)
            .and_modify(|current| {
                if (expression.size(), expression.to_string())
                    < (current.size(), current.to_string())
                {
                    *current = expression.clone();
                }
            })
            .or_insert(expression);
    }
    by_signature
        .into_iter()
        .map(|(signature, expression)| ObservedExpr {
            expression,
            signature,
        })
        .collect()
}

struct Search<'a> {
    source: &'a Expr,
    target: Vec<u64>,
    width: u8,
    limits: P11bLimits,
    best: Option<Expr>,
    candidates_generated: usize,
    observation_matches: usize,
    certifications_attempted: usize,
    certifications_proved: usize,
    counterexamples: usize,
    unknown: usize,
    failure: Option<P11bFailure>,
}

impl Search<'_> {
    fn consider(&mut self, candidate: Expr, observed: Vec<u64>) {
        if self.failure.is_some() || observed != self.target {
            return;
        }
        self.observation_matches += 1;
        if candidate.size() >= self.source.size()
            || candidate.size() > self.limits.max_candidate_nodes
            || self.best.as_ref().is_some_and(|best| {
                (best.size(), best.to_string()) <= (candidate.size(), candidate.to_string())
            })
        {
            return;
        }
        if self.certifications_attempted >= self.limits.max_certifications {
            self.failure = Some(P11bFailure::CertificationBudget);
            return;
        }
        self.certifications_attempted += 1;
        match prove_zero_without_p11(
            self.source.clone() - candidate.clone(),
            self.width,
            self.limits.p9,
        ) {
            FrozenZeroProof::Proved(_) => {
                self.certifications_proved += 1;
                self.best = Some(candidate);
            }
            FrozenZeroProof::Counterexample(_) => self.counterexamples += 1,
            FrozenZeroProof::Unknown => self.unknown += 1,
        }
    }

    fn count_candidate(&mut self) -> bool {
        if self.candidates_generated >= self.limits.max_candidates {
            self.failure = Some(P11bFailure::CandidateBudget);
            false
        } else {
            self.candidates_generated += 1;
            true
        }
    }
}

fn base_expressions(expression: &Expr, mask: u64) -> Vec<Expr> {
    let mut variables = expression.get_vars().into_iter().collect::<Vec<_>>();
    variables.sort();
    let mut bases = BTreeSet::new();
    for variable in variables {
        let variable = Expr::Var(variable);
        bases.insert(variable.clone());
        bases.insert(!variable.clone());
        bases.insert(Expr::scale(VarInt::from(mask), variable.clone()));
        bases.insert(Expr::scale(VarInt::from(2), variable));
    }
    bases.into_iter().collect()
}

fn bitwise_pairs(bases: &[ObservedExpr]) -> Vec<Expr> {
    let mut pairs = BTreeSet::new();
    for left_index in 0..bases.len() {
        for right_index in left_index..bases.len() {
            for operation in BitwiseOp::ALL {
                pairs.insert(operation.expression(
                    bases[left_index].expression.clone(),
                    bases[right_index].expression.clone(),
                ));
            }
        }
    }
    pairs.into_iter().collect()
}

fn additive_surfaces(bases: &[ObservedExpr], pairs: &[ObservedExpr], mask: u64) -> Vec<Expr> {
    let mut sums = BTreeSet::new();
    for pair in pairs {
        sums.insert(pair.expression.clone());
        sums.insert(Expr::scale(VarInt::from(mask), pair.expression.clone()));
        for left_index in 0..bases.len() {
            let left = bases[left_index].expression.clone();
            sums.insert(surface_add(vec![left.clone(), pair.expression.clone()]));
            sums.insert(surface_add(vec![
                left.clone(),
                Expr::scale(VarInt::from(mask), pair.expression.clone()),
            ]));
            for right in bases.iter().skip(left_index) {
                sums.insert(surface_add(vec![
                    left.clone(),
                    right.expression.clone(),
                    pair.expression.clone(),
                ]));
            }
        }
    }
    sums.into_iter().collect()
}

fn search_nested_retention(
    search: &mut Search<'_>,
    bases: &[ObservedExpr],
    sums: &[ObservedExpr],
) {
    for sum in sums {
        let mut clauses = BTreeMap::<Vec<u64>, Expr>::new();
        for base in bases {
            for operation in BitwiseOp::ALL {
                if !search.count_candidate() {
                    return;
                }
                let observed = operation.signature(&base.signature, &sum.signature);
                let expression = operation.expression(
                    base.expression.clone(),
                    sum.expression.clone(),
                );
                search.consider(expression.clone(), observed.clone());
                clauses.entry(observed).or_insert(expression);
            }
        }
        let clauses = clauses
            .into_iter()
            .map(|(signature, expression)| ObservedExpr {
                expression,
                signature,
            })
            .collect::<Vec<_>>();
        for operation in [BitwiseOp::And, BitwiseOp::Or] {
            let eligible = clauses
                .iter()
                .filter(|clause| match operation {
                    BitwiseOp::And => clause
                        .signature
                        .iter()
                        .zip(&search.target)
                        .all(|(clause, target)| clause & target == *target),
                    BitwiseOp::Or => clause
                        .signature
                        .iter()
                        .zip(&search.target)
                        .all(|(clause, target)| clause | target == *target),
                    BitwiseOp::Xor => unreachable!(),
                })
                .collect::<Vec<_>>();
            for left_index in 0..eligible.len() {
                for right in eligible.iter().skip(left_index) {
                    if !search.count_candidate() {
                        return;
                    }
                    let observed = operation.signature(
                        &eligible[left_index].signature,
                        &right.signature,
                    );
                    if observed == search.target {
                        search.consider(
                            operation.expression(
                                eligible[left_index].expression.clone(),
                                right.expression.clone(),
                            ),
                            observed,
                        );
                    }
                }
            }
        }
        let by_signature = clauses
            .iter()
            .map(|clause| (clause.signature.clone(), clause))
            .collect::<BTreeMap<_, _>>();
        for left in &clauses {
            let needed = left
                .signature
                .iter()
                .zip(&search.target)
                .map(|(left, target)| left ^ target)
                .collect::<Vec<_>>();
            if let Some(right) = by_signature.get(&needed)
                && search.count_candidate()
            {
                search.consider(
                    surface_xor(vec![
                        left.expression.clone(),
                        right.expression.clone(),
                    ]),
                    search.target.clone(),
                );
            }
        }
    }
}

fn search_simple_bitwise_composition(search: &mut Search<'_>, sums: &[ObservedExpr]) {
    for operation in [BitwiseOp::And, BitwiseOp::Or] {
        let eligible = sums
            .iter()
            .filter(|surface| match operation {
                BitwiseOp::And => surface
                    .signature
                    .iter()
                    .zip(&search.target)
                    .all(|(surface, target)| surface & target == *target),
                BitwiseOp::Or => surface
                    .signature
                    .iter()
                    .zip(&search.target)
                    .all(|(surface, target)| surface | target == *target),
                BitwiseOp::Xor => unreachable!(),
            })
            .collect::<Vec<_>>();
        for left_index in 0..eligible.len() {
            for right in eligible.iter().skip(left_index) {
                if !search.count_candidate() {
                    return;
                }
                let observed = operation.signature(
                    &eligible[left_index].signature,
                    &right.signature,
                );
                if observed == search.target {
                    search.consider(
                        operation.expression(
                            eligible[left_index].expression.clone(),
                            right.expression.clone(),
                        ),
                        observed,
                    );
                }
            }
        }
    }
}

fn search_correlated_xor(
    search: &mut Search<'_>,
    bases: &[ObservedExpr],
    pairs: &[ObservedExpr],
    inputs: &[Vec<u64>],
    mask: u64,
) {
    let mut pair_xors = BTreeMap::<Vec<u64>, Expr>::new();
    for left_index in 0..pairs.len() {
        for right in pairs.iter().skip(left_index) {
            let observed = BitwiseOp::Xor.signature(
                &pairs[left_index].signature,
                &right.signature,
            );
            let candidate = surface_xor(vec![
                pairs[left_index].expression.clone(),
                right.expression.clone(),
            ]);
            pair_xors
                .entry(observed)
                .and_modify(|current| {
                    if (candidate.size(), candidate.to_string())
                        < (current.size(), current.to_string())
                    {
                        *current = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
    }
    for left_index in 0..bases.len() {
        for right in bases.iter().skip(left_index) {
            let sum = surface_add(vec![
                bases[left_index].expression.clone(),
                right.expression.clone(),
            ]);
            let negated_sum = !sum;
            for base in bases {
                let tail = Expr::scale(
                    VarInt::from(mask),
                    surface_xor(vec![negated_sum.clone(), base.expression.clone()]),
                );
                let tail_signature = signature(&tail, inputs, mask);
                let needed = tail_signature
                    .iter()
                    .zip(&search.target)
                    .map(|(tail, target)| tail ^ target)
                    .collect::<Vec<_>>();
                if let Some(prefix) = pair_xors.get(&needed) {
                    if !search.count_candidate() {
                        return;
                    }
                    search.consider(
                        surface_xor(vec![prefix.clone(), tail]),
                        search.target.clone(),
                    );
                }
            }
        }
    }
}

pub fn synthesize_carry_grammar(
    expression: Expr,
    width: u8,
    limits: P11bLimits,
) -> P11bAnalysis {
    if contains_word_multiplication(&expression) {
        return P11bAnalysis {
            result: expression,
            changed: false,
            bases: 0,
            bitwise_pairs: 0,
            additive_surfaces: 0,
            candidates_generated: 0,
            observation_matches: 0,
            certifications_attempted: 0,
            certifications_proved: 0,
            counterexamples: 0,
            unknown: 0,
            proof_verified: false,
            failure: Some(P11bFailure::WordMultiplication),
        };
    }
    let mask = make_mask(width);
    let inputs = observation_inputs(&expression, limits.observation_samples);
    let target = signature(&expression, &inputs, mask);
    let bases = retain_cheapest(base_expressions(&expression, mask), &inputs, mask);
    if bases.len() > limits.max_bases {
        return failed(expression, bases.len(), 0, 0, P11bFailure::BaseBudget);
    }
    let pairs = retain_cheapest(bitwise_pairs(&bases), &inputs, mask);
    if pairs.len() > limits.max_pairs {
        return failed(
            expression,
            bases.len(),
            pairs.len(),
            0,
            P11bFailure::PairBudget,
        );
    }
    let sums = retain_cheapest(additive_surfaces(&bases, &pairs, mask), &inputs, mask);
    if sums.len() > limits.max_sums {
        return failed(
            expression,
            bases.len(),
            pairs.len(),
            sums.len(),
            P11bFailure::SumBudget,
        );
    }
    let mut search = Search {
        source: &expression,
        target,
        width,
        limits,
        best: None,
        candidates_generated: 0,
        observation_matches: 0,
        certifications_attempted: 0,
        certifications_proved: 0,
        counterexamples: 0,
        unknown: 0,
        failure: None,
    };
    search_nested_retention(&mut search, &bases, &sums);
    if search.failure.is_none() {
        search_simple_bitwise_composition(&mut search, &sums);
    }
    if search.failure.is_none() {
        search_correlated_xor(&mut search, &bases, &pairs, &inputs, mask);
    }
    let best = search.best;
    let changed = best.is_some();
    let result = best.unwrap_or_else(|| expression.clone());
    let failure = search.failure.or((!changed).then_some(P11bFailure::NoCandidate));
    P11bAnalysis {
        result,
        changed,
        bases: bases.len(),
        bitwise_pairs: pairs.len(),
        additive_surfaces: sums.len(),
        candidates_generated: search.candidates_generated,
        observation_matches: search.observation_matches,
        certifications_attempted: search.certifications_attempted,
        certifications_proved: search.certifications_proved,
        counterexamples: search.counterexamples,
        unknown: search.unknown,
        proof_verified: changed && search.certifications_proved != 0,
        failure,
    }
}

fn failed(
    expression: Expr,
    bases: usize,
    bitwise_pairs: usize,
    additive_surfaces: usize,
    failure: P11bFailure,
) -> P11bAnalysis {
    P11bAnalysis {
        result: expression,
        changed: false,
        bases,
        bitwise_pairs,
        additive_surfaces,
        candidates_generated: 0,
        observation_matches: 0,
        certifications_attempted: 0,
        certifications_proved: 0,
        counterexamples: 0,
        unknown: 0,
        proof_verified: false,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_word_multiplication() {
        let expression = Expr::Var(0.into()) * Expr::Var(1.into());
        let analysis = synthesize_carry_grammar(expression.clone(), 64, P11bLimits::default());
        assert_eq!(analysis.result, expression);
        assert_eq!(analysis.failure, Some(P11bFailure::WordMultiplication));
    }

    #[test]
    fn synthesizes_a_nested_carry_surface() {
        let x = Expr::Var(0.into());
        let y = Expr::Var(1.into());
        let compact = x.clone()
            | surface_add(vec![
                x.clone(),
                y.clone(),
                y | Expr::scale(VarInt::from(2), x.clone()),
            ]);
        let expanded = surface_add(vec![
            compact.clone(),
            x.clone(),
            Expr::scale(VarInt::MAX, x),
        ]);
        let analysis = synthesize_carry_grammar(expanded, 8, P11bLimits::default());
        assert!(analysis.proof_verified, "{analysis:?}");
        assert!(analysis.result.size() <= compact.size());
    }
}

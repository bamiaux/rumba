//! Bounded synthesis of `F(L1, L2)` where both parents are independently
//! certified linear MBAs and `F` is one of the sixteen binary bitwise tables.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    expr::Expr,
    p9::{Certification, P9Limits, P9Proof, analyze_linear, certify_equivalent},
    varint::make_mask,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P10cLimits {
    pub p9: P9Limits,
    pub max_subexpressions: usize,
    pub max_parents: usize,
    pub max_pairs: usize,
    pub observation_samples: usize,
    pub max_certifications: usize,
}

impl Default for P10cLimits {
    fn default() -> Self {
        Self {
            p9: P9Limits::default(),
            max_subexpressions: 128,
            max_parents: 32,
            max_pairs: 528,
            observation_samples: 64,
            max_certifications: 1_024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct P10cParent {
    pub expression: Expr,
    pub sources: Vec<Expr>,
    pub proof: P9Proof,
}

#[derive(Clone, Debug)]
pub struct P10cCandidate {
    pub left: Expr,
    pub right: Expr,
    pub truth_table: u8,
    pub expression: Expr,
    pub certification: Certification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P10cFailure {
    SubexpressionBudget,
    ParentBudget,
    PairBudget,
    CertificationBudget,
    NoCandidate,
}

#[derive(Clone, Debug)]
pub struct P10cAnalysis {
    pub result: Expr,
    pub changed: bool,
    pub parents: Vec<P10cParent>,
    pub pairs_considered: usize,
    pub observation_candidates: usize,
    pub certifications: usize,
    pub proved_candidates: Vec<P10cCandidate>,
    pub counterexamples: usize,
    pub unknown: usize,
    pub failure: Option<P10cFailure>,
}

fn collect_subexpressions(expression: &Expr, output: &mut BTreeSet<Expr>) {
    if !output.insert(expression.clone()) {
        return;
    }
    match expression {
        Expr::Not(inner) | Expr::Scale(_, inner) => collect_subexpressions(inner, output),
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => {
            for term in terms {
                collect_subexpressions(term, output);
            }
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn deterministic_values(max_variable: usize, sample: usize) -> Vec<u64> {
    let mut state = 0xa076_1d64_78bd_642fu64 ^ sample as u64;
    (0..=max_variable)
        .map(|position| {
            state = state
                .wrapping_add(0x9e37_79b9_7f4a_7c15 ^ position as u64)
                .wrapping_mul(0xe703_7ed1_a0b4_28db);
            state ^ (state >> 32)
        })
        .collect()
}

fn observed_tables(
    source: &Expr,
    left: &Expr,
    right: &Expr,
    samples: usize,
    mask: u64,
) -> Vec<u8> {
    let max_variable = source
        .get_vars()
        .into_iter()
        .chain(left.get_vars())
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let mut observed = [None; 4];
    for sample in 0..samples {
        let values = deterministic_values(max_variable, sample);
        let source_value = source.eval(&values).get(mask);
        let left_value = left.eval(&values).get(mask);
        let right_value = right.eval(&values).get(mask);
        for bit in 0..mask.count_ones() {
            let assignment = (((left_value >> bit) & 1) | (((right_value >> bit) & 1) << 1))
                as usize;
            let output = (source_value >> bit) & 1 != 0;
            if observed[assignment].is_some_and(|previous| previous != output) {
                return Vec::new();
            }
            observed[assignment] = Some(output);
        }
    }
    (0..16u8)
        .filter(|truth_table| {
            observed.iter().enumerate().all(|(assignment, output)| {
                output.is_none_or(|output| {
                    ((*truth_table >> assignment) & 1 != 0) == output
                })
            })
        })
        .collect()
}

fn compact_binary_function(left: Expr, right: Expr, truth_table: u8, mask: u64) -> Expr {
    match truth_table {
        0b0000 => Expr::zero(),
        0b1111 => Expr::make_const(mask),
        0b1010 => left,
        0b0101 => !left,
        0b1100 => right,
        0b0011 => !right,
        0b1000 => left & right,
        0b1110 => left | right,
        0b0110 => left ^ right,
        0b1001 => !(left ^ right),
        0b0111 => !(left & right),
        0b0001 => !(left | right),
        0b0010 => left & !right,
        0b0100 => !left & right,
        0b1011 => left | !right,
        0b1101 => !left | right,
        _ => unreachable!(),
    }
    .reduce(mask)
}

pub fn analyze_binary_linear_composition(
    expression: Expr,
    width: u8,
    limits: P10cLimits,
) -> P10cAnalysis {
    let mut subexpressions = BTreeSet::new();
    collect_subexpressions(&expression, &mut subexpressions);
    if subexpressions.len() > limits.max_subexpressions {
        return P10cAnalysis {
            result: expression,
            changed: false,
            parents: Vec::new(),
            pairs_considered: 0,
            observation_candidates: 0,
            certifications: 0,
            proved_candidates: Vec::new(),
            counterexamples: 0,
            unknown: 0,
            failure: Some(P10cFailure::SubexpressionBudget),
        };
    }

    let mut parents_by_expression = BTreeMap::<Expr, P10cParent>::new();
    for subexpression in subexpressions {
        let analysis = analyze_linear(subexpression.clone(), width, limits.p9);
        let (Some(parent), Certification::ProvedEquivalent(proof)) =
            (analysis.projection, analysis.certification)
        else {
            continue;
        };
        parents_by_expression
            .entry(parent.clone())
            .and_modify(|entry| entry.sources.push(subexpression.clone()))
            .or_insert(P10cParent {
                expression: parent,
                sources: vec![subexpression],
                proof,
            });
    }
    let mut parents = parents_by_expression.into_values().collect::<Vec<_>>();
    parents.sort_by_key(|parent| (parent.expression.size(), parent.expression.to_string()));
    if parents.len() > limits.max_parents {
        return P10cAnalysis {
            result: expression,
            changed: false,
            parents,
            pairs_considered: 0,
            observation_candidates: 0,
            certifications: 0,
            proved_candidates: Vec::new(),
            counterexamples: 0,
            unknown: 0,
            failure: Some(P10cFailure::ParentBudget),
        };
    }

    let mask = make_mask(width);
    let mut pairs_considered = 0;
    let mut observation_candidates = 0;
    let mut certifications = 0;
    let mut proved_candidates = Vec::new();
    let mut counterexamples = 0;
    let mut unknown = 0;
    let mut failure = None;
    'pairs: for left_index in 0..parents.len() {
        for right_index in left_index..parents.len() {
            pairs_considered += 1;
            if pairs_considered > limits.max_pairs {
                failure = Some(P10cFailure::PairBudget);
                break 'pairs;
            }
            let left = &parents[left_index].expression;
            let right = &parents[right_index].expression;
            for truth_table in observed_tables(
                &expression,
                left,
                right,
                limits.observation_samples,
                mask,
            ) {
                observation_candidates += 1;
                if certifications == limits.max_certifications {
                    failure = Some(P10cFailure::CertificationBudget);
                    break 'pairs;
                }
                certifications += 1;
                let candidate = compact_binary_function(
                    left.clone(),
                    right.clone(),
                    truth_table,
                    mask,
                )
                .reduce(mask);
                let certification =
                    certify_equivalent(&expression, &candidate, width, limits.p9);
                match &certification {
                    Certification::ProvedEquivalent(_) => {
                        proved_candidates.push(P10cCandidate {
                            left: left.clone(),
                            right: right.clone(),
                            truth_table,
                            expression: candidate,
                            certification,
                        });
                    }
                    Certification::NotEquivalent(_) => counterexamples += 1,
                    Certification::Unknown(_) | Certification::Unsupported => unknown += 1,
                }
            }
        }
    }

    let best = proved_candidates
        .iter()
        .map(|candidate| candidate.expression.clone())
        .min_by_key(|candidate| (candidate.size(), candidate.to_string()));
    let changed = best
        .as_ref()
        .is_some_and(|candidate| candidate.size() < expression.size());
    let result = if changed {
        best.unwrap()
    } else {
        expression.clone()
    };
    if failure.is_none() && proved_candidates.is_empty() {
        failure = Some(P10cFailure::NoCandidate);
    }
    P10cAnalysis {
        result,
        changed,
        parents,
        pairs_considered,
        observation_candidates,
        certifications,
        proved_candidates,
        counterexamples,
        unknown,
        failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::VarId;

    #[test]
    fn finds_binary_bitwise_composition_of_certified_linear_parents() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let left = x.clone() + y.clone();
        let right = x - y;
        let expression = left.clone() + right.clone()
            - Expr::scale(
                2u64.into(),
                Expr::And(vec![left.clone(), right.clone()]),
            );
        let analysis = analyze_binary_linear_composition(
            expression.clone(),
            64,
            P10cLimits::default(),
        );
        assert!(
            analysis.changed,
            "parents={} pairs={} observations={} certifications={} proved={} counterexamples={} unknown={} failure={:?}",
            analysis.parents.len(),
            analysis.pairs_considered,
            analysis.observation_candidates,
            analysis.certifications,
            analysis.proved_candidates.len(),
            analysis.counterexamples,
            analysis.unknown,
            analysis.failure,
        );
        assert!(!analysis.proved_candidates.is_empty());
        assert!(matches!(
            certify_equivalent(
                &expression,
                &analysis.result,
                64,
                P9Limits::default()
            ),
            Certification::ProvedEquivalent(_)
        ));
    }
}

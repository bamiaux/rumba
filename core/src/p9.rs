//! Bounded P9 carry-state equivalence certification and linear projection.
//!
//! The production simplifier uses the guarded P9-L projection. The broader
//! analysis APIs remain available for corpus experiments and diagnostics.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};

use crate::{
    expr::{Expr, VarId},
    simplify::conjunction_sum_from_signature,
    varint::{VarInt, make_mask},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P9Limits {
    pub max_variables: usize,
    pub max_dag_nodes: usize,
    pub max_reachable_states: usize,
    pub max_candidates: usize,
}

impl Default for P9Limits {
    fn default() -> Self {
        Self {
            max_variables: 3,
            max_dag_nodes: 256,
            max_reachable_states: 256,
            max_candidates: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    Bitwise,
    Affine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnknownReason {
    VariableBudget,
    DagNodeBudget,
    StateBudget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P9Proof {
    pub max_reachable_states: usize,
    pub dag_nodes: usize,
    pub carry_slots: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P9Counterexample {
    pub values: Vec<u64>,
    pub differing_bit: u8,
    pub expression_bit: u8,
    pub candidate_bit: u8,
    pub max_reachable_states: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P9Unknown {
    pub reason: UnknownReason,
    pub max_reachable_states: usize,
    pub dag_nodes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Certification {
    ProvedEquivalent(P9Proof),
    NotEquivalent(P9Counterexample),
    Unknown(P9Unknown),
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P9Failure {
    NoCandidate,
    CandidateRejected,
    Unsupported,
    BudgetExceeded,
}

#[derive(Clone, Debug)]
pub struct CandidateAttempt {
    pub kind: CandidateKind,
    pub expression: Expr,
    pub certification: Certification,
}

#[derive(Clone, Debug)]
pub struct P9Analysis {
    pub result: Expr,
    pub changed: bool,
    pub failure: Option<P9Failure>,
    pub bitwise_candidate_generated: bool,
    pub affine_candidate_generated: bool,
    pub attempts: Vec<CandidateAttempt>,
    pub candidate_generation_time: Duration,
    pub verification_time: Duration,
}

#[derive(Clone, Debug)]
pub struct P9LinearAnalysis {
    pub result: Expr,
    pub projection: Option<Expr>,
    pub changed: bool,
    pub certification: Certification,
    pub candidate_generation_time: Duration,
    pub verification_time: Duration,
}

#[derive(Clone, Debug)]
enum Node {
    Var(VarId),
    Const(u64),
    Not(usize),
    And(Vec<usize>),
    Or(Vec<usize>),
    Xor(Vec<usize>),
    Add { children: Vec<usize>, carry: usize },
    Scale { coefficient: i128, child: usize, carry: usize },
}

#[derive(Default)]
struct DagBuilder {
    nodes: Vec<Node>,
    node_ids: HashMap<Expr, usize>,
    carry_slots: usize,
    mask: u64,
    width: u8,
}

impl DagBuilder {
    fn new(width: u8) -> Self {
        Self {
            mask: make_mask(width),
            width,
            ..Self::default()
        }
    }

    fn intern(&mut self, expression: &Expr) -> Result<usize, ()> {
        if let Some(id) = self.node_ids.get(expression) {
            return Ok(*id);
        }
        let node = match expression {
            Expr::Var(variable) => Node::Var(*variable),
            Expr::Const(constant) => Node::Const(constant.get(self.mask)),
            Expr::Not(inner) => Node::Not(self.intern(inner)?),
            Expr::And(terms) => Node::And(self.intern_all(terms)?),
            Expr::Or(terms) => Node::Or(self.intern_all(terms)?),
            Expr::Xor(terms) => Node::Xor(self.intern_all(terms)?),
            Expr::Add(terms) => {
                let children = self.intern_all(terms)?;
                let carry = self.new_carry();
                Node::Add { children, carry }
            }
            Expr::Scale(coefficient, inner) => {
                let child = self.intern(inner)?;
                let carry = self.new_carry();
                Node::Scale {
                    coefficient: i128::from(coefficient.get_signed(self.width, self.mask)),
                    child,
                    carry,
                }
            }
            Expr::Mul(_) => return Err(()),
        };
        let id = self.nodes.len();
        self.nodes.push(node);
        self.node_ids.insert(expression.clone(), id);
        Ok(id)
    }

    fn intern_all(&mut self, expressions: &[Expr]) -> Result<Vec<usize>, ()> {
        expressions.iter().map(|expression| self.intern(expression)).collect()
    }

    fn new_carry(&mut self) -> usize {
        let result = self.carry_slots;
        self.carry_slots += 1;
        result
    }
}

fn multiply_coefficients(left: VarInt, right: VarInt, mask: u64) -> VarInt {
    VarInt::from(left.get(mask).wrapping_mul(right.get(mask)) & mask)
}

/// Restores scalar multiplication nodes that may have lost their `Scale` tag
/// while crossing a text boundary. General word-by-word products remain
/// `Mul` and are still outside the P9 fragment.
pub(crate) fn normalize_p9_expression(expression: Expr, width: u8) -> Expr {
    fn scale(coefficient: VarInt, expression: Expr, mask: u64) -> Expr {
        let coefficient = VarInt::from(coefficient.get(mask));
        match expression {
            Expr::Const(inner) => {
                Expr::Const(multiply_coefficients(coefficient, inner, mask))
            }
            Expr::Scale(inner_coefficient, inner) => Expr::scale(
                multiply_coefficients(coefficient, inner_coefficient, mask),
                *inner,
            ),
            other => Expr::scale(coefficient, other),
        }
    }

    let mask = make_mask(width);
    match expression {
        Expr::Var(variable) => Expr::Var(variable),
        Expr::Const(constant) => Expr::Const(VarInt::from(constant.get(mask))),
        Expr::Not(inner) => !normalize_p9_expression(*inner, width),
        Expr::Scale(coefficient, inner) => {
            scale(coefficient, normalize_p9_expression(*inner, width), mask)
        }
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| normalize_p9_expression(term, width))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| normalize_p9_expression(term, width))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| normalize_p9_expression(term, width))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| normalize_p9_expression(term, width))
                .collect(),
        ),
        Expr::Mul(terms) => {
            let mut coefficient = VarInt::ONE;
            let mut factors = Vec::new();
            for term in terms {
                match normalize_p9_expression(term, width) {
                    Expr::Const(constant) => {
                        coefficient = multiply_coefficients(coefficient, constant, mask);
                    }
                    Expr::Scale(inner_coefficient, inner) => {
                        coefficient =
                            multiply_coefficients(coefficient, inner_coefficient, mask);
                        factors.push(*inner);
                    }
                    Expr::Mul(inner) => factors.extend(inner),
                    other => factors.push(other),
                }
            }
            let product = match factors.len() {
                0 => return Expr::Const(VarInt::from(coefficient.get(mask))),
                1 => factors.pop().unwrap(),
                _ => Expr::Mul(factors),
            };
            scale(coefficient, product, mask)
        }
    }
}

struct Machine<'a> {
    nodes: &'a [Node],
    variable_positions: HashMap<VarId, usize>,
    carry_slots: usize,
}

impl Machine<'_> {
    fn step(
        &self,
        root: usize,
        bit_index: u8,
        input: &[u8],
        carries: &[i128],
        next_carries: &mut [i128],
        cache: &mut [Option<u8>],
    ) -> u8 {
        if let Some(bit) = cache[root] {
            return bit;
        }
        let bit = match &self.nodes[root] {
            Node::Var(variable) => input[self.variable_positions[variable]],
            Node::Const(constant) => ((constant >> bit_index) & 1) as u8,
            Node::Not(inner) => 1 - self.step(
                *inner,
                bit_index,
                input,
                carries,
                next_carries,
                cache,
            ),
            Node::And(children) => children.iter().fold(1, |value, child| {
                value
                    & self.step(
                        *child,
                        bit_index,
                        input,
                        carries,
                        next_carries,
                        cache,
                    )
            }),
            Node::Or(children) => children.iter().fold(0, |value, child| {
                value
                    | self.step(
                        *child,
                        bit_index,
                        input,
                        carries,
                        next_carries,
                        cache,
                    )
            }),
            Node::Xor(children) => children.iter().fold(0, |value, child| {
                value
                    ^ self.step(
                        *child,
                        bit_index,
                        input,
                        carries,
                        next_carries,
                        cache,
                    )
            }),
            Node::Add { children, carry } => {
                let total = children.iter().fold(carries[*carry], |value, child| {
                    value
                        + i128::from(self.step(
                            *child,
                            bit_index,
                            input,
                            carries,
                            next_carries,
                            cache,
                        ))
                });
                next_carries[*carry] = total.div_euclid(2);
                total.rem_euclid(2) as u8
            }
            Node::Scale {
                coefficient,
                child,
                carry,
            } => {
                let child_bit = self.step(
                    *child,
                    bit_index,
                    input,
                    carries,
                    next_carries,
                    cache,
                );
                let total = carries[*carry] + *coefficient * i128::from(child_bit);
                next_carries[*carry] = total.div_euclid(2);
                total.rem_euclid(2) as u8
            }
        };
        cache[root] = Some(bit);
        bit
    }

    fn product_step(
        &self,
        left: usize,
        right: usize,
        bit_index: u8,
        input: &[u8],
        carries: &[i128],
    ) -> (u8, u8, Vec<i128>) {
        let mut next_carries = vec![0; self.carry_slots];
        let mut cache = vec![None; self.nodes.len()];
        let left_bit = self.step(
            left,
            bit_index,
            input,
            carries,
            &mut next_carries,
            &mut cache,
        );
        let right_bit = self.step(
            right,
            bit_index,
            input,
            carries,
            &mut next_carries,
            &mut cache,
        );
        (left_bit, right_bit, next_carries)
    }
}

pub fn certify_equivalent(
    expression: &Expr,
    candidate: &Expr,
    width: u8,
    limits: P9Limits,
) -> Certification {
    let expression = normalize_p9_expression(expression.clone(), width);
    let candidate = normalize_p9_expression(candidate.clone(), width);
    let variables = expression
        .get_vars()
        .into_iter()
        .chain(candidate.get_vars())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if variables.len() > limits.max_variables {
        return Certification::Unknown(P9Unknown {
            reason: UnknownReason::VariableBudget,
            max_reachable_states: 0,
            dag_nodes: 0,
        });
    }

    let mut builder = DagBuilder::new(width);
    let Ok(left) = builder.intern(&expression) else {
        return Certification::Unsupported;
    };
    let Ok(right) = builder.intern(&candidate) else {
        return Certification::Unsupported;
    };
    if builder.nodes.len() > limits.max_dag_nodes {
        return Certification::Unknown(P9Unknown {
            reason: UnknownReason::DagNodeBudget,
            max_reachable_states: 0,
            dag_nodes: builder.nodes.len(),
        });
    }
    let machine = Machine {
        nodes: &builder.nodes,
        variable_positions: variables
            .iter()
            .enumerate()
            .map(|(position, variable)| (*variable, position))
            .collect(),
        carry_slots: builder.carry_slots,
    };

    let mut states = BTreeMap::new();
    states.insert(vec![0; builder.carry_slots], vec![0u64; variables.len()]);
    let mut max_reachable_states = 1;
    let alphabet_size = 1usize << variables.len();

    for bit_index in 0..width {
        let mut next_states = BTreeMap::new();
        for (state, witness) in &states {
            for symbol in 0..alphabet_size {
                let input = (0..variables.len())
                    .map(|position| ((symbol >> position) & 1) as u8)
                    .collect::<Vec<_>>();
                let (expression_bit, candidate_bit, next_state) =
                    machine.product_step(left, right, bit_index, &input, state);
                let mut next_witness = witness.clone();
                for (position, input_bit) in input.iter().enumerate() {
                    if *input_bit != 0 {
                        next_witness[position] |= 1u64 << bit_index;
                    }
                }
                if expression_bit != candidate_bit {
                    let max_variable = variables
                        .iter()
                        .map(|variable| variable.0)
                        .max()
                        .unwrap_or(0);
                    let mut values = vec![0; max_variable + 1];
                    for (position, variable) in variables.iter().enumerate() {
                        values[variable.0] = next_witness[position];
                    }
                    return Certification::NotEquivalent(P9Counterexample {
                        values,
                        differing_bit: bit_index,
                        expression_bit,
                        candidate_bit,
                        max_reachable_states,
                    });
                }
                next_states.entry(next_state).or_insert(next_witness);
                if next_states.len() > limits.max_reachable_states {
                    return Certification::Unknown(P9Unknown {
                        reason: UnknownReason::StateBudget,
                        max_reachable_states: next_states.len(),
                        dag_nodes: builder.nodes.len(),
                    });
                }
            }
        }
        max_reachable_states = max_reachable_states.max(next_states.len());
        states = next_states;
    }

    Certification::ProvedEquivalent(P9Proof {
        max_reachable_states,
        dag_nodes: builder.nodes.len(),
        carry_slots: builder.carry_slots,
    })
}

fn fragment_supported(expression: &Expr) -> bool {
    match expression {
        Expr::Var(_) | Expr::Const(_) => true,
        Expr::Not(inner) | Expr::Scale(_, inner) => fragment_supported(inner),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            terms.iter().all(fragment_supported)
        }
        Expr::Mul(_) => false,
    }
}

fn values_for_variables(variables: &[VarId], selected: impl Fn(usize) -> u64) -> Vec<u64> {
    let max_variable = variables.iter().map(|variable| variable.0).max().unwrap_or(0);
    let mut values = vec![0; max_variable + 1];
    for (position, variable) in variables.iter().enumerate() {
        values[variable.0] = selected(position);
    }
    values
}

/// Returns the unique linear-MBA projection matching `expression` on the
/// numeric Boolean cube `{0, 1}^t`.
pub fn generate_linear_projection(expression: &Expr, width: u8) -> Expr {
    let mask = make_mask(width);
    let variables = expression
        .get_vars()
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut signature = Vec::with_capacity(1usize << variables.len());
    for assignment in 0..(1usize << variables.len()) {
        let values = values_for_variables(&variables, |position| {
            u64::from(assignment & (1usize << position) != 0)
        });
        signature.push(expression.eval(&values).get(mask));
    }
    conjunction_sum_from_signature(signature, &variables, mask).reduce(mask)
}

pub fn analyze_linear(expression: Expr, width: u8, limits: P9Limits) -> P9LinearAnalysis {
    let generation_started = Instant::now();
    let original = expression;
    let expression = normalize_p9_expression(original.clone(), width);
    if !fragment_supported(&expression) {
        return P9LinearAnalysis {
            result: original,
            projection: None,
            changed: false,
            certification: Certification::Unsupported,
            candidate_generation_time: generation_started.elapsed(),
            verification_time: Duration::ZERO,
        };
    }
    if expression.get_vars().len() > limits.max_variables {
        return P9LinearAnalysis {
            result: original,
            projection: None,
            changed: false,
            certification: Certification::Unknown(P9Unknown {
                reason: UnknownReason::VariableBudget,
                max_reachable_states: 0,
                dag_nodes: 0,
            }),
            candidate_generation_time: generation_started.elapsed(),
            verification_time: Duration::ZERO,
        };
    }

    let projection = generate_linear_projection(&expression, width);
    let candidate_generation_time = generation_started.elapsed();
    let verification_started = Instant::now();
    let certification = certify_equivalent(&expression, &projection, width, limits);
    let verification_time = verification_started.elapsed();
    let changed = matches!(certification, Certification::ProvedEquivalent(_))
        && projection.size() < expression.size();
    P9LinearAnalysis {
        result: if changed {
            projection.clone()
        } else {
            original
        },
        projection: Some(projection),
        changed,
        certification,
        candidate_generation_time,
        verification_time,
    }
}

/// Returns a certified canonical linear projection only when it is strictly
/// smaller than the exact caller-provided expression. Unsupported inputs,
/// budgets, counterexamples, and non-improving projections preserve the input.
pub(crate) fn simplify_linear_if_smaller(
    expression: Expr,
    width: u8,
    limits: P9Limits,
) -> Expr {
    let original = expression;
    let normalized = normalize_p9_expression(original.clone(), width);
    if !fragment_supported(&normalized) || normalized.get_vars().len() > limits.max_variables {
        return original;
    }
    let projection = generate_linear_projection(&normalized, width);
    if projection.size() >= original.size() {
        return original;
    }
    if matches!(
        certify_equivalent(&normalized, &projection, width, limits),
        Certification::ProvedEquivalent(_)
    ) {
        projection
    } else {
        original
    }
}

pub fn generate_bitwise_candidate(expression: &Expr, width: u8) -> Option<Expr> {
    let mask = make_mask(width);
    let variables = expression
        .get_vars()
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for assignment in 0..(1usize << variables.len()) {
        let values = values_for_variables(&variables, |position| {
            if assignment & (1usize << position) == 0 {
                0
            } else {
                mask
            }
        });
        let output = expression.eval(&values).get(mask);
        if output != 0 && output != mask {
            return None;
        }
    }
    // The {0, mask} cube above proves that a uniform bitwise candidate exists.
    // Its arithmetic conjunction-basis coefficients must then be interpolated
    // on the {0, 1} cube; feeding mask-valued truth outputs into that basis is
    // incorrect, notably for NOT, OR, and XOR.
    let mut signature = Vec::with_capacity(1usize << variables.len());
    for assignment in 0..(1usize << variables.len()) {
        let values = values_for_variables(&variables, |position| {
            u64::from(assignment & (1usize << position) != 0)
        });
        signature.push(expression.eval(&values).get(mask));
    }
    Some(conjunction_sum_from_signature(signature, &variables, mask).reduce(mask))
}

pub fn generate_affine_candidate(expression: &Expr, width: u8) -> Expr {
    let mask = make_mask(width);
    let variables = expression
        .get_vars()
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let zeros = values_for_variables(&variables, |_| 0);
    let constant = expression.eval(&zeros).get(mask);
    let mut terms = Vec::new();
    if constant != 0 {
        terms.push(Expr::make_const(constant));
    }
    for (position, variable) in variables.iter().enumerate() {
        let values = values_for_variables(&variables, |current| {
            u64::from(current == position)
        });
        let coefficient = expression.eval(&values).get(mask).wrapping_sub(constant) & mask;
        if coefficient != 0 {
            terms.push(Expr::scale(VarInt::from(coefficient), Expr::Var(*variable)));
        }
    }
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap(),
        _ => Expr::Add(terms),
    }
    .reduce(mask)
}

fn expression_depth(expression: &Expr) -> usize {
    match expression {
        Expr::Var(_) | Expr::Const(_) => 1,
        Expr::Not(inner) | Expr::Scale(_, inner) => 1 + expression_depth(inner),
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => 1 + terms.iter().map(expression_depth).max().unwrap_or(0),
    }
}

fn arithmetic_nodes(expression: &Expr) -> usize {
    let own = usize::from(matches!(expression, Expr::Add(_) | Expr::Scale(_, _) | Expr::Mul(_)));
    own + match expression {
        Expr::Var(_) | Expr::Const(_) => 0,
        Expr::Not(inner) | Expr::Scale(_, inner) => arithmetic_nodes(inner),
        Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms)
        | Expr::Add(terms)
        | Expr::Mul(terms) => terms.iter().map(arithmetic_nodes).sum(),
    }
}

fn candidate_score(
    candidate: &(CandidateKind, Expr),
) -> (usize, usize, usize, String, CandidateKind) {
    (
        candidate.1.size(),
        expression_depth(&candidate.1),
        arithmetic_nodes(&candidate.1),
        candidate.1.to_string(),
        candidate.0,
    )
}

pub fn analyze(expression: Expr, width: u8, limits: P9Limits) -> P9Analysis {
    let generation_started = Instant::now();
    let original = expression;
    let expression = normalize_p9_expression(original.clone(), width);
    if !fragment_supported(&expression) {
        return P9Analysis {
            result: original,
            changed: false,
            failure: Some(P9Failure::Unsupported),
            bitwise_candidate_generated: false,
            affine_candidate_generated: false,
            attempts: Vec::new(),
            candidate_generation_time: generation_started.elapsed(),
            verification_time: Duration::ZERO,
        };
    }
    if expression.get_vars().len() > limits.max_variables {
        return P9Analysis {
            result: original,
            changed: false,
            failure: Some(P9Failure::BudgetExceeded),
            bitwise_candidate_generated: false,
            affine_candidate_generated: false,
            attempts: Vec::new(),
            candidate_generation_time: generation_started.elapsed(),
            verification_time: Duration::ZERO,
        };
    }

    let bitwise = generate_bitwise_candidate(&expression, width);
    let affine = generate_affine_candidate(&expression, width);
    let bitwise_candidate_generated = bitwise.is_some();
    let affine_candidate_generated = true;
    let mut candidates = Vec::new();
    if let Some(candidate) = bitwise {
        candidates.push((CandidateKind::Bitwise, candidate));
    }
    candidates.push((CandidateKind::Affine, affine));
    candidates.sort_by_key(candidate_score);
    candidates.dedup_by(|left, right| left.1 == right.1);
    candidates.retain(|(_, candidate)| candidate.size() < expression.size());
    candidates.truncate(limits.max_candidates);
    let candidate_generation_time = generation_started.elapsed();
    if candidates.is_empty() {
        return P9Analysis {
            result: original,
            changed: false,
            failure: Some(P9Failure::NoCandidate),
            bitwise_candidate_generated,
            affine_candidate_generated,
            attempts: Vec::new(),
            candidate_generation_time,
            verification_time: Duration::ZERO,
        };
    }

    let mut attempts = Vec::new();
    let mut budget_exceeded = false;
    let verification_started = Instant::now();
    for (kind, candidate) in candidates {
        let certification = certify_equivalent(&expression, &candidate, width, limits);
        let proved = matches!(certification, Certification::ProvedEquivalent(_));
        budget_exceeded |= matches!(certification, Certification::Unknown(_));
        attempts.push(CandidateAttempt {
            kind,
            expression: candidate.clone(),
            certification,
        });
        if proved {
            return P9Analysis {
                result: candidate,
                changed: true,
                failure: None,
                bitwise_candidate_generated,
                affine_candidate_generated,
                attempts,
                candidate_generation_time,
                verification_time: verification_started.elapsed(),
            };
        }
    }
    P9Analysis {
        result: original,
        changed: false,
        failure: Some(if budget_exceeded {
            P9Failure::BudgetExceeded
        } else {
            P9Failure::CandidateRejected
        }),
        bitwise_candidate_generated,
        affine_candidate_generated,
        attempts,
        candidate_generation_time,
        verification_time: verification_started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(index: usize) -> Expr {
        Expr::Var(VarId(index))
    }

    fn assert_exhaustively_equal(left: &Expr, right: &Expr, width: u8, variables: usize) {
        let mask = make_mask(width);
        let domain = 1usize << width;
        for packed in 0..domain.pow(variables as u32) {
            let mut value = packed;
            let assignments = (0..variables)
                .map(|_| {
                    let current = (value % domain) as u64;
                    value /= domain;
                    current
                })
                .collect::<Vec<_>>();
            assert_eq!(left.eval(&assignments).get(mask), right.eval(&assignments).get(mask));
        }
    }

    #[test]
    fn exact_carry_machine_covers_required_operations_at_small_widths() {
        let x = variable(0);
        let y = variable(1);
        for width in 1..=6 {
            let mask = make_mask(width);
            let cases = [
                (x.clone() + y.clone(), y.clone() + x.clone()),
                (x.clone() - y.clone(), x.clone() + VarInt::MAX * y.clone()),
                (VarInt::MAX * x.clone(), VarInt::from(mask) * x.clone()),
                (3u64 * (x.clone() + y.clone()), 3u64 * x.clone() + 3u64 * y.clone()),
                (
                    VarInt::from(u64::MAX - 2) * (x.clone() + y.clone()),
                    VarInt::from(u64::MAX - 2) * x.clone()
                        + VarInt::from(u64::MAX - 2) * y.clone(),
                ),
                (x.clone() & y.clone(), y.clone() & x.clone()),
                (x.clone() | y.clone(), y.clone() | x.clone()),
                (x.clone() ^ y.clone(), y.clone() ^ x.clone()),
                (!x.clone(), Expr::make_const(mask) - x.clone()),
                ((x.clone() + y.clone()) & !x.clone(), (y.clone() + x.clone()) & !x.clone()),
            ];
            for (left, right) in cases {
                assert!(matches!(
                    certify_equivalent(&left, &right, width, P9Limits::default()),
                    Certification::ProvedEquivalent(_)
                ));
                assert_exhaustively_equal(&left, &right, width, 2);
            }
        }
    }

    #[test]
    fn counterexample_is_replayable_by_word_evaluator() {
        let expression = variable(0) + variable(1);
        let candidate = variable(0) ^ variable(1);
        let Certification::NotEquivalent(counterexample) = certify_equivalent(
            &expression,
            &candidate,
            6,
            P9Limits::default(),
        ) else {
            panic!("expected a counterexample");
        };
        let mask = make_mask(6);
        assert_ne!(
            expression.eval(&counterexample.values).get(mask),
            candidate.eval(&counterexample.values).get(mask)
        );
    }

    #[test]
    fn generated_candidates_are_sample_checked_at_32_and_64_bits() {
        let x = variable(0);
        let y = variable(1);
        let expressions = [
            (x.clone() & y.clone()) + (x.clone() | y.clone()),
            7u64 * x.clone() + 11u64 * y.clone() + Expr::make_const(13),
            (!x.clone() & y.clone()) | (x.clone() & !y.clone()),
        ];
        for width in [32, 64] {
            let mask = make_mask(width);
            let mut accepted = 0;
            for expression in &expressions {
                let analysis = analyze(expression.clone(), width, P9Limits::default());
                if !analysis.changed {
                    continue;
                }
                accepted += 1;
                let mut state = 0x9e37_79b9_7f4a_7c15u64;
                for _ in 0..256 {
                    let values = (0..2)
                        .map(|_| {
                            state ^= state << 7;
                            state ^= state >> 9;
                            state ^= state << 8;
                            state & mask
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        expression.eval(&values).get(mask),
                        analysis.result.eval(&values).get(mask)
                    );
                }
            }
            assert!(accepted > 0);
        }
    }

    #[test]
    fn bitwise_generator_converts_mask_outputs_to_boolean_coefficients() {
        let x = variable(0);
        let y = variable(1);
        let expressions = [
            x.clone() & y.clone(),
            x.clone() | y.clone(),
            x.clone() ^ y.clone(),
            !x.clone(),
        ];
        for width in (1..=6).chain([32, 64]) {
            for expression in &expressions {
                let candidate = generate_bitwise_candidate(expression, width).unwrap();
                let certification =
                    certify_equivalent(expression, &candidate, width, P9Limits::default());
                assert!(
                    matches!(certification, Certification::ProvedEquivalent(_)),
                    "width={width} expression={expression} candidate={candidate} certification={certification:?}"
                );
                if width <= 6 {
                    assert_exhaustively_equal(expression, &candidate, width, 2);
                }
            }
        }
    }

    #[test]
    fn linear_projection_is_canonical_and_covers_mixed_linear_mbas() {
        let x = variable(0);
        let y = variable(1);
        let expression = 3u64 * x.clone()
            + VarInt::from(u64::MAX - 4) * y.clone()
            + 7u64 * (x.clone() & y.clone());
        for width in (1..=6).chain([32, 64]) {
            let projection = generate_linear_projection(&expression, width);
            assert_eq!(
                generate_linear_projection(&projection, width),
                projection
            );
            assert!(matches!(
                certify_equivalent(&expression, &projection, width, P9Limits::default()),
                Certification::ProvedEquivalent(_)
            ));
            if width <= 6 {
                assert_exhaustively_equal(&expression, &projection, width, 2);
            }
        }
    }

    #[test]
    fn linear_analysis_separates_counterexample_from_unsupported() {
        let x = variable(0);
        let y = variable(1);
        let nonlinear_without_mul = (x.clone() + y.clone()) & Expr::make_const(1);
        let rejected = analyze_linear(
            nonlinear_without_mul.clone(),
            4,
            P9Limits::default(),
        );
        assert!(rejected.projection.is_some());
        assert!(matches!(
            rejected.certification,
            Certification::NotEquivalent(_)
        ));
        assert_eq!(rejected.result, nonlinear_without_mul);

        let word_product = Expr::Mul(vec![x, y]);
        let unsupported = analyze_linear(word_product.clone(), 64, P9Limits::default());
        assert!(unsupported.projection.is_none());
        assert_eq!(unsupported.certification, Certification::Unsupported);
        assert_eq!(unsupported.result, word_product);
    }

    #[test]
    fn unsupported_and_unknown_leave_the_input_unchanged() {
        let x = variable(0);
        let y = variable(1);
        let unsupported = Expr::Mul(vec![x.clone(), y.clone()]);
        let unsupported_result = analyze(unsupported.clone(), 64, P9Limits::default());
        assert_eq!(unsupported_result.result, unsupported);
        assert_eq!(unsupported_result.failure, Some(P9Failure::Unsupported));

        let common = x.clone() & y.clone();
        let expression = Expr::Add(vec![x, y, common.clone(), -common]);
        let limited = analyze(
            expression.clone(),
            64,
            P9Limits {
                max_reachable_states: 1,
                ..P9Limits::default()
            },
        );
        assert_eq!(limited.result, expression);
        assert_eq!(limited.failure, Some(P9Failure::BudgetExceeded));
    }

    #[test]
    fn scalar_mul_normalization_preserves_general_mul_as_unsupported() {
        let x = variable(0);
        let y = variable(1);
        let scalar_cases = [
            (
                Expr::Mul(vec![Expr::Const(VarInt::MAX), x.clone()]),
                VarInt::MAX * x.clone(),
            ),
            (
                Expr::Mul(vec![x.clone(), Expr::make_const(2)]),
                2u64 * x.clone(),
            ),
            (
                Expr::Mul(vec![
                    Expr::Const(VarInt::from(u64::MAX - 2)),
                    x.clone() & y.clone(),
                ]),
                VarInt::from(u64::MAX - 2) * (x.clone() & y.clone()),
            ),
            (
                Expr::Mul(vec![Expr::make_const(3), 5u64 * x.clone()]),
                15u64 * x.clone(),
            ),
        ];
        for width in (1..=6).chain([32, 64]) {
            for (serialized, scale) in &scalar_cases {
                assert_eq!(
                    normalize_p9_expression(serialized.clone(), width),
                    normalize_p9_expression(scale.clone(), width)
                );
                assert!(matches!(
                    certify_equivalent(serialized, scale, width, P9Limits::default()),
                    Certification::ProvedEquivalent(_)
                ));
            }
        }
        for (serialized, _) in &scalar_cases {
            let analysis = analyze(serialized.clone(), 64, P9Limits::default());
            assert_ne!(analysis.failure, Some(P9Failure::Unsupported));
        }

        let wrapped_product = Expr::Mul(vec![Expr::make_const(7), x.clone(), y.clone()]);
        assert!(matches!(
            certify_equivalent(&wrapped_product, &Expr::zero(), 64, P9Limits::default()),
            Certification::Unsupported
        ));
        let analysis = analyze(wrapped_product.clone(), 64, P9Limits::default());
        assert_eq!(analysis.result, wrapped_product);
        assert_eq!(analysis.failure, Some(P9Failure::Unsupported));
    }

    #[cfg(feature = "parse")]
    #[test]
    fn display_roundtrip_preserves_p9_support_after_boundary_normalization() {
        use crate::parser::parse_expr;

        let x = variable(0);
        let y = variable(1);
        let supported = [
            VarInt::MAX * x.clone(),
            2u64 * x.clone(),
            VarInt::from(u64::MAX - 2) * (x.clone() & y.clone()),
            x.clone() + 2u64 * y.clone(),
            !x.clone(),
            x.clone() & y.clone(),
            x.clone() | y.clone(),
            x.clone() ^ y.clone(),
            Expr::Const(VarInt::MAX),
        ];
        for expression in supported {
            let reparsed = parse_expr(&expression.to_string()).unwrap();
            let normalized = normalize_p9_expression(reparsed, 64);
            assert!(fragment_supported(&normalized));
            assert!(matches!(
                certify_equivalent(&expression, &normalized, 64, P9Limits::default()),
                Certification::ProvedEquivalent(_)
            ));
        }
    }
}

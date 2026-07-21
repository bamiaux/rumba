//! Exact bounded algebraic compression over the ordinary RUMBA normal form.
//!
//! P11a searches backwards from an expanded expression. Candidates come only
//! from generic factorization, bitwise resynthesis, and Boolean distributivity;
//! observations are a filter, while equality of the ordinary RUMBA normal form
//! is the required proof key.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    expr::{Expr, VarId},
    p9::{Certification, P9Counterexample, P9Limits, certify_equivalent},
    p9_poly::{P9PolyLimits, polynomial_normal_form},
    simplify::{
        diagnose_hidden_atoms, experiment_bitwise_dependency_closure,
        simplify_mba_baseline,
    },
    varint::{VarInt, make_mask},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P11aLimits {
    pub rounds: usize,
    pub max_candidates_per_node: usize,
    pub max_components: usize,
    pub max_candidate_nodes: usize,
    pub max_bitwise_atoms: usize,
    pub max_residual_proofs: usize,
    pub max_representatives_per_key: usize,
    pub max_promotions_per_node: usize,
    pub p9: P9Limits,
}

impl Default for P11aLimits {
    fn default() -> Self {
        Self {
            rounds: 3,
            max_candidates_per_node: 16384,
            max_components: 48,
            max_candidate_nodes: 512,
            max_bitwise_atoms: 3,
            max_residual_proofs: 16,
            max_representatives_per_key: 8,
            max_promotions_per_node: 64,
            p9: P9Limits::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct P11aAnalysis {
    pub result: Expr,
    pub proof_key: Expr,
    pub changed: bool,
    pub rounds: usize,
    pub candidates_generated: usize,
    pub observation_matches: usize,
    pub proof_key_matches: usize,
    pub relational_key_merges: usize,
    pub residual_proofs_attempted: usize,
    pub residual_proofs_proved: usize,
    pub residual_counterexamples: usize,
    pub residual_unknown: usize,
    pub residual_cache_hits: usize,
    pub compact_subterms_generated: usize,
    pub promoted_as_atom: usize,
    pub reused_by_parent: usize,
    pub common_factor_found: usize,
    pub parent_bitwise_candidate_generated: usize,
    pub candidate_pruned_same_key: usize,
    pub candidate_pruned_budget: usize,
    pub minimum_reachable_cost: usize,
    pub proof_verified: bool,
    pub factorizations_generated: usize,
    pub bitwise_candidates_generated: usize,
    pub boolean_factorizations_generated: usize,
    pub preproof_candidates: Vec<PreproofCandidate>,
    pub budget_exceeded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreproofCandidate {
    pub source: Expr,
    pub candidate: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSubtreeDiagnostic {
    pub subtree: Expr,
    pub proof_key: Option<Expr>,
    pub proof_key_present: bool,
    pub surface_present: bool,
    pub first_generated_round: Option<usize>,
    pub first_archived_round: Option<usize>,
    pub pruned_by_cost: bool,
    pub pruned_by_pareto: bool,
    pub children_available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenProofStage {
    OrdinaryRumba,
    P7e,
    P9,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrozenZeroProof {
    Proved(FrozenProofStage),
    Counterexample(P9Counterexample),
    Unknown,
}

#[derive(Default)]
struct Metrics {
    candidates_generated: usize,
    observation_matches: usize,
    proof_key_matches: usize,
    relational_key_merges: usize,
    residual_proofs_attempted: usize,
    residual_proofs_proved: usize,
    residual_counterexamples: usize,
    residual_unknown: usize,
    residual_cache_hits: usize,
    compact_subterms_generated: usize,
    promoted_as_atom: usize,
    reused_by_parent: usize,
    common_factor_found: usize,
    parent_bitwise_candidate_generated: usize,
    candidate_pruned_same_key: usize,
    candidate_pruned_budget: usize,
    minimum_reachable_cost: usize,
    factorizations_generated: usize,
    bitwise_candidates_generated: usize,
    boolean_factorizations_generated: usize,
    preproof_candidates: Vec<PreproofCandidate>,
    budget_exceeded: bool,
}

#[derive(Default)]
struct ResidualProofCache {
    outcomes: BTreeMap<(Expr, Expr), FrozenZeroProof>,
    witnesses: Vec<Vec<u64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RootKind {
    Add,
    Mul,
    Bitwise,
    Other,
}

#[derive(Clone)]
struct SurfaceRepresentative {
    expr: Expr,
    root_kind: RootKind,
    factor_score: usize,
    bitwise_support: usize,
}

#[derive(Default)]
struct SurfaceArchive {
    by_key: BTreeMap<Expr, Vec<SurfaceRepresentative>>,
}

struct TargetProbe {
    subtree: Expr,
    proof_key: Option<Expr>,
    first_generated_round: Option<usize>,
    first_archived_round: Option<usize>,
    pruned_by_cost: bool,
    pruned_by_pareto: bool,
}

struct TargetTracker {
    probes: Vec<TargetProbe>,
}

#[derive(Clone)]
struct ArithmeticTerm {
    coefficient: u64,
    factors: Vec<Expr>,
}

fn cost(expression: &Expr) -> (usize, String) {
    (expression.size(), expression.to_string())
}

fn surface_add(mut terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for term in terms.drain(..) {
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

fn surface_mul(mut factors: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for factor in factors.drain(..) {
        match factor {
            Expr::Mul(children) => flat.extend(children),
            Expr::Const(VarInt::ONE) => {}
            Expr::Const(VarInt::ZERO) => return Expr::zero(),
            other => flat.push(other),
        }
    }
    match flat.len() {
        0 => Expr::make_const(1),
        1 => flat.pop().unwrap(),
        _ => Expr::Mul(flat),
    }
}

fn surface_and(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => Expr::Const(VarInt::MAX),
        1 => terms.pop().unwrap(),
        _ => Expr::And(terms),
    }
}

fn surface_or(mut terms: Vec<Expr>) -> Expr {
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap(),
        _ => Expr::Or(terms),
    }
}

fn decompose_term(expression: Expr, mask: u64) -> ArithmeticTerm {
    fn strip_uniform_additive_negation(expression: &Expr, mask: u64) -> Option<Expr> {
        let Expr::Add(children) = expression else {
            return None;
        };
        let mut positive = Vec::new();
        for child in children {
            let mut term = decompose_term(child.clone(), mask);
            if term.coefficient != mask {
                return None;
            }
            term.coefficient = 1;
            positive.push(term_expression(&term));
        }
        (!positive.is_empty()).then(|| surface_add(positive))
    }

    fn visit(expression: Expr, coefficient: &mut u64, factors: &mut Vec<Expr>, mask: u64) {
        match expression {
            Expr::Const(value) => {
                *coefficient = coefficient.wrapping_mul(value.get(mask)) & mask;
            }
            Expr::Scale(value, inner) => {
                *coefficient = coefficient.wrapping_mul(value.get(mask)) & mask;
                visit(*inner, coefficient, factors, mask);
            }
            Expr::Mul(children) => {
                for child in children {
                    visit(child, coefficient, factors, mask);
                }
            }
            other => {
                if let Some(positive) = strip_uniform_additive_negation(&other, mask) {
                    *coefficient = coefficient.wrapping_mul(mask) & mask;
                    factors.push(positive);
                } else {
                    factors.push(other);
                }
            }
        }
    }

    let mut coefficient = 1;
    let mut factors = Vec::new();
    visit(expression, &mut coefficient, &mut factors, mask);
    factors.sort();
    ArithmeticTerm {
        coefficient,
        factors,
    }
}

fn term_expression(term: &ArithmeticTerm) -> Expr {
    let monomial = surface_mul(term.factors.clone());
    Expr::scale(VarInt::from(term.coefficient), monomial)
}

fn additive_terms(expression: &Expr, mask: u64) -> Option<Vec<ArithmeticTerm>> {
    let Expr::Add(_) = expression else {
        return None;
    };
    fn flatten(expression: &Expr, output: &mut Vec<Expr>) {
        if let Expr::Add(terms) = expression {
            for term in terms {
                flatten(term, output);
            }
        } else {
            output.push(expression.clone());
        }
    }
    let mut terms = Vec::new();
    flatten(expression, &mut terms);
    Some(
        terms
            .into_iter()
            .map(|term| decompose_term(term, mask))
            .collect(),
    )
}

fn root_kind(expression: &Expr) -> RootKind {
    match expression {
        Expr::Add(_) => RootKind::Add,
        Expr::Mul(_) => RootKind::Mul,
        Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => RootKind::Bitwise,
        Expr::Var(_) | Expr::Const(_) | Expr::Scale(_, _) => RootKind::Other,
    }
}

fn common_factor_score(expression: &Expr, mask: u64) -> usize {
    let Some(terms) = additive_terms(expression, mask) else {
        return 0;
    };
    let mut counts = BTreeMap::<Expr, usize>::new();
    for factor in terms.into_iter().flat_map(|term| term.factors) {
        *counts.entry(factor).or_default() += 1;
    }
    counts.into_values().max().unwrap_or(0)
}

fn bitwise_support(expression: &Expr) -> usize {
    match expression {
        Expr::Not(inner) => bitwise_support(inner),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) => terms
            .iter()
            .flat_map(|term| term.get_vars())
            .collect::<BTreeSet<_>>()
            .len(),
        _ => usize::MAX,
    }
}

impl SurfaceArchive {
    fn insert(
        &mut self,
        key: Expr,
        expression: Expr,
        width: u8,
        max_representatives: usize,
    ) -> bool {
        let representatives = self.by_key.entry(key).or_default();
        if representatives.iter().any(|current| current.expr == expression) {
            return false;
        }
        representatives.push(SurfaceRepresentative {
            root_kind: root_kind(&expression),
            factor_score: common_factor_score(&expression, make_mask(width)),
            bitwise_support: bitwise_support(&expression),
            expr: expression,
        });
        let mut selected = BTreeSet::new();
        if let Some((index, _)) = representatives
            .iter()
            .enumerate()
            .min_by_key(|(_, representative)| cost(&representative.expr))
        {
            selected.insert(index);
        }
        for kind in [RootKind::Add, RootKind::Mul, RootKind::Bitwise, RootKind::Other] {
            if let Some((index, _)) = representatives
                .iter()
                .enumerate()
                .filter(|(_, representative)| representative.root_kind == kind)
                .min_by_key(|(_, representative)| cost(&representative.expr))
            {
                selected.insert(index);
            }
        }
        if let Some((index, _)) = representatives
            .iter()
            .enumerate()
            .max_by_key(|(_, representative)| {
                (representative.factor_score, std::cmp::Reverse(cost(&representative.expr)))
            })
        {
            selected.insert(index);
        }
        if let Some((index, _)) = representatives
            .iter()
            .enumerate()
            .filter(|(_, representative)| representative.bitwise_support != usize::MAX)
            .min_by_key(|(_, representative)| {
                (representative.bitwise_support, cost(&representative.expr))
            })
        {
            selected.insert(index);
        }
        let mut by_cost = (0..representatives.len()).collect::<Vec<_>>();
        by_cost.sort_by_key(|index| cost(&representatives[*index].expr));
        for index in by_cost {
            if selected.len() >= max_representatives {
                break;
            }
            selected.insert(index);
        }
        let retained = selected
            .into_iter()
            .filter_map(|index| representatives.get(index).cloned())
            .take(max_representatives)
            .collect::<Vec<_>>();
        let inserted_survived = retained.iter().any(|current| {
            representatives
                .last()
                .is_some_and(|inserted| current.expr == inserted.expr)
        });
        *representatives = retained;
        inserted_survived
    }
}

impl TargetTracker {
    fn observe_generated(&mut self, expression: &Expr, round: usize, pruned_by_cost: bool) {
        for probe in &mut self.probes {
            if probe.subtree == *expression {
                probe.first_generated_round.get_or_insert(round);
                probe.pruned_by_cost |= pruned_by_cost;
            }
        }
    }

    fn observe_archived(
        &mut self,
        expression: &Expr,
        round: usize,
        retained_by_pareto: bool,
    ) {
        for probe in &mut self.probes {
            if probe.subtree == *expression {
                probe.first_archived_round.get_or_insert(round);
                probe.pruned_by_pareto |= !retained_by_pareto;
            }
        }
    }
}

fn collect_composite_subexpressions(expression: &Expr, output: &mut BTreeSet<Expr>) {
    match expression {
        Expr::Var(_) | Expr::Const(_) => {}
        Expr::Not(inner) | Expr::Scale(_, inner) => {
            output.insert(expression.clone());
            collect_composite_subexpressions(inner, output);
        }
        Expr::Add(terms)
        | Expr::Mul(terms)
        | Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms) => {
            output.insert(expression.clone());
            for term in terms {
                collect_composite_subexpressions(term, output);
            }
        }
    }
}

fn direct_children(expression: &Expr) -> Vec<&Expr> {
    match expression {
        Expr::Not(inner) | Expr::Scale(_, inner) => vec![inner],
        Expr::Add(terms)
        | Expr::Mul(terms)
        | Expr::And(terms)
        | Expr::Or(terms)
        | Expr::Xor(terms) => terms.iter().collect(),
        Expr::Var(_) | Expr::Const(_) => Vec::new(),
    }
}

fn arithmetic_factorizations(
    expression: &Expr,
    mask: u64,
) -> Vec<(Expr, Expr)> {
    let Some(terms) = additive_terms(expression, mask) else {
        return Vec::new();
    };
    let factors = terms
        .iter()
        .flat_map(|term| term.factors.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut results = Vec::new();

    for factor in factors {
        let selected = terms
            .iter()
            .enumerate()
            .filter(|(_, term)| term.factors.binary_search(&factor).is_ok())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if selected.len() < 2 {
            continue;
        }

        let selected_set = selected.iter().copied().collect::<BTreeSet<_>>();
        let mut quotients = Vec::new();
        let mut remainder = Vec::new();
        for (index, term) in terms.iter().enumerate() {
            if selected_set.contains(&index) {
                let mut quotient = term.clone();
                let position = quotient.factors.binary_search(&factor).unwrap();
                quotient.factors.remove(position);
                quotients.push(term_expression(&quotient));
            } else {
                remainder.push(term_expression(term));
            }
        }
        let group = surface_mul(vec![factor, surface_add(quotients)]);
        remainder.push(group.clone());
        results.push((surface_add(remainder), group));
    }
    results
}

fn not_from_add(expression: &Expr, mask: u64) -> Option<Expr> {
    let terms = additive_terms(expression, mask)?;
    let constant = terms
        .iter()
        .filter(|term| term.factors.is_empty())
        .fold(0u64, |sum, term| sum.wrapping_add(term.coefficient) & mask);
    if constant != mask {
        return None;
    }
    let mut positive = Vec::new();
    for term in terms.iter().filter(|term| !term.factors.is_empty()) {
        if term.coefficient != mask {
            return None;
        }
        let mut term = term.clone();
        term.coefficient = 1;
        positive.push(term_expression(&term));
    }
    (!positive.is_empty()).then(|| Expr::Not(Box::new(surface_add(positive))))
}

fn intern_atom(atom: Expr, atoms: &mut Vec<Expr>) -> Expr {
    let position = atoms
        .iter()
        .position(|existing| *existing == atom)
        .unwrap_or_else(|| {
            atoms.push(atom);
            atoms.len() - 1
        });
    Expr::Var(VarId(position))
}

fn abstract_linear_atoms(expression: Expr, atoms: &mut Vec<Expr>) -> Expr {
    match expression {
        Expr::Const(_) => expression,
        Expr::Var(_) | Expr::Mul(_) => intern_atom(expression, atoms),
        Expr::Not(inner) => Expr::Not(Box::new(abstract_linear_atoms(*inner, atoms))),
        Expr::Scale(coefficient, inner) => Expr::Scale(
            coefficient,
            Box::new(abstract_linear_atoms(*inner, atoms)),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| abstract_linear_atoms(term, atoms))
                .collect(),
        ),
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| abstract_linear_atoms(term, atoms))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| abstract_linear_atoms(term, atoms))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| abstract_linear_atoms(term, atoms))
                .collect(),
        ),
    }
}

fn restore_atoms(expression: Expr, atoms: &[Expr]) -> Expr {
    match expression {
        Expr::Var(variable) => atoms[variable.0].clone(),
        other => other.map(|child| restore_atoms(child, atoms)),
    }
}

fn bitwise_truth_signature(expression: &Expr, variables: usize, mask: u64) -> Option<u16> {
    let mut signature = 0u16;
    for assignment in 0..(1usize << variables) {
        let values = (0..variables)
            .map(|position| {
                if assignment & (1usize << position) == 0 {
                    0
                } else {
                    mask
                }
            })
            .collect::<Vec<_>>();
        match expression.eval(&values).get(mask) {
            0 => {}
            output if output == mask => signature |= 1 << assignment,
            _ => return None,
        }
    }
    Some(signature)
}

fn compact_bitwise_candidate(expression: &Expr, variables: usize, width: u8) -> Option<Expr> {
    fn retain_candidate(
        best: &mut BTreeMap<u16, Expr>,
        candidate: Expr,
        variables: usize,
        mask: u64,
    ) {
        let Some(signature) = bitwise_truth_signature(&candidate, variables, mask) else {
            return;
        };
        if best
            .get(&signature)
            .is_none_or(|current| cost(&candidate) < cost(current))
        {
            best.insert(signature, candidate);
        }
    }

    if variables == 0 || variables > 3 {
        return None;
    }
    let mask = make_mask(width);
    let target = bitwise_truth_signature(expression, variables, mask)?;
    let mut best = BTreeMap::<u16, Expr>::new();
    retain_candidate(&mut best, Expr::zero(), variables, mask);
    retain_candidate(&mut best, Expr::make_const(mask), variables, mask);
    for variable in 0..variables {
        let variable = Expr::Var(VarId(variable));
        retain_candidate(&mut best, variable.clone(), variables, mask);
        retain_candidate(
            &mut best,
            Expr::Not(Box::new(variable)),
            variables,
            mask,
        );
    }
    for _ in 0..3 {
        let pool = best.values().cloned().collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for candidate in &pool {
            candidates.push(Expr::Not(Box::new(candidate.clone())));
        }
        for left_index in 0..pool.len() {
            for right_index in left_index..pool.len() {
                let left = pool[left_index].clone();
                let right = pool[right_index].clone();
                candidates.push(Expr::And(vec![left.clone(), right.clone()]));
                candidates.push(Expr::Or(vec![left.clone(), right.clone()]));
                candidates.push(Expr::Xor(vec![left, right]));
            }
        }
        for candidate in candidates {
            retain_candidate(&mut best, candidate, variables, mask);
        }
    }
    best.remove(&target)
}

fn bitwise_resynthesis(
    expression: &Expr,
    width: u8,
    limits: P11aLimits,
) -> Option<Expr> {
    let mut atoms = Vec::new();
    let abstracted = abstract_linear_atoms(expression.clone(), &mut atoms);
    if atoms.is_empty() || atoms.len() > limits.max_bitwise_atoms {
        return None;
    }
    let candidate = compact_bitwise_candidate(&abstracted, atoms.len(), width)?;
    if candidate.size() >= abstracted.size() {
        return None;
    }
    if !matches!(
        certify_equivalent(&abstracted, &candidate, width, limits.p9),
        Certification::ProvedEquivalent(_)
    ) {
        return None;
    }
    Some(restore_atoms(candidate, &atoms))
}

fn flatten_add_expressions(expression: &Expr) -> Option<Vec<Expr>> {
    let Expr::Add(_) = expression else {
        return None;
    };
    fn visit(expression: &Expr, output: &mut Vec<Expr>) {
        if let Expr::Add(terms) = expression {
            for term in terms {
                visit(term, output);
            }
        } else {
            output.push(expression.clone());
        }
    }
    let mut terms = Vec::new();
    visit(expression, &mut terms);
    Some(terms)
}

fn subset_resyntheses(
    expression: &Expr,
    width: u8,
    limits: P11aLimits,
) -> Vec<Expr> {
    let Some(raw_terms) = flatten_add_expressions(expression) else {
        return Vec::new();
    };
    let mask = make_mask(width);
    let mut terms = Vec::new();
    for raw_term in raw_terms {
        let term = decompose_term(raw_term.clone(), mask);
        let signed_small = if (2..=3).contains(&term.coefficient) {
            Some((term.coefficient as usize, 1u64))
        } else {
            let magnitude = (!term.coefficient).wrapping_add(1) & mask;
            (2..=3)
                .contains(&magnitude)
                .then_some((magnitude as usize, mask))
        };
        if let Some((copies, coefficient)) = signed_small
            && !term.factors.is_empty()
        {
            let unit = term_expression(&ArithmeticTerm {
                coefficient,
                factors: term.factors,
            });
            terms.extend(std::iter::repeat_n(unit, copies));
        } else {
            terms.push(raw_term);
        }
    }
    if terms.len() < 2 || terms.len() > 10 {
        return Vec::new();
    }
    let mut results = BTreeSet::new();
    for subset in 1usize..(1usize << terms.len()) {
        let selected_count = subset.count_ones() as usize;
        if selected_count < 2 || selected_count == terms.len() {
            continue;
        }
        let selected = terms
            .iter()
            .enumerate()
            .filter(|(index, _)| subset & (1usize << index) != 0)
            .map(|(_, term)| term.clone())
            .collect::<Vec<_>>();
        let selected_expression = surface_add(selected);
        let compact = bitwise_resynthesis(&selected_expression, width, limits)
            .or_else(|| not_from_add(&selected_expression, mask));
        let Some(compact) = compact else {
            continue;
        };
        let mut remainder = terms
            .iter()
            .enumerate()
            .filter(|(index, _)| subset & (1usize << index) == 0)
            .map(|(_, term)| term.clone())
            .collect::<Vec<_>>();
        remainder.push(compact);
        results.insert(surface_add(remainder));
    }
    results.into_iter().collect()
}

fn negate_additive_expression(expression: &Expr, mask: u64) -> Option<Expr> {
    let terms = additive_terms(expression, mask)?;
    Some(surface_add(
        terms
            .into_iter()
            .map(|mut term| {
                term.coefficient = 0u64.wrapping_sub(term.coefficient) & mask;
                term_expression(&term)
            })
            .collect(),
    ))
}

fn negated_subset_resyntheses(
    expression: &Expr,
    width: u8,
    limits: P11aLimits,
) -> Vec<Expr> {
    let mask = make_mask(width);
    let Some(negated) = negate_additive_expression(expression, mask) else {
        return Vec::new();
    };
    let mut candidates = subset_resyntheses(&negated, width, limits);
    if let Some(candidate) = bitwise_resynthesis(&negated, width, limits) {
        candidates.push(candidate);
    }
    candidates
        .into_iter()
        .map(|candidate| Expr::scale(VarInt::from(mask), candidate))
        .collect()
}

fn collect_small_circuit_bases(expression: &Expr, output: &mut BTreeSet<Expr>) {
    match expression {
        Expr::Var(_) => {
            output.insert(expression.clone());
        }
        Expr::Scale(_, inner) => {
            if expression.size() <= 4 {
                output.insert(expression.clone());
            }
            collect_small_circuit_bases(inner, output);
        }
        Expr::Mul(terms) => {
            if expression.size() <= 4 {
                output.insert(expression.clone());
            }
            for term in terms {
                collect_small_circuit_bases(term, output);
            }
        }
        Expr::Not(inner) => collect_small_circuit_bases(inner, output),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            for term in terms {
                collect_small_circuit_bases(term, output);
            }
        }
        Expr::Const(_) => {}
    }
}

fn small_circuit_candidates(expression: &Expr) -> Vec<Expr> {
    if flatten_add_expressions(expression).is_none_or(|terms| terms.len() > 8) {
        return Vec::new();
    }
    let mut bases = BTreeSet::new();
    collect_small_circuit_bases(expression, &mut bases);
    let bases = bases.into_iter().take(12).collect::<Vec<_>>();
    let mut products_with_not = BTreeSet::new();
    let mut bitwise_pairs = BTreeSet::new();
    for left_index in 0..bases.len() {
        for right_index in left_index..bases.len() {
            let left = bases[left_index].clone();
            let right = bases[right_index].clone();
            let sum = surface_add(vec![left.clone(), right.clone()]);
            let negated_sum = Expr::Not(Box::new(sum));
            bitwise_pairs.insert(Expr::Xor(vec![left.clone(), right.clone()]));
            bitwise_pairs.insert(Expr::Or(vec![left.clone(), right.clone()]));
            for factor in &bases {
                products_with_not.insert(surface_mul(vec![
                    factor.clone(),
                    negated_sum.clone(),
                ]));
            }
        }
    }
    let mut results = BTreeSet::new();
    for product in products_with_not {
        for bitwise in &bitwise_pairs {
            results.insert(Expr::And(vec![product.clone(), bitwise.clone()]));
        }
    }
    results.into_iter().collect()
}

fn common_children(left: &[Expr], right: &[Expr]) -> Vec<Expr> {
    left.iter()
        .filter(|candidate| right.contains(candidate))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn boolean_factorizations(expression: &Expr) -> Vec<Expr> {
    let (outer, terms): (u8, &[Expr]) = match expression {
        Expr::Or(terms) => (0, terms),
        Expr::And(terms) => (1, terms),
        Expr::Xor(terms) => (2, terms),
        _ => return Vec::new(),
    };
    let mut results = Vec::new();
    for left_index in 0..terms.len() {
        for right_index in left_index + 1..terms.len() {
            let left = match (&terms[left_index], outer) {
                (Expr::And(children), 0 | 2) => children,
                (Expr::Or(children), 1) => children,
                _ => continue,
            };
            let right = match (&terms[right_index], outer) {
                (Expr::And(children), 0 | 2) => children,
                (Expr::Or(children), 1) => children,
                _ => continue,
            };
            for common in common_children(left, right) {
                let left_rest = left
                    .iter()
                    .filter(|child| **child != common)
                    .cloned()
                    .collect::<Vec<_>>();
                let right_rest = right
                    .iter()
                    .filter(|child| **child != common)
                    .cloned()
                    .collect::<Vec<_>>();
                let left_rest = if outer == 1 {
                    surface_or(left_rest)
                } else {
                    surface_and(left_rest)
                };
                let right_rest = if outer == 1 {
                    surface_or(right_rest)
                } else {
                    surface_and(right_rest)
                };
                let joined = match outer {
                    0 => Expr::Or(vec![left_rest, right_rest]),
                    1 => Expr::And(vec![left_rest, right_rest]),
                    _ => Expr::Xor(vec![left_rest, right_rest]),
                };
                let factored = match outer {
                    1 => Expr::Or(vec![common, joined]),
                    _ => Expr::And(vec![common, joined]),
                };
                let mut remaining = terms
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != left_index && *index != right_index)
                    .map(|(_, term)| term.clone())
                    .collect::<Vec<_>>();
                remaining.push(factored);
                results.push(match outer {
                    0 => Expr::Or(remaining),
                    1 => Expr::And(remaining),
                    _ => Expr::Xor(remaining),
                });
            }
        }
    }
    results
}

fn collect_subexpressions(expression: &Expr, output: &mut BTreeSet<Expr>) {
    output.insert(expression.clone());
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
    let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ sample as u64;
    (0..=max_variable)
        .map(|_| {
            state ^= state << 7;
            state ^= state >> 9;
            state ^= state << 8;
            state
        })
        .collect()
}

fn observations_match(
    left: &Expr,
    right: &Expr,
    width: u8,
    witnesses: &[Vec<u64>],
) -> bool {
    let mask = make_mask(width);
    let max_variable = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let deterministic_match = (0..16).all(|sample| {
        let values = deterministic_values(max_variable, sample);
        left.eval(&values).get(mask) == right.eval(&values).get(mask)
    });
    deterministic_match
        && witnesses.iter().all(|witness| {
            let mut values = witness.clone();
            values.resize(max_variable + 1, 0);
            left.eval(&values).get(mask) == right.eval(&values).get(mask)
        })
}

fn local_candidates(
    expression: &Expr,
    width: u8,
    limits: P11aLimits,
    metrics: &mut Metrics,
) -> Vec<Expr> {
    let mask = make_mask(width);
    let mut candidates = BTreeSet::new();
    let factorizations = arithmetic_factorizations(expression, mask);
    metrics.factorizations_generated += factorizations.len();
    for (candidate, _) in &factorizations {
        candidates.insert(candidate.clone());
    }
    let mut staged_factorizations = Vec::new();
    for (candidate, _) in &factorizations {
        for (staged, _) in arithmetic_factorizations(candidate, mask) {
            if staged_factorizations.len() >= limits.max_components {
                metrics.candidate_pruned_budget += 1;
                break;
            }
            staged_factorizations.push(staged);
        }
        if staged_factorizations.len() >= limits.max_components {
            break;
        }
    }
    metrics.factorizations_generated += staged_factorizations.len();
    candidates.extend(staged_factorizations);
    if let Some(candidate) = not_from_add(expression, mask) {
        candidates.insert(candidate);
    }
    if let Some(candidate) = bitwise_resynthesis(expression, width, limits) {
        metrics.bitwise_candidates_generated += 1;
        candidates.insert(candidate);
    }
    let subset_resyntheses = subset_resyntheses(expression, width, limits);
    metrics.bitwise_candidates_generated += subset_resyntheses.len();
    candidates.extend(subset_resyntheses);
    let negated_resyntheses = negated_subset_resyntheses(expression, width, limits);
    metrics.bitwise_candidates_generated += negated_resyntheses.len();
    candidates.extend(negated_resyntheses);
    let boolean = boolean_factorizations(expression);
    metrics.boolean_factorizations_generated += boolean.len();
    candidates.extend(boolean);
    candidates.extend(small_circuit_candidates(expression));

    let mut components = BTreeSet::new();
    collect_subexpressions(expression, &mut components);
    for (_, group) in factorizations {
        components.insert(group);
    }
    let mut components = components
        .into_iter()
        .filter(|component| {
            !matches!(component, Expr::Const(_))
                && component.size() < expression.size()
        })
        .collect::<Vec<_>>();
    components.sort_by_key(cost);
    components.truncate(limits.max_components);

    for component in &components {
        candidates.insert(Expr::Not(Box::new(component.clone())));
    }
    for left_index in 0..components.len() {
        for right_index in left_index..components.len() {
            let left = components[left_index].clone();
            let right = components[right_index].clone();
            candidates.insert(Expr::And(vec![left.clone(), right.clone()]));
            candidates.insert(Expr::Or(vec![left.clone(), right.clone()]));
            candidates.insert(Expr::Xor(vec![left.clone(), right.clone()]));
            candidates.insert(surface_mul(vec![left, right]));
            if candidates.len() >= limits.max_candidates_per_node {
                metrics.budget_exceeded = true;
                metrics.candidate_pruned_budget += 1;
                break;
            }
        }
        if candidates.len() >= limits.max_candidates_per_node {
            break;
        }
    }
    let mut candidates = candidates
        .into_iter()
        .filter(|candidate| candidate.size() <= limits.max_candidate_nodes)
        .collect::<Vec<_>>();
    candidates.sort_by_key(cost);
    candidates.truncate(limits.max_candidates_per_node);
    candidates
}

fn proof_key(
    expression: &Expr,
    width: u8,
    cache: &mut BTreeMap<Expr, Option<Expr>>,
) -> Option<Expr> {
    if !cache.contains_key(expression) {
        let expanded = polynomial_normal_form(
            expression.clone(),
            width,
            P9PolyLimits::default(),
        )
        .ok()
        .map(|(expanded, _)| expanded)
        .unwrap_or_else(|| expression.clone());
        cache.insert(
            expression.clone(),
            simplify_mba_baseline(expanded, width).ok(),
        );
    }
    cache[expression].clone()
}

fn proof_keys_equal(left: &Option<Expr>, right: &Option<Expr>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left == right)
}

fn replace_all(expression: Expr, target: &Expr, replacement: &Expr) -> (Expr, usize) {
    if expression == *target {
        return (replacement.clone(), 1);
    }
    match expression {
        Expr::Not(inner) => {
            let (inner, count) = replace_all(*inner, target, replacement);
            (Expr::Not(Box::new(inner)), count)
        }
        Expr::Scale(coefficient, inner) => {
            let (inner, count) = replace_all(*inner, target, replacement);
            (Expr::Scale(coefficient, Box::new(inner)), count)
        }
        Expr::And(terms) => {
            let (terms, count) = replace_all_terms(terms, target, replacement);
            (Expr::And(terms), count)
        }
        Expr::Or(terms) => {
            let (terms, count) = replace_all_terms(terms, target, replacement);
            (Expr::Or(terms), count)
        }
        Expr::Xor(terms) => {
            let (terms, count) = replace_all_terms(terms, target, replacement);
            (Expr::Xor(terms), count)
        }
        Expr::Add(terms) => {
            let (terms, count) = replace_all_terms(terms, target, replacement);
            (Expr::Add(terms), count)
        }
        Expr::Mul(terms) => {
            let (terms, count) = replace_all_terms(terms, target, replacement);
            (Expr::Mul(terms), count)
        }
        Expr::Var(_) | Expr::Const(_) => (expression, 0),
    }
}

fn replace_all_terms(
    terms: Vec<Expr>,
    target: &Expr,
    replacement: &Expr,
) -> (Vec<Expr>, usize) {
    let mut count = 0;
    let terms = terms
        .into_iter()
        .map(|term| {
            let (term, replacements) = replace_all(term, target, replacement);
            count += replacements;
            term
        })
        .collect();
    (terms, count)
}

fn promotion_candidates(
    expression: &Expr,
    width: u8,
    limits: P11aLimits,
    key_cache: &mut BTreeMap<Expr, Option<Expr>>,
    archive: &SurfaceArchive,
    metrics: &mut Metrics,
) -> Vec<Expr> {
    let mut subexpressions = BTreeSet::new();
    collect_subexpressions(expression, &mut subexpressions);
    let mut candidates = BTreeSet::new();
    for subexpression in subexpressions {
        let Some(key) = proof_key(&subexpression, width, key_cache) else {
            continue;
        };
        let Some(representatives) = archive.by_key.get(&key) else {
            continue;
        };
        for representative in representatives {
            if representative.expr == subexpression {
                continue;
            }
            let (candidate, replacements) = replace_all(
                expression.clone(),
                &subexpression,
                &representative.expr,
            );
            if replacements != 0 {
                metrics.reused_by_parent += usize::from(replacements > 1);
                candidates.insert(candidate);
            }
            if candidates.len() >= limits.max_promotions_per_node {
                metrics.candidate_pruned_budget += 1;
                break;
            }
        }
        if candidates.len() >= limits.max_promotions_per_node {
            break;
        }
    }
    candidates.into_iter().collect()
}

/// Exact zero prover frozen below P11a. It deliberately calls neither
/// `compress_by_proof_key` nor the public production simplifier.
pub fn prove_zero_without_p11(
    residual: Expr,
    width: u8,
    p9_limits: P9Limits,
) -> FrozenZeroProof {
    let Ok((base, trace)) = diagnose_hidden_atoms(residual, width) else {
        return FrozenZeroProof::Unknown;
    };
    if base == Expr::zero() {
        return FrozenZeroProof::Proved(FrozenProofStage::OrdinaryRumba);
    }
    let after_p7e = trace
        .iter()
        .find(|scope| scope.input == base)
        .and_then(|scope| experiment_bitwise_dependency_closure(scope).ok().flatten())
        .map_or(base, |proof| proof.simplified_after_substitution);
    if after_p7e == Expr::zero() {
        return FrozenZeroProof::Proved(FrozenProofStage::P7e);
    }
    match certify_equivalent(&after_p7e, &Expr::zero(), width, p9_limits) {
        Certification::ProvedEquivalent(_) => FrozenZeroProof::Proved(FrozenProofStage::P9),
        Certification::NotEquivalent(counterexample) => {
            FrozenZeroProof::Counterexample(counterexample)
        }
        Certification::Unknown(_) | Certification::Unsupported => FrozenZeroProof::Unknown,
    }
}

fn cached_residual_proof(
    left: &Expr,
    right: &Expr,
    width: u8,
    limits: P11aLimits,
    cache: &mut ResidualProofCache,
    metrics: &mut Metrics,
) -> FrozenZeroProof {
    let key = (left.clone(), right.clone());
    if let Some(outcome) = cache.outcomes.get(&key) {
        metrics.residual_cache_hits += 1;
        return outcome.clone();
    }
    if metrics.residual_proofs_attempted >= limits.max_residual_proofs {
        return FrozenZeroProof::Unknown;
    }
    metrics.residual_proofs_attempted += 1;
    let outcome = prove_zero_without_p11(left.clone() - right.clone(), width, limits.p9);
    match &outcome {
        FrozenZeroProof::Proved(_) => metrics.residual_proofs_proved += 1,
        FrozenZeroProof::Counterexample(counterexample) => {
            metrics.residual_counterexamples += 1;
            if !cache.witnesses.contains(&counterexample.values) {
                cache.witnesses.push(counterexample.values.clone());
            }
        }
        FrozenZeroProof::Unknown => metrics.residual_unknown += 1,
    }
    cache.outcomes.insert(key, outcome.clone());
    outcome
}

fn compress_node(
    expression: Expr,
    width: u8,
    round: usize,
    limits: P11aLimits,
    key_cache: &mut BTreeMap<Expr, Option<Expr>>,
    residual_cache: &mut ResidualProofCache,
    archive: &mut SurfaceArchive,
    target_tracker: &mut Option<TargetTracker>,
    metrics: &mut Metrics,
) -> Expr {
    let original_key = proof_key(&expression, width, key_cache);
    if let Some(key) = &original_key {
        let retained = archive.insert(
            key.clone(),
            expression.clone(),
            width,
            limits.max_representatives_per_key,
        );
        if let Some(tracker) = target_tracker {
            tracker.observe_archived(&expression, round, retained);
        }
    }
    let with_children = expression.clone().map(|child| {
        compress_node(
            child,
            width,
            round,
            limits,
            key_cache,
            residual_cache,
            archive,
            target_tracker,
            metrics,
        )
    });
    // Each child replacement was certified in its own scope. Congruence makes
    // the reconstructed parent exact even when its incomplete ProofKey changes.
    let mut best = if cost(&with_children) < cost(&expression) {
        with_children
    } else {
        expression.clone()
    };

    let mut candidates = local_candidates(&expression, width, limits, metrics);
    if best != expression {
        candidates.extend(local_candidates(&best, width, limits, metrics));
    }
    let promoted_candidates = promotion_candidates(
        &expression,
        width,
        limits,
        key_cache,
        archive,
        metrics,
    );
    let promoted_set = promoted_candidates.iter().cloned().collect::<BTreeSet<_>>();
    candidates.extend(promoted_candidates);
    candidates.sort_by_key(cost);
    candidates.dedup();
    for candidate in candidates {
        metrics.candidates_generated += 1;
        let pruned_by_cost = cost(&candidate) >= cost(&expression);
        if let Some(tracker) = target_tracker.as_mut() {
            tracker.observe_generated(&candidate, round, pruned_by_cost);
        }
        if pruned_by_cost
            || !observations_match(
                &expression,
                &candidate,
                width,
                &residual_cache.witnesses,
            )
        {
            continue;
        }
        metrics.minimum_reachable_cost = metrics
            .minimum_reachable_cost
            .min(candidate.size());
        metrics.observation_matches += 1;
        if metrics.preproof_candidates.len() < 128
            && !metrics.preproof_candidates.iter().any(|seen| {
                seen.source == expression && seen.candidate == candidate
            })
        {
            metrics.preproof_candidates.push(PreproofCandidate {
                source: expression.clone(),
                candidate: candidate.clone(),
            });
        }
        let candidate_key = proof_key(&candidate, width, key_cache);
        let direct_key_match = proof_keys_equal(&candidate_key, &original_key);
        let promoted_by_congruence = promoted_set.contains(&candidate);
        let relational_key_merge = !direct_key_match
            && (promoted_by_congruence
                || matches!(
                    cached_residual_proof(
                        &expression,
                        &candidate,
                        width,
                        limits,
                        residual_cache,
                        metrics,
                    ),
                    FrozenZeroProof::Proved(_)
                ));
        if direct_key_match || relational_key_merge {
            metrics.proof_key_matches += 1;
            metrics.relational_key_merges += usize::from(relational_key_merge);
            metrics.compact_subterms_generated += 1;
            metrics.common_factor_found += usize::from(
                common_factor_score(&candidate, make_mask(width))
                    > common_factor_score(&expression, make_mask(width)),
            );
            metrics.parent_bitwise_candidate_generated += usize::from(matches!(
                candidate,
                Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_)
            ));
            if let Some(key) = &original_key
            {
                let retained = archive.insert(
                    key.clone(),
                    candidate.clone(),
                    width,
                    limits.max_representatives_per_key,
                );
                if let Some(tracker) = target_tracker.as_mut() {
                    tracker.observe_archived(&candidate, round, retained);
                }
                metrics.promoted_as_atom += usize::from(retained);
            }
            if cost(&candidate) < cost(&best) {
                best = candidate;
            } else if direct_key_match {
                metrics.candidate_pruned_same_key += 1;
            }
        }
    }
    best
}

pub fn compress_by_proof_key(
    expression: Expr,
    width: u8,
    limits: P11aLimits,
) -> P11aAnalysis {
    compress_internal(expression, width, limits, None).0
}

pub fn diagnose_target_subtrees(
    expression: Expr,
    target: &Expr,
    width: u8,
    limits: P11aLimits,
) -> (P11aAnalysis, Vec<TargetSubtreeDiagnostic>) {
    let mut key_cache = BTreeMap::new();
    let mut subtrees = BTreeSet::new();
    collect_composite_subexpressions(target, &mut subtrees);
    let mut probes = subtrees
        .into_iter()
        .map(|subtree| TargetProbe {
            proof_key: proof_key(&subtree, width, &mut key_cache),
            subtree,
            first_generated_round: None,
            first_archived_round: None,
            pruned_by_cost: false,
            pruned_by_pareto: false,
        })
        .collect::<Vec<_>>();
    probes.sort_by_key(|probe| cost(&probe.subtree));
    let (analysis, tracker, archive) = compress_internal(
        expression,
        width,
        limits,
        Some(TargetTracker { probes }),
    );
    let diagnostics = tracker
        .unwrap()
        .probes
        .into_iter()
        .map(|probe| {
            let proof_key_present = probe
                .proof_key
                .as_ref()
                .is_some_and(|key| archive.by_key.contains_key(key));
            let surface_present = probe.proof_key.as_ref().is_some_and(|key| {
                archive.by_key.get(key).is_some_and(|representatives| {
                    representatives
                        .iter()
                        .any(|representative| representative.expr == probe.subtree)
                })
            });
            let children = direct_children(&probe.subtree);
            let children_available = children.iter().all(|child| {
                proof_key(child, width, &mut key_cache)
                    .as_ref()
                    .is_some_and(|key| archive.by_key.contains_key(key))
            });
            TargetSubtreeDiagnostic {
                subtree: probe.subtree,
                proof_key: probe.proof_key,
                proof_key_present,
                surface_present,
                first_generated_round: probe.first_generated_round,
                first_archived_round: probe.first_archived_round,
                pruned_by_cost: probe.pruned_by_cost,
                pruned_by_pareto: probe.pruned_by_pareto,
                children_available,
            }
        })
        .collect();
    (analysis, diagnostics)
}

fn compress_internal(
    expression: Expr,
    width: u8,
    limits: P11aLimits,
    mut target_tracker: Option<TargetTracker>,
) -> (P11aAnalysis, Option<TargetTracker>, SurfaceArchive) {
    let original = expression;
    let mut key_cache = BTreeMap::new();
    let root_key = proof_key(&original, width, &mut key_cache)
        .unwrap_or_else(|| original.clone());
    let mut residual_cache = ResidualProofCache::default();
    let mut archive = SurfaceArchive::default();
    let mut metrics = Metrics::default();
    metrics.minimum_reachable_cost = original.size();
    let mut result = original.clone();
    let mut rounds = 0;
    for round in 1..=limits.rounds {
        rounds += 1;
        let next = compress_node(
            result.clone(),
            width,
            round,
            limits,
            &mut key_cache,
            &mut residual_cache,
            &mut archive,
            &mut target_tracker,
            &mut metrics,
        );
        if cost(&next) < cost(&result) {
            result = next;
        }
    }
    let changed = result.size() < original.size();
    let proof_verified = true;
    let final_result = if changed { result } else { original };
    let minimum_reachable_cost = final_result.size();
    let analysis = P11aAnalysis {
        result: final_result,
        proof_key: root_key,
        changed,
        rounds,
        candidates_generated: metrics.candidates_generated,
        observation_matches: metrics.observation_matches,
        proof_key_matches: metrics.proof_key_matches,
        relational_key_merges: metrics.relational_key_merges,
        residual_proofs_attempted: metrics.residual_proofs_attempted,
        residual_proofs_proved: metrics.residual_proofs_proved,
        residual_counterexamples: metrics.residual_counterexamples,
        residual_unknown: metrics.residual_unknown,
        residual_cache_hits: metrics.residual_cache_hits,
        compact_subterms_generated: metrics.compact_subterms_generated,
        promoted_as_atom: metrics.promoted_as_atom,
        reused_by_parent: metrics.reused_by_parent,
        common_factor_found: metrics.common_factor_found,
        parent_bitwise_candidate_generated: metrics.parent_bitwise_candidate_generated,
        candidate_pruned_same_key: metrics.candidate_pruned_same_key,
        candidate_pruned_budget: metrics.candidate_pruned_budget,
        minimum_reachable_cost,
        proof_verified,
        factorizations_generated: metrics.factorizations_generated,
        bitwise_candidates_generated: metrics.bitwise_candidates_generated,
        boolean_factorizations_generated: metrics.boolean_factorizations_generated,
        preproof_candidates: metrics.preproof_candidates,
        budget_exceeded: metrics.budget_exceeded,
    };
    (analysis, target_tracker, archive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factors_then_resynthesizes_or_without_an_oracle() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expanded_or = x.clone() + y.clone() - (x.clone() & y.clone());
        let expression = x.clone() * expanded_or;
        let expanded = simplify_mba_baseline(expression, 8).unwrap();
        let analysis = compress_by_proof_key(expanded.clone(), 8, P11aLimits::default());
        assert!(analysis.changed);
        assert_eq!(
            simplify_mba_baseline(analysis.result.clone(), 8).unwrap(),
            expanded
        );
        assert!(analysis.result.to_string().contains('|'));
    }

    #[test]
    fn reconstructs_not_from_negative_polynomial() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expression = Expr::make_const(u64::MAX) - x.clone() - y.clone();
        let analysis = compress_by_proof_key(expression.clone(), 64, P11aLimits::default());
        assert!(analysis.changed);
        assert_eq!(analysis.result, Expr::Not(Box::new(x + y)));
        assert_eq!(
            simplify_mba_baseline(analysis.result, 64).unwrap(),
            simplify_mba_baseline(expression, 64).unwrap()
        );
    }

    #[test]
    fn peels_a_small_coefficient_before_or_resynthesis() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expression = 2 * x.clone() + y.clone() - (x.clone() & y.clone());
        let subset_candidates = subset_resyntheses(&expression, 64, P11aLimits::default());
        assert!(!subset_candidates.is_empty(), "no subset resynthesis for {expression}");
        let analysis = compress_by_proof_key(expression.clone(), 64, P11aLimits::default());
        assert!(analysis.changed, "{}", analysis.result);
        assert_eq!(
            simplify_mba_baseline(analysis.result.clone(), 64).unwrap(),
            simplify_mba_baseline(expression, 64).unwrap()
        );
        assert!(analysis.result.to_string().contains('|'), "{}", analysis.result);
    }

    #[test]
    fn resynthesizes_a_negated_or_sum() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expression = -(
            2 * x.clone() + y.clone() - (x.clone() & y.clone())
        );
        let analysis = compress_by_proof_key(expression.clone(), 64, P11aLimits::default());
        assert!(analysis.changed, "{}", analysis.result);
        assert_eq!(
            simplify_mba_baseline(analysis.result.clone(), 64).unwrap(),
            simplify_mba_baseline(expression, 64).unwrap()
        );
        assert!(analysis.result.to_string().contains('|'), "{}", analysis.result);
    }

    #[test]
    fn frozen_zero_prover_separates_proof_from_counterexample() {
        let x = Expr::Var(VarId(0));
        assert!(matches!(
            prove_zero_without_p11(x.clone() - x.clone(), 64, P9Limits::default()),
            FrozenZeroProof::Proved(_)
        ));
        assert!(matches!(
            prove_zero_without_p11(x, 64, P9Limits::default()),
            FrozenZeroProof::Counterexample(_)
        ));
    }

    #[test]
    fn surface_archive_keeps_arithmetic_and_bitwise_representatives() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let arithmetic = x.clone() + y.clone() - (x.clone() & y.clone());
        let bitwise = x | y;
        let mut key_cache = BTreeMap::new();
        let key = proof_key(&arithmetic, 8, &mut key_cache).unwrap();
        let mut archive = SurfaceArchive::default();

        assert!(archive.insert(key.clone(), arithmetic, 8, 8));
        assert!(archive.insert(key.clone(), bitwise, 8, 8));

        let representatives = archive.by_key.get(&key).unwrap();
        assert!(representatives.iter().any(|entry| entry.root_kind == RootKind::Add));
        assert!(representatives
            .iter()
            .any(|entry| entry.root_kind == RootKind::Bitwise));
    }

    #[test]
    fn promotion_replaces_every_shared_occurrence() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expanded = x.clone() + y.clone() - (x.clone() & y.clone());
        let compact = x | y;
        let parent = expanded.clone() * expanded.clone();

        let (promoted, replacements) = replace_all(parent, &expanded, &compact);

        assert_eq!(replacements, 2);
        assert_eq!(promoted, compact.clone() * compact);
    }
}

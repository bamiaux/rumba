use rumba_core::{
    expr::Expr,
    p11a::{
        FrozenZeroProof, P11aLimits, compress_by_proof_key,
        prove_zero_without_p11,
    },
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const LINES: [usize; 3] = [13, 53, 486];

fn p7e_input(expression: Expr) -> Expr {
    let Ok((base, trace)) = diagnose_hidden_atoms(expression.clone(), WIDTH) else {
        return expression;
    };
    let Some(scope) = trace.iter().find(|scope| scope.input == base) else {
        return base;
    };
    experiment_bitwise_dependency_closure(scope)
        .ok()
        .flatten()
        .map_or(base, |result| result.simplified_after_substitution)
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

fn sample_equal(left: &Expr, right: &Expr) -> bool {
    let max_variable = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    (0..256).all(|sample| {
        let values = deterministic_values(max_variable, sample);
        left.eval(&values).get(u64::MAX) == right.eval(&values).get(u64::MAX)
    })
}

fn main() {
    let selected_lines = std::env::args()
        .skip(1)
        .map(|argument| argument.parse::<usize>().unwrap())
        .collect::<Vec<_>>();
    let selected_lines = if selected_lines.is_empty() {
        LINES.to_vec()
    } else {
        selected_lines
    };
    let mut changed = 0;
    let mut expected_matches = 0;
    let mut proof_key_matches = 0;
    let mut budget_exceeded = 0;
    let mut equivalent_candidate_seen = 0;
    let mut target_residuals_proved = 0;
    let mut target_cost_reached = 0;

    for &line in &selected_lines {
        let row = QSYNTH.lines().nth(line - 1).unwrap();
        let (source_text, expected_text) = row.split_once(',').unwrap();
        let source = parse_expr(source_text.trim()).unwrap();
        let expected = parse_expr(expected_text.trim())
            .unwrap()
            .reduce(make_mask(WIDTH));
        let input = p7e_input(source);
        let mut limits = P11aLimits::default();
        if let Ok(rounds) = std::env::var("P11A_ROUNDS") {
            limits.rounds = rounds.parse().unwrap();
        }
        let analysis = compress_by_proof_key(input.clone(), WIDTH, limits);
        let expected_match = sample_equal(&analysis.result, &expected);
        let expected_key_match = diagnose_hidden_atoms(expected.clone(), WIDTH)
            .ok()
            .is_some_and(|(key, _)| key == analysis.proof_key);
        let key_preserved = analysis.proof_verified;
        let preproof_candidates = analysis
            .preproof_candidates
            .iter()
            .map(|entry| &entry.candidate)
            .chain(std::iter::once(&analysis.result))
            .collect::<Vec<_>>();
        let target_seen_before_key = preproof_candidates
            .iter()
            .any(|candidate| **candidate == expected);
        let target_fingerprint_match = sample_equal(&input, &expected);
        let target_residual_proof = prove_zero_without_p11(
            input.clone() - expected.clone(),
            WIDTH,
            limits.p9,
        );
        let target_residual_proved =
            matches!(target_residual_proof, FrozenZeroProof::Proved(_));
        let expected_or_equivalent_candidate_seen_before_proofkey_filter = preproof_candidates
            .iter()
            .filter(|candidate| candidate.size() <= expected.size())
            .any(|candidate| {
                matches!(
                    prove_zero_without_p11(
                        (*candidate).clone() - expected.clone(),
                        WIDTH,
                        limits.p9,
                    ),
                    FrozenZeroProof::Proved(_)
                )
            });
        let final_target_cost_reached = analysis.result.size() <= expected.size();

        changed += usize::from(analysis.changed);
        expected_matches += usize::from(expected_match);
        proof_key_matches += usize::from(key_preserved);
        budget_exceeded += usize::from(analysis.budget_exceeded);
        equivalent_candidate_seen += usize::from(
            expected_or_equivalent_candidate_seen_before_proofkey_filter,
        );
        target_residuals_proved += usize::from(target_residual_proved);
        target_cost_reached += usize::from(final_target_cost_reached);

        println!("qsynth_ea.csv:{line}");
        println!("input_nodes={}", input.size());
        println!("result_nodes={}", analysis.result.size());
        println!("expected_nodes={}", expected.size());
        println!("changed={}", analysis.changed);
        println!("expected_sample_match={expected_match}");
        println!("expected_proof_key_match={expected_key_match}");
        println!("target_seen_before_key={target_seen_before_key}");
        println!("target_cost={}", expected.size());
        println!("target_fingerprint_match={target_fingerprint_match}");
        println!("target_residual_proof={target_residual_proof:?}");
        println!("target_residual_proved={target_residual_proved}");
        println!(
            "expected_or_equivalent_candidate_seen_before_proofkey_filter={}",
            expected_or_equivalent_candidate_seen_before_proofkey_filter
        );
        println!("final_target_cost_reached={final_target_cost_reached}");
        println!("proof_key_preserved={key_preserved}");
        println!("rounds={}", analysis.rounds);
        println!("candidates_generated={}", analysis.candidates_generated);
        println!("observation_matches={}", analysis.observation_matches);
        println!("proof_key_matches={}", analysis.proof_key_matches);
        println!("relational_key_merges={}", analysis.relational_key_merges);
        println!(
            "residual_proofs=attempted:{},proved:{},counterexamples:{},unknown:{},cache_hits:{}",
            analysis.residual_proofs_attempted,
            analysis.residual_proofs_proved,
            analysis.residual_counterexamples,
            analysis.residual_unknown,
            analysis.residual_cache_hits,
        );
        println!("factorizations={}", analysis.factorizations_generated);
        println!("bitwise_resyntheses={}", analysis.bitwise_candidates_generated);
        println!(
            "boolean_factorizations={}",
            analysis.boolean_factorizations_generated
        );
        println!(
            "hierarchy=compact_subterms:{},promoted:{},reused_by_parent:{},common_factor_found:{},parent_bitwise_generated:{}",
            analysis.compact_subterms_generated,
            analysis.promoted_as_atom,
            analysis.reused_by_parent,
            analysis.common_factor_found,
            analysis.parent_bitwise_candidate_generated,
        );
        println!(
            "pruning=same_key:{},budget:{},minimum_reachable_cost:{}",
            analysis.candidate_pruned_same_key,
            analysis.candidate_pruned_budget,
            analysis.minimum_reachable_cost,
        );
        println!("budget_exceeded={}", analysis.budget_exceeded);
        println!("input={input}");
        println!("result={}", analysis.result);
        println!("expected={expected}");
        println!("---");
    }

    println!("sources={}", selected_lines.len());
    println!("changed={changed}");
    println!("expected_sample_matches={expected_matches}");
    println!("proof_keys_preserved={proof_key_matches}");
    println!("equivalent_candidate_seen={equivalent_candidate_seen}");
    println!("target_residuals_proved={target_residuals_proved}");
    println!("target_cost_reached={target_cost_reached}");
    println!("budget_exceeded={budget_exceeded}");
    println!("expected_used_for_candidate_generation=false");
}

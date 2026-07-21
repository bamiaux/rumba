use std::time::Duration;

use rumba_core::{
    expr::Expr,
    p9::{Certification, P9Limits, analyze_linear, certify_equivalent},
    p9_poly::{P9PolyLimits, analyze_poly},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure, simplify_mba},
};

const WIDTH: u8 = 64;
const DATASETS: [(&str, &str); 7] = [
    ("loki_tiny.csv", include_str!("../../third_party/dataset/loki_tiny.csv")),
    ("mba_flatten.csv", include_str!("../../third_party/dataset/mba_flatten.csv")),
    (
        "mba_obf_linear.csv",
        include_str!("../../third_party/dataset/mba_obf_linear.csv"),
    ),
    (
        "mba_obf_nonlinear.csv",
        include_str!("../../third_party/dataset/mba_obf_nonlinear.csv"),
    ),
    ("neureduce.csv", include_str!("../../third_party/dataset/neureduce.csv")),
    ("qsynth_ea.csv", include_str!("../../third_party/dataset/qsynth_ea.csv")),
    ("syntia.csv", include_str!("../../third_party/dataset/syntia.csv")),
];

fn residual_is_zero(expected: Expr, actual: Expr) -> bool {
    simplify_mba(expected - actual, WIDTH) == Ok(Expr::zero())
}

fn p7e_prepass(input: Expr) -> Expr {
    let Ok((diagnosed, trace)) = diagnose_hidden_atoms(input.clone(), WIDTH) else {
        return input;
    };
    let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed) else {
        return input;
    };
    experiment_bitwise_dependency_closure(scope)
        .ok()
        .flatten()
        .map_or(input, |result| result.simplified_after_substitution)
}

fn proof_states(expression: &Expr) -> usize {
    match certify_equivalent(expression, expression, WIDTH, P9Limits::default()) {
        Certification::ProvedEquivalent(proof) => proof.max_reachable_states,
        _ => 0,
    }
}

fn explicit_mul_degree(expression: &Expr) -> usize {
    match expression {
        Expr::Const(_) => 0,
        Expr::Var(_) => 1,
        Expr::Scale(_, inner) => explicit_mul_degree(inner),
        Expr::Add(terms) => terms.iter().map(explicit_mul_degree).max().unwrap_or(0),
        Expr::Mul(terms) => terms.iter().map(explicit_mul_degree).sum(),
        Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Xor(_) => 1,
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
    let mut sources = 0;
    let mut mul_cases = 0;
    let mut all_factors_linearized = 0;
    let mut factor_occurrences = 0;
    let mut factors_proved = 0;
    let mut factor_counterexamples = 0;
    let mut factor_unknown = 0;
    let mut resolved_by_sparse_collection = 0;
    let mut resolved_by_pct = 0;
    let mut final_resolved = 0;
    let mut changed_cases = 0;
    let mut pct_attempted_cases = 0;
    let mut nonlinear_resolved_by_poly = 0;
    let mut unknown = 0;
    let mut semantic_regressions = 0;
    let mut max_factor_states = 0;
    let mut factor_generation_time = Duration::ZERO;
    let mut factor_verification_time = Duration::ZERO;
    let mut sparse_collection_time = Duration::ZERO;
    let mut pct_time = Duration::ZERO;
    let mut mul_lines = Vec::new();
    let mut nonlinear_lines = Vec::new();

    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().enumerate() {
            if row.trim().is_empty() {
                continue;
            }
            let (source_text, expected_text) = row.split_once(',').unwrap();
            let (Ok(base), Ok(expected)) = (
                simplify_mba(parse_expr(source_text.trim()).unwrap(), WIDTH),
                simplify_mba(parse_expr(expected_text.trim()).unwrap(), WIDTH),
            ) else {
                continue;
            };
            if residual_is_zero(expected.clone(), base.clone()) {
                continue;
            }
            sources += 1;
            let input = p7e_prepass(base);
            if residual_is_zero(expected.clone(), input.clone()) {
                continue;
            }
            let linear = analyze_linear(input.clone(), WIDTH, P9Limits::default());
            let line = index + 1;
            match &linear.certification {
                Certification::Unsupported => {
                    mul_cases += 1;
                    let poly = analyze_poly(input.clone(), WIDTH, P9PolyLimits::default());
                    all_factors_linearized += usize::from(poly.all_factors_linearized);
                    factor_occurrences += poly.factor_occurrences;
                    factors_proved += poly.factors_proved;
                    factor_counterexamples += poly.factor_counterexamples;
                    factor_unknown += poly.factor_unknown;
                    unknown += usize::from(
                        poly.factor_unknown != 0
                            || poly.factor_unsupported != 0
                            || poly.sparse_budget_exceeded,
                    );
                    max_factor_states = max_factor_states.max(poly.max_factor_states);
                    factor_generation_time += poly.factor_generation_time;
                    factor_verification_time += poly.factor_verification_time;
                    sparse_collection_time += poly.sparse_collection_time;
                    pct_time += poly.pct_time;

                    let local_resolved = poly.local_result.as_ref().is_some_and(|candidate| {
                        residual_is_zero(expected.clone(), candidate.clone())
                    });
                    let pct_resolved = !local_resolved
                        && poly.pct_result.as_ref().is_some_and(|candidate| {
                            residual_is_zero(expected.clone(), candidate.clone())
                        });
                    let resolved = poly.changed
                        && residual_is_zero(expected.clone(), poly.result.clone());
                    resolved_by_sparse_collection += usize::from(local_resolved);
                    resolved_by_pct += usize::from(pct_resolved);
                    final_resolved += usize::from(resolved);
                    changed_cases += usize::from(poly.changed);
                    pct_attempted_cases += usize::from(poly.pct_attempted);

                    let mut exact = sample_equal(&input, &poly.result);
                    if let Some(local) = &poly.local_result {
                        exact &= sample_equal(&input, local);
                    }
                    if let Some(pct) = &poly.pct_result {
                        exact &= sample_equal(&input, pct);
                    }
                    semantic_regressions += usize::from(!exact);
                    mul_lines.push(format!(
                        "{dataset}:{line}:all_factors_linearized={}:factors={}/{}:\
factor_counterexamples={}:factor_unknown={}:monomials={}:local_resolved={}:\
pct_attempted={}:pct_resolved={}:final_resolved={}:nodes={}->{}",
                        poly.all_factors_linearized,
                        poly.factors_proved,
                        poly.factor_occurrences,
                        poly.factor_counterexamples,
                        poly.factor_unknown,
                        poly.sparse_monomials,
                        local_resolved,
                        poly.pct_attempted,
                        pct_resolved,
                        resolved,
                        input.size(),
                        poly.result.size(),
                    ));
                }
                Certification::NotEquivalent(counterexample) => {
                    let poly = analyze_poly(input.clone(), WIDTH, P9PolyLimits::default());
                    let poly_resolved = poly.changed
                        && residual_is_zero(expected.clone(), poly.result.clone());
                    nonlinear_resolved_by_poly += usize::from(poly_resolved);
                    semantic_regressions += usize::from(!sample_equal(&input, &poly.result));
                    nonlinear_lines.push(format!(
                        "{dataset}:{line}:variables={}:explicit_mul_degree={}:\
source_states={}:expected_states={}:counterexample_states={}:source_nodes={}:\
expected_nodes={}:poly_changed={}:poly_resolved={}:source={}:expected={}",
                        input.get_vars().len(),
                        explicit_mul_degree(&input),
                        proof_states(&input),
                        proof_states(&expected),
                        counterexample.max_reachable_states,
                        input.size(),
                        expected.size(),
                        poly.changed,
                        poly_resolved,
                        input,
                        expected,
                    ));
                }
                Certification::ProvedEquivalent(_) | Certification::Unknown(_) => {}
            }
        }
    }

    assert_eq!(sources, 101);
    assert_eq!(mul_cases, 12);
    assert_eq!(nonlinear_lines.len(), 4);
    println!("sources={sources}");
    println!("mul_cases={mul_cases}");
    println!("all_factors_linearized={all_factors_linearized}");
    println!("factor_occurrences={factor_occurrences}");
    println!("factors_proved={factors_proved}");
    println!("factor_counterexamples={factor_counterexamples}");
    println!("factor_unknown={factor_unknown}");
    println!("resolved_by_sparse_collection={resolved_by_sparse_collection}");
    println!("resolved_by_pct={resolved_by_pct}");
    println!("final_resolved={final_resolved}");
    println!("changed_cases={changed_cases}");
    println!("pct_attempted_cases={pct_attempted_cases}");
    println!("nonlinear_cases={}", nonlinear_lines.len());
    println!("nonlinear_resolved_by_poly={nonlinear_resolved_by_poly}");
    println!("unknown={unknown}");
    println!("semantic_regressions={semantic_regressions}");
    println!("max_factor_states={max_factor_states}");
    println!(
        "factor_generation_ms={:.3}",
        factor_generation_time.as_secs_f64() * 1_000.0
    );
    println!(
        "factor_verification_ms={:.3}",
        factor_verification_time.as_secs_f64() * 1_000.0
    );
    println!(
        "sparse_collection_ms={:.3}",
        sparse_collection_time.as_secs_f64() * 1_000.0
    );
    println!("pct_ms={:.3}", pct_time.as_secs_f64() * 1_000.0);
    println!("mul_lines=[{}]", mul_lines.join(" | "));
    println!("nonlinear_lines=[{}]", nonlinear_lines.join(" | "));
    println!("expected_used_for_candidate_generation=false");
    println!("general_mul_added_to_p9=false");
}

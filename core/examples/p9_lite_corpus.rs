use std::{collections::BTreeMap, time::Duration};

use rumba_core::{
    expr::Expr,
    p8::experiment_pipeline,
    p9::{Certification, P9Analysis, P9Failure, P9Limits, UnknownReason, analyze},
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

struct Case {
    dataset: &'static str,
    line: usize,
    mba: Expr,
    expected: Expr,
}

#[derive(Default)]
struct Report {
    ng_input: usize,
    ng_resolved: usize,
    prepass_resolved: usize,
    p9_inputs: usize,
    candidate_generated: usize,
    certification_attempted: usize,
    candidate_certified: usize,
    result_reduced: usize,
    certified_not_reduced: usize,
    bitwise_candidates: usize,
    affine_candidates: usize,
    candidate_not_found: usize,
    candidate_rejected: usize,
    unsupported_fragment: usize,
    state_budget_exceeded: usize,
    other_budget_exceeded: usize,
    result_still_nonzero: usize,
    prepass_time: Duration,
    candidate_generation: Duration,
    verification: Duration,
    states: Vec<usize>,
    nodes_before: Vec<usize>,
    nodes_after: Vec<usize>,
    resolved_lines: Vec<String>,
    failure_lines: Vec<String>,
    unsupported_details: Vec<String>,
    resolved_by_dataset: BTreeMap<&'static str, usize>,
}

fn load_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().enumerate() {
            if row.trim().is_empty() {
                continue;
            }
            let (mba, expected) = row
                .split_once(',')
                .unwrap_or_else(|| panic!("{dataset}:{}: expected two columns", index + 1));
            cases.push(Case {
                dataset,
                line: index + 1,
                mba: parse_expr(mba.trim()).unwrap(),
                expected: parse_expr(expected.trim()).unwrap(),
            });
        }
    }
    cases
}

fn residual_is_zero(expected: Expr, actual: Expr) -> bool {
    simplify_mba(expected - actual, WIDTH) == Ok(Expr::zero())
}

fn max_states(analysis: &P9Analysis) -> Option<usize> {
    analysis
        .attempts
        .iter()
        .filter_map(|attempt| match &attempt.certification {
            Certification::ProvedEquivalent(proof) => Some(proof.max_reachable_states),
            Certification::NotEquivalent(counterexample) => {
                Some(counterexample.max_reachable_states)
            }
            Certification::Unknown(unknown) => Some(unknown.max_reachable_states),
            Certification::Unsupported => None,
        })
        .max()
}

fn has_budget_reason(analysis: &P9Analysis, reason: UnknownReason) -> bool {
    analysis.attempts.iter().any(|attempt| {
        matches!(
            &attempt.certification,
            Certification::Unknown(unknown) if unknown.reason == reason
        )
    })
}

fn percentile(values: &[usize], percentile: usize) -> usize {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (percentile * sorted.len()).div_ceil(100).saturating_sub(1);
    sorted[rank]
}

fn collect_mul_details(expression: &Expr, output: &mut Vec<String>) {
    match expression {
        Expr::Mul(terms) => {
            let factors = terms
                .iter()
                .map(|term| {
                    format!(
                        "{} [constant={}]",
                        term,
                        matches!(term, Expr::Const(_))
                    )
                })
                .collect::<Vec<_>>()
                .join(" ; ");
            let nonconstant = terms
                .iter()
                .filter(|term| !matches!(term, Expr::Const(_)))
                .count();
            output.push(format!(
                "nonconstant_factors={nonconstant} factors=[{factors}]"
            ));
            for term in terms {
                collect_mul_details(term, output);
            }
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => collect_mul_details(inner, output),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            for term in terms {
                collect_mul_details(term, output);
            }
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn record_p9(report: &mut Report, case: &Case, input: Expr, expected: Expr) {
    report.p9_inputs += 1;
    let input_size = input.size();
    let analysis = analyze(input.clone(), WIDTH, P9Limits::default());
    report.candidate_generation += analysis.candidate_generation_time;
    report.verification += analysis.verification_time;
    report.bitwise_candidates += usize::from(analysis.bitwise_candidate_generated);
    report.affine_candidates += usize::from(analysis.affine_candidate_generated);
    report.candidate_generated += usize::from(
        analysis.bitwise_candidate_generated || analysis.affine_candidate_generated,
    );
    report.certification_attempted += usize::from(!analysis.attempts.is_empty());
    if let Some(states) = max_states(&analysis) {
        report.states.push(states);
    }
    report.nodes_after.push(analysis.result.size());

    if analysis.changed && residual_is_zero(expected, analysis.result.clone()) {
        report.candidate_certified += 1;
        if analysis.result.size() < input_size {
            report.result_reduced += 1;
        } else {
            report.certified_not_reduced += 1;
        }
        report.ng_resolved += 1;
        *report.resolved_by_dataset.entry(case.dataset).or_default() += 1;
        report
            .resolved_lines
            .push(format!("{}:{}", case.dataset, case.line));
        return;
    }

    let cause = if analysis.changed {
        report.result_still_nonzero += 1;
        "ResultStillNonZero"
    } else {
        match analysis.failure {
            Some(P9Failure::NoCandidate) => {
                report.candidate_not_found += 1;
                "NoCandidate"
            }
            Some(P9Failure::CandidateRejected) => {
                report.candidate_rejected += 1;
                "CandidateRejected"
            }
            Some(P9Failure::Unsupported) => {
                report.unsupported_fragment += 1;
                let mut details = Vec::new();
                collect_mul_details(&input, &mut details);
                report.unsupported_details.push(format!(
                    "{}:{} input={} mul_nodes={}",
                    case.dataset,
                    case.line,
                    input,
                    details.join(" || ")
                ));
                "Unsupported"
            }
            Some(P9Failure::BudgetExceeded) => {
                if has_budget_reason(&analysis, UnknownReason::StateBudget) {
                    report.state_budget_exceeded += 1;
                } else {
                    report.other_budget_exceeded += 1;
                }
                "BudgetExceeded"
            }
            None => "ResultStillNonZero",
        }
    };
    report
        .failure_lines
        .push(format!("{}:{}:{cause}", case.dataset, case.line));
}

fn baseline_case(case: &Case) -> Option<(Expr, Expr)> {
    let (Ok(input), Ok(expected)) = (
        simplify_mba(case.mba.clone(), WIDTH),
        simplify_mba(case.expected.clone(), WIDTH),
    ) else {
        return None;
    };
    (!residual_is_zero(expected.clone(), input.clone())).then_some((input, expected))
}

fn run_p9_alone(cases: &[Case]) -> Report {
    let mut report = Report::default();
    for case in cases {
        let Some((input, expected)) = baseline_case(case) else {
            continue;
        };
        report.ng_input += 1;
        report.nodes_before.push(input.size());
        record_p9(&mut report, case, input, expected);
    }
    report
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

fn run_p7e_p9(cases: &[Case]) -> Report {
    let mut report = Report::default();
    for case in cases {
        let Some((input, expected)) = baseline_case(case) else {
            continue;
        };
        report.ng_input += 1;
        report.nodes_before.push(input.size());
        let started = std::time::Instant::now();
        let after_p7e = p7e_prepass(input);
        report.prepass_time += started.elapsed();
        if residual_is_zero(expected.clone(), after_p7e.clone()) {
            report.ng_resolved += 1;
            report.prepass_resolved += 1;
            report.nodes_after.push(after_p7e.size());
            *report.resolved_by_dataset.entry(case.dataset).or_default() += 1;
            report
                .resolved_lines
                .push(format!("{}:{}", case.dataset, case.line));
        } else {
            record_p9(&mut report, case, after_p7e, expected);
        }
    }
    report
}

fn p7e_p8_prepass(input: Expr) -> Expr {
    let Ok((diagnosed, trace)) = diagnose_hidden_atoms(input.clone(), WIDTH) else {
        return input;
    };
    let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed) else {
        return input;
    };
    experiment_pipeline(scope)
        .ok()
        .flatten()
        .map_or(input, |pipeline| pipeline.result)
}

fn run_p7e_p8_p9(cases: &[Case]) -> Report {
    let mut report = Report::default();
    for case in cases {
        let Some((input, expected)) = baseline_case(case) else {
            continue;
        };
        report.ng_input += 1;
        report.nodes_before.push(input.size());
        let started = std::time::Instant::now();
        let after_p8 = p7e_p8_prepass(input);
        report.prepass_time += started.elapsed();
        if residual_is_zero(expected.clone(), after_p8.clone()) {
            report.ng_resolved += 1;
            report.prepass_resolved += 1;
            report.nodes_after.push(after_p8.size());
            *report.resolved_by_dataset.entry(case.dataset).or_default() += 1;
            report
                .resolved_lines
                .push(format!("{}:{}", case.dataset, case.line));
        } else {
            record_p9(&mut report, case, after_p8, expected);
        }
    }
    report
}

fn print_report(variant: &str, report: &Report) {
    let total = report.prepass_time + report.candidate_generation + report.verification;
    println!("variant={variant}");
    println!("NG_input={}", report.ng_input);
    println!("NG_resolved={}", report.ng_resolved);
    println!("NG_remaining={}", report.ng_input - report.ng_resolved);
    println!("prepass_resolved={}", report.prepass_resolved);
    println!("p9_inputs={}", report.p9_inputs);
    println!("candidate_generated={}", report.candidate_generated);
    println!(
        "certification_attempted={}",
        report.certification_attempted
    );
    println!("candidate_certified={}", report.candidate_certified);
    println!("result_reduced={}", report.result_reduced);
    println!(
        "certified_not_reduced={}",
        report.certified_not_reduced
    );
    println!();
    println!("bitwise_candidates={}", report.bitwise_candidates);
    println!("affine_candidates={}", report.affine_candidates);
    println!("candidate_not_found={}", report.candidate_not_found);
    println!("candidate_rejected={}", report.candidate_rejected);
    println!("unsupported_fragment={}", report.unsupported_fragment);
    println!("state_budget_exceeded={}", report.state_budget_exceeded);
    println!("other_budget_exceeded={}", report.other_budget_exceeded);
    println!("result_still_nonzero={}", report.result_still_nonzero);
    println!();
    println!(
        "prepass_ms={:.3}",
        report.prepass_time.as_secs_f64() * 1_000.0
    );
    println!(
        "candidate_generation_ms={:.3}",
        report.candidate_generation.as_secs_f64() * 1_000.0
    );
    println!(
        "verification_ms={:.3}",
        report.verification.as_secs_f64() * 1_000.0
    );
    println!("total_ms={:.3}", total.as_secs_f64() * 1_000.0);
    println!();
    println!("median_states={}", percentile(&report.states, 50));
    println!("p95_states={}", percentile(&report.states, 95));
    println!("max_states={}", report.states.iter().copied().max().unwrap_or(0));
    println!("median_nodes_before={}", percentile(&report.nodes_before, 50));
    println!("median_nodes_after={}", percentile(&report.nodes_after, 50));
    for (dataset, resolved) in &report.resolved_by_dataset {
        println!("dataset={dataset} resolved={resolved}");
    }
    println!("resolved_lines=[{}]", report.resolved_lines.join(","));
    println!("failure_lines=[{}]", report.failure_lines.join(","));
    println!(
        "unsupported_exact_ast_details=[{}]",
        report.unsupported_details.join(" | ")
    );
    println!("expected_used_for_candidate_generation=false");
    println!("intermediate_serialization=false");
    println!("normal_path_modified=false");
}

fn main() {
    let cases = load_cases();
    let p9_alone = run_p9_alone(&cases);
    print_report("P9_alone", &p9_alone);
    if p9_alone.ng_resolved != p9_alone.ng_input {
        println!();
        let p7e_p9 = run_p7e_p9(&cases);
        print_report("P7e_then_P9", &p7e_p9);
        if p7e_p9.ng_resolved != p7e_p9.ng_input {
            println!();
            let p7e_p8_p9 = run_p7e_p8_p9(&cases);
            print_report("P7e_then_P8_then_P9", &p7e_p8_p9);
        }
    }
}

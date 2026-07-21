use std::time::Duration;

use rumba_core::{
    expr::Expr,
    p9::{
        Certification, P9Failure, P9Limits, UnknownReason, analyze, analyze_linear,
    },
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
    base: Expr,
    expected: Expr,
}

#[derive(Default)]
struct Report {
    sources: usize,
    pipeline_resolved: usize,
    prepass_resolved: usize,
    p9l_inputs: usize,
    projection_proved: usize,
    projection_reduced: usize,
    proved_not_smaller: usize,
    linear_projection_counterexample: usize,
    unsupported_mul: usize,
    budget_exceeded: usize,
    result_still_nonzero: usize,
    resolved_new_candidate_rejects: usize,
    legacy_resolved_regressions: usize,
    candidate_generation: Duration,
    verification: Duration,
    prepass_time: Duration,
    states: Vec<usize>,
    resolved_lines: Vec<String>,
    counterexample_lines: Vec<String>,
    unsupported_lines: Vec<String>,
    new_resolution_lines: Vec<String>,
}

fn load_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().enumerate() {
            if row.trim().is_empty() {
                continue;
            }
            let (source, expected) = row.split_once(',').unwrap();
            let (Ok(base), Ok(expected)) = (
                simplify_mba(parse_expr(source.trim()).unwrap(), WIDTH),
                simplify_mba(parse_expr(expected.trim()).unwrap(), WIDTH),
            ) else {
                continue;
            };
            if residual_is_zero(expected.clone(), base.clone()) {
                continue;
            }
            cases.push(Case {
                dataset,
                line: index + 1,
                base,
                expected,
            });
        }
    }
    cases
}

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

fn states(certification: &Certification) -> Option<usize> {
    match certification {
        Certification::ProvedEquivalent(proof) => Some(proof.max_reachable_states),
        Certification::NotEquivalent(counterexample) => {
            Some(counterexample.max_reachable_states)
        }
        Certification::Unknown(unknown) => Some(unknown.max_reachable_states),
        Certification::Unsupported => None,
    }
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

fn record_p9l(report: &mut Report, case: &Case, input: Expr) {
    report.p9l_inputs += 1;
    let legacy = analyze(input.clone(), WIDTH, P9Limits::default());
    let linear = analyze_linear(input, WIDTH, P9Limits::default());
    report.candidate_generation += linear.candidate_generation_time;
    report.verification += linear.verification_time;
    if let Some(states) = states(&linear.certification) {
        report.states.push(states);
    }

    let line = format!("{}:{}", case.dataset, case.line);
    match &linear.certification {
        Certification::ProvedEquivalent(_) => {
            report.projection_proved += 1;
            if linear.changed {
                report.projection_reduced += 1;
            } else {
                report.proved_not_smaller += 1;
            }
        }
        Certification::NotEquivalent(_) => {
            report.linear_projection_counterexample += 1;
            report.counterexample_lines.push(line.clone());
        }
        Certification::Unknown(unknown) => {
            report.budget_exceeded += usize::from(matches!(
                unknown.reason,
                UnknownReason::VariableBudget
                    | UnknownReason::DagNodeBudget
                    | UnknownReason::StateBudget
            ));
        }
        Certification::Unsupported => {
            report.unsupported_mul += 1;
            report.unsupported_lines.push(line.clone());
        }
    }

    let resolved = linear.changed
        && residual_is_zero(case.expected.clone(), linear.result.clone());
    let legacy_resolved = legacy.changed
        && residual_is_zero(case.expected.clone(), legacy.result.clone());
    if resolved {
        report.pipeline_resolved += 1;
        report.resolved_lines.push(line.clone());
    } else if linear.changed {
        report.result_still_nonzero += 1;
    }
    if legacy.failure == Some(P9Failure::CandidateRejected) && resolved {
        report.resolved_new_candidate_rejects += 1;
        report.new_resolution_lines.push(line);
    }
    report.legacy_resolved_regressions += usize::from(legacy_resolved && !resolved);
}

fn run(cases: &[Case], with_p7e: bool) -> Report {
    let mut report = Report::default();
    for case in cases {
        report.sources += 1;
        let input = if with_p7e {
            let started = std::time::Instant::now();
            let input = p7e_prepass(case.base.clone());
            report.prepass_time += started.elapsed();
            input
        } else {
            case.base.clone()
        };
        if with_p7e && residual_is_zero(case.expected.clone(), input.clone()) {
            report.pipeline_resolved += 1;
            report.prepass_resolved += 1;
            report
                .resolved_lines
                .push(format!("{}:{}", case.dataset, case.line));
            continue;
        }
        record_p9l(&mut report, case, input);
    }
    report
}

fn print_report(name: &str, report: &Report) {
    let total = report.prepass_time + report.candidate_generation + report.verification;
    println!("variant={name}");
    println!("sources={}", report.sources);
    println!("pipeline_resolved={}", report.pipeline_resolved);
    println!("prepass_resolved={}", report.prepass_resolved);
    println!("p9l_inputs={}", report.p9l_inputs);
    println!("projection_proved={}", report.projection_proved);
    println!("projection_reduced={}", report.projection_reduced);
    println!("proved_not_smaller={}", report.proved_not_smaller);
    println!(
        "linear_projection_counterexample={}",
        report.linear_projection_counterexample
    );
    println!("unsupported_mul={}", report.unsupported_mul);
    println!("budget_exceeded={}", report.budget_exceeded);
    println!("result_still_nonzero={}", report.result_still_nonzero);
    println!(
        "resolved_new_candidate_rejects={}",
        report.resolved_new_candidate_rejects
    );
    println!(
        "legacy_resolved_regressions={}",
        report.legacy_resolved_regressions
    );
    println!(
        "states_median={}",
        percentile(&report.states, 50)
    );
    println!("states_p95={}", percentile(&report.states, 95));
    println!("states_max={}", report.states.iter().copied().max().unwrap_or(0));
    println!(
        "candidate_generation_ms={:.3}",
        report.candidate_generation.as_secs_f64() * 1_000.0
    );
    println!(
        "verification_ms={:.3}",
        report.verification.as_secs_f64() * 1_000.0
    );
    println!("time_ms={:.3}", total.as_secs_f64() * 1_000.0);
    println!("resolved_lines=[{}]", report.resolved_lines.join(","));
    println!(
        "counterexample_lines=[{}]",
        report.counterexample_lines.join(",")
    );
    println!(
        "unsupported_lines=[{}]",
        report.unsupported_lines.join(",")
    );
    println!(
        "new_resolution_lines=[{}]",
        report.new_resolution_lines.join(",")
    );
    println!("expected_used_for_candidate_generation=false");
    println!("p8_used=false");
}

fn main() {
    let cases = load_cases();
    assert_eq!(cases.len(), 101);
    print_report("P9L_base", &run(&cases, false));
    println!();
    print_report("P7e_then_P9L", &run(&cases, true));
}

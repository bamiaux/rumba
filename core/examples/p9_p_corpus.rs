use std::time::Duration;

use rumba_core::{
    expr::Expr,
    p9::{Certification, P9Limits, analyze_linear},
    p9_p::{P9PFailure, P9PLimits, analyze_global_polynomial},
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

#[derive(Default)]
struct Metrics {
    cases: usize,
    projection_found: usize,
    howell_no_solution: usize,
    holdout_rejected: usize,
    candidate_certified: usize,
    candidate_rejected: usize,
    budget_exceeded: usize,
    resolved: usize,
    certified_not_smaller: usize,
    monomials_max: usize,
    evaluations_max: usize,
    basis_time: Duration,
    system_time: Duration,
    certification_time: Duration,
    lines: Vec<String>,
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

fn record(
    metrics: &mut Metrics,
    dataset: &str,
    line: usize,
    input: Expr,
    expected: Expr,
    degree: usize,
    limits: P9PLimits,
) {
    let analysis = analyze_global_polynomial(input.clone(), WIDTH, degree, limits);
    metrics.cases += 1;
    metrics.projection_found += usize::from(analysis.system_solved);
    metrics.candidate_certified += usize::from(analysis.candidate_certified);
    metrics.candidate_rejected += usize::from(matches!(
        analysis.failure,
        Some(P9PFailure::HoldoutRejected | P9PFailure::PctRejected)
    ));
    metrics.howell_no_solution +=
        usize::from(analysis.failure == Some(P9PFailure::HowellNoSolution));
    metrics.holdout_rejected +=
        usize::from(analysis.failure == Some(P9PFailure::HoldoutRejected));
    metrics.budget_exceeded += usize::from(matches!(
        analysis.failure,
        Some(
            P9PFailure::VariableBudget
                | P9PFailure::MonomialBudget
                | P9PFailure::EvaluationBudget
                | P9PFailure::CandidateBudget
        )
    ));
    let resolved = analysis.changed
        && residual_is_zero(expected.clone(), analysis.result.clone());
    metrics.resolved += usize::from(resolved);
    metrics.certified_not_smaller +=
        usize::from(analysis.candidate_certified && !analysis.changed);
    metrics.monomials_max = metrics.monomials_max.max(analysis.monomials);
    metrics.evaluations_max = metrics.evaluations_max.max(analysis.evaluations);
    metrics.basis_time += analysis.basis_time;
    metrics.system_time += analysis.system_time;
    metrics.certification_time += analysis.certification_time;
    metrics.lines.push(format!(
        "{dataset}:{line}:vars={}:monomials={}:evaluations={}:system_solved={}:\
holdout={}:certified={}:changed={}:resolved={}:failure={:?}:nodes={}->{}",
        analysis.variables,
        analysis.monomials,
        analysis.evaluations,
        analysis.system_solved,
        analysis.holdout_passed,
        analysis.candidate_certified,
        analysis.changed,
        resolved,
        analysis.failure,
        input.size(),
        analysis.result.size(),
    ));
}

fn print_metrics(prefix: &str, metrics: &Metrics) {
    println!("{prefix}_cases={}", metrics.cases);
    println!("{prefix}_projection_found={}", metrics.projection_found);
    println!("{prefix}_howell_no_solution={}", metrics.howell_no_solution);
    println!("{prefix}_holdout_rejected={}", metrics.holdout_rejected);
    println!("{prefix}_candidate_certified={}", metrics.candidate_certified);
    println!("{prefix}_candidate_rejected={}", metrics.candidate_rejected);
    println!("{prefix}_budget_exceeded={}", metrics.budget_exceeded);
    println!("{prefix}_resolved={}", metrics.resolved);
    println!(
        "{prefix}_certified_not_smaller={}",
        metrics.certified_not_smaller
    );
    println!("{prefix}_monomials_max={}", metrics.monomials_max);
    println!("{prefix}_evaluations_max={}", metrics.evaluations_max);
    println!(
        "{prefix}_basis_ms={:.3}",
        metrics.basis_time.as_secs_f64() * 1_000.0
    );
    println!(
        "{prefix}_system_ms={:.3}",
        metrics.system_time.as_secs_f64() * 1_000.0
    );
    println!(
        "{prefix}_certification_ms={:.3}",
        metrics.certification_time.as_secs_f64() * 1_000.0
    );
    println!("{prefix}_lines=[{}]", metrics.lines.join(" | "));
}

fn main() {
    let mut sources = 0;
    let mut mul_degree_2 = Metrics::default();
    let mut nonlinear_degree_2 = Metrics::default();
    let mut mul_degree_3 = Metrics::default();
    let mut nonlinear_degree_3 = Metrics::default();
    let degree_3_limits = P9PLimits {
        max_degree: 3,
        ..P9PLimits::default()
    };

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
            match linear.certification {
                Certification::Unsupported => {
                    record(
                        &mut mul_degree_2,
                        dataset,
                        index + 1,
                        input.clone(),
                        expected.clone(),
                        2,
                        P9PLimits::default(),
                    );
                    record(
                        &mut mul_degree_3,
                        dataset,
                        index + 1,
                        input,
                        expected,
                        3,
                        degree_3_limits,
                    );
                }
                Certification::NotEquivalent(_) => {
                    record(
                        &mut nonlinear_degree_2,
                        dataset,
                        index + 1,
                        input.clone(),
                        expected.clone(),
                        2,
                        P9PLimits::default(),
                    );
                    record(
                        &mut nonlinear_degree_3,
                        dataset,
                        index + 1,
                        input,
                        expected,
                        3,
                        degree_3_limits,
                    );
                }
                Certification::ProvedEquivalent(_) | Certification::Unknown(_) => {}
            }
        }
    }

    assert_eq!(sources, 101);
    assert_eq!(mul_degree_2.cases, 12);
    assert_eq!(nonlinear_degree_2.cases, 4);
    assert_eq!(mul_degree_3.cases, 12);
    assert_eq!(nonlinear_degree_3.cases, 4);
    println!("sources={sources}");
    print_metrics("mul_degree_2", &mul_degree_2);
    print_metrics("mul_degree_3", &mul_degree_3);
    print_metrics("nonlinear_degree_2", &nonlinear_degree_2);
    print_metrics("nonlinear_degree_3", &nonlinear_degree_3);
    println!("degree_3_attempted=true");
    println!("expected_used_for_candidate_generation=false");
    println!("sat_or_smt_used=false");
}

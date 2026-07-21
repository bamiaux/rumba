use std::collections::{BTreeMap, BTreeSet};

use rumba_core::{
    expr::Expr,
    p8::experiment_pipeline,
    p9::{CandidateKind, P9Failure, P9Limits, analyze},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
};

const WIDTH: u8 = 64;
const REMAINING: &str = include_str!("../../remaining_unresolved.csv");
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

fn p7e_p8_branch(input: Expr) -> Expr {
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

fn remaining_lines() -> BTreeSet<(String, usize)> {
    REMAINING
        .lines()
        .skip(1)
        .filter(|row| !row.trim().is_empty())
        .map(|row| {
            let mut columns = row.splitn(3, ',');
            let dataset = columns.next().unwrap().trim_matches('"').to_owned();
            let line = columns.next().unwrap().parse().unwrap();
            (dataset, line)
        })
        .collect()
}

fn failure_name(failure: Option<P9Failure>) -> &'static str {
    match failure {
        None => "None",
        Some(P9Failure::NoCandidate) => "NoCandidate",
        Some(P9Failure::CandidateRejected) => "CandidateRejected",
        Some(P9Failure::Unsupported) => "Unsupported",
        Some(P9Failure::BudgetExceeded) => "BudgetExceeded",
    }
}

fn kind_name(kind: Option<CandidateKind>) -> &'static str {
    match kind {
        Some(CandidateKind::Bitwise) => "Bitwise",
        Some(CandidateKind::Affine) => "Affine",
        None => "None",
    }
}

fn main() {
    let remaining = remaining_lines();
    assert_eq!(remaining.len(), 41);
    let mut counts = BTreeMap::<&'static str, usize>::new();
    println!(
        "dataset,line,in_remaining_41,p9_from_base_resolved,p7e_p8_resolved,both,p9_only,\
p7e_p8_only,neither,unsupported_mul_by_p9,resolved_by_p7e_p8,p9_failure,\
p9_candidate_kind,base_nodes,p9_nodes,p7e_p8_nodes,chosen_branch,chosen_nodes,chosen_resolved"
    );

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

            *counts.entry("ng_input").or_default() += 1;
            let line = index + 1;
            let in_remaining = remaining.contains(&(dataset.to_owned(), line));
            let p9 = analyze(base.clone(), WIDTH, P9Limits::default());
            let p8 = p7e_p8_branch(base.clone());
            let p9_resolved =
                p9.changed && residual_is_zero(expected.clone(), p9.result.clone());
            let p8_resolved = residual_is_zero(expected.clone(), p8.clone());
            let both = p9_resolved && p8_resolved;
            let p9_only = p9_resolved && !p8_resolved;
            let p8_only = !p9_resolved && p8_resolved;
            let neither = !p9_resolved && !p8_resolved;
            let unsupported = p9.failure == Some(P9Failure::Unsupported);
            let accepted_kind = p9
                .changed
                .then(|| p9.attempts.last().map(|attempt| attempt.kind))
                .flatten();

            for (name, present) in [
                ("p9_resolved", p9_resolved),
                ("p8_resolved", p8_resolved),
                ("both", both),
                ("p9_only", p9_only),
                ("p8_only", p8_only),
                ("neither", neither),
                ("unsupported", unsupported),
                ("unsupported_p8_resolved", unsupported && p8_resolved),
                ("unsupported_p8_unresolved", unsupported && !p8_resolved),
                ("remaining_p9_resolved", in_remaining && p9_resolved),
                ("remaining_p9_unresolved", in_remaining && !p9_resolved),
            ] {
                *counts.entry(name).or_default() += usize::from(present);
            }

            let choices = [
                ("base", &base),
                ("p7e_p8", &p8),
                ("p9", &p9.result),
            ];
            let (chosen_branch, chosen) = choices
                .into_iter()
                .min_by_key(|(name, expression)| {
                    (expression.size(), expression.to_string(), *name)
                })
                .unwrap();
            let chosen_resolved = residual_is_zero(expected, chosen.clone());
            *counts.entry(chosen_branch).or_default() += 1;
            *counts.entry("chosen_resolved").or_default() += usize::from(chosen_resolved);

            println!(
                "{dataset},{line},{in_remaining},{p9_resolved},{p8_resolved},{both},\
{p9_only},{p8_only},{neither},{unsupported},{p8_resolved},{},{},{},{},{},{},{},{chosen_resolved}",
                failure_name(p9.failure),
                kind_name(accepted_kind),
                base.size(),
                p9.result.size(),
                p8.size(),
                chosen_branch,
                chosen.size(),
            );
        }
    }

    assert_eq!(counts["ng_input"], 101);
    eprintln!("NG_input={}", counts["ng_input"]);
    for name in [
        "p9_resolved",
        "p8_resolved",
        "both",
        "p9_only",
        "p8_only",
        "neither",
        "unsupported",
        "unsupported_p8_resolved",
        "unsupported_p8_unresolved",
        "remaining_p9_resolved",
        "remaining_p9_unresolved",
        "chosen_resolved",
        "base",
        "p7e_p8",
        "p9",
    ] {
        eprintln!("{name}={}", counts.get(name).copied().unwrap_or(0));
    }
    eprintln!(
        "union_resolved={}",
        counts["ng_input"] - counts.get("neither").copied().unwrap_or(0)
    );
    eprintln!("expected_used_for_branch_generation=false");
    eprintln!("intermediate_serialization=false");
}

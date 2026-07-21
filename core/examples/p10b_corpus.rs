use rumba_core::{
    expr::Expr,
    p10b::{P10bLimits, analyze_contextual_linearization},
    p9::{Certification, P9Limits, analyze_linear},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure, simplify_mba},
};

const WIDTH: u8 = 64;
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");

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

fn main() {
    let mut cases = 0;
    let mut nonlinear_factor_projections = 0;
    let mut pct_attempts = 0;
    let mut reduced_width_attempts = 0;
    let mut contextual_rewrites = 0;
    let mut changed = 0;
    let mut resolved = 0;
    let mut budget_exceeded = 0;
    let mut reports = Vec::new();

    for (index, row) in QSYNTH.lines().enumerate() {
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
        let input = p7e_prepass(base);
        if residual_is_zero(expected.clone(), input.clone()) {
            continue;
        }
        if !matches!(
            analyze_linear(input.clone(), WIDTH, P9Limits::default()).certification,
            Certification::Unsupported
        ) {
            continue;
        }

        cases += 1;
        let analysis = analyze_contextual_linearization(
            input.clone(),
            WIDTH,
            P10bLimits::default(),
        );
        let line_resolved = analysis.changed
            && residual_is_zero(expected.clone(), analysis.result.clone());
        nonlinear_factor_projections += analysis.nonlinear_factor_projections;
        pct_attempts += analysis.pct_attempts;
        reduced_width_attempts += analysis.reduced_width_attempts;
        contextual_rewrites += analysis.rewrites.len();
        changed += usize::from(analysis.changed);
        resolved += usize::from(line_resolved);
        budget_exceeded += usize::from(analysis.budget_exceeded);
        reports.push(format!(
            "qsynth_ea.csv:{}\nsource={}\nexpected={}\nnonlinear_factor_projections={}\n\
pct_attempts={}\nreduced_width_attempts={}\nrewrites={:?}\nchanged={}\nresolved={}\n\
budget_exceeded={}\nnodes={}->{}",
            index + 1,
            input,
            expected,
            analysis.nonlinear_factor_projections,
            analysis.pct_attempts,
            analysis.reduced_width_attempts,
            analysis.rewrites,
            analysis.changed,
            line_resolved,
            analysis.budget_exceeded,
            input.size(),
            analysis.result.size(),
        ));
    }

    assert_eq!(cases, 12);
    println!("cases={cases}");
    println!("nonlinear_factor_projections={nonlinear_factor_projections}");
    println!("pct_attempts={pct_attempts}");
    println!("reduced_width_attempts={reduced_width_attempts}");
    println!("contextual_rewrites={contextual_rewrites}");
    println!("changed={changed}");
    println!("resolved={resolved}");
    println!("budget_exceeded={budget_exceeded}");
    println!("expected_used_for_candidate_generation=false");
    println!("reports_begin");
    println!("{}", reports.join("\n---\n"));
    println!("reports_end");
}

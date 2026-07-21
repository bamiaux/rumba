use std::time::{Duration, Instant};

use rumba_core::{
    expr::Expr,
    p11a::{P11aAnalysis, P11aLimits, compress_by_proof_key},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const MUL_LINES: [usize; 12] = [13, 53, 77, 125, 134, 210, 234, 260, 294, 369, 481, 486];

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

fn run(input: Expr, representatives: usize) -> (P11aAnalysis, Duration) {
    let limits = P11aLimits {
        max_representatives_per_key: representatives,
        ..P11aLimits::default()
    };
    let started = Instant::now();
    let analysis = compress_by_proof_key(input, WIDTH, limits);
    (analysis, started.elapsed())
}

fn percentile(times: &[Duration], numerator: usize, denominator: usize) -> Duration {
    let index = ((times.len() - 1) * numerator).div_ceil(denominator);
    times[index]
}

fn main() {
    let mut pareto_times = Vec::new();
    let mut best_only_times = Vec::new();
    let mut reduced = 0;
    let mut reduced_25 = 0;
    let mut reduced_50 = 0;
    let mut resolved = 0;
    let mut pareto_wins = 0;
    let mut surfaces = 0;
    let mut residual_proofs = 0;
    let mut budgets = 0;

    for line in MUL_LINES {
        let row = QSYNTH.lines().nth(line - 1).unwrap();
        let (source_text, expected_text) = row.split_once(',').unwrap();
        let source = parse_expr(source_text.trim()).unwrap();
        let expected = parse_expr(expected_text.trim())
            .unwrap()
            .reduce(make_mask(WIDTH));
        let input = p7e_input(source);
        let (best_only, best_time) = run(input.clone(), 1);
        let (pareto, pareto_time) = run(input.clone(), 8);
        best_only_times.push(best_time);
        pareto_times.push(pareto_time);

        let input_nodes = input.size();
        let result_nodes = pareto.result.size();
        let reduction = input_nodes.saturating_sub(result_nodes);
        let reduction_percent = 100.0 * reduction as f64 / input_nodes as f64;
        let is_reduced = result_nodes < input_nodes;
        let is_resolved = pareto.proof_verified && result_nodes <= expected.size();
        let pareto_win = pareto.result.size() < best_only.result.size();
        reduced += usize::from(is_reduced);
        reduced_25 += usize::from(reduction * 4 >= input_nodes);
        reduced_50 += usize::from(reduction * 2 >= input_nodes);
        resolved += usize::from(is_resolved);
        pareto_wins += usize::from(pareto_win);
        surfaces += pareto.compact_subterms_generated;
        residual_proofs += pareto.residual_proofs_attempted;
        budgets += usize::from(pareto.budget_exceeded);

        println!(
            "line={line} input_nodes={input_nodes} best_only_nodes={} pareto_nodes={result_nodes} expected_nodes={} reduced={is_reduced} reduction_percent={reduction_percent:.1} resolved={is_resolved} pareto_win={pareto_win} time_best_ms={:.3} time_pareto_ms={:.3} surfaces={} promoted={} reused={} residual_proofs={} residual_proved={} budget_exceeded={} pruned_budget={}",
            best_only.result.size(),
            expected.size(),
            best_time.as_secs_f64() * 1_000.0,
            pareto_time.as_secs_f64() * 1_000.0,
            pareto.compact_subterms_generated,
            pareto.promoted_as_atom,
            pareto.reused_by_parent,
            pareto.residual_proofs_attempted,
            pareto.residual_proofs_proved,
            pareto.budget_exceeded,
            pareto.candidate_pruned_budget,
        );
    }

    pareto_times.sort();
    best_only_times.sort();
    println!("mul_cases={}", MUL_LINES.len());
    println!("resolved={resolved}");
    println!("cost_reduced={reduced}");
    println!("reduction_ge_25_percent={reduced_25}");
    println!("reduction_ge_50_percent={reduced_50}");
    println!("pareto_wins_over_best_only={pareto_wins}");
    println!("surfaces_generated={surfaces}");
    println!("secondary_proofs={residual_proofs}");
    println!("budgets_reached={budgets}");
    println!(
        "best_only_time_ms=median:{:.3},p95:{:.3},max:{:.3}",
        percentile(&best_only_times, 1, 2).as_secs_f64() * 1_000.0,
        percentile(&best_only_times, 95, 100).as_secs_f64() * 1_000.0,
        best_only_times.last().unwrap().as_secs_f64() * 1_000.0,
    );
    println!(
        "pareto_time_ms=median:{:.3},p95:{:.3},max:{:.3}",
        percentile(&pareto_times, 1, 2).as_secs_f64() * 1_000.0,
        percentile(&pareto_times, 95, 100).as_secs_f64() * 1_000.0,
        pareto_times.last().unwrap().as_secs_f64() * 1_000.0,
    );
    println!("expected_used_for_candidate_generation=false");
}

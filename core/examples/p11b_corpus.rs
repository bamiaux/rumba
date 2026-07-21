use std::time::{Duration, Instant};

use rumba_core::{
    expr::Expr,
    p11b::{P11bLimits, synthesize_carry_grammar},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const LINES: [usize; 4] = [25, 114, 139, 423];

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

fn percentile(times: &[Duration], numerator: usize, denominator: usize) -> Duration {
    let index = ((times.len() - 1) * numerator).div_ceil(denominator);
    times[index]
}

fn main() {
    let mut times = Vec::new();
    let mut resolved = 0;
    let mut reduced = 0;
    let mut proofs = 0;
    for line in LINES {
        let row = QSYNTH.lines().nth(line - 1).unwrap();
        let (source_text, expected_text) = row.split_once(',').unwrap();
        let source = parse_expr(source_text.trim()).unwrap();
        let expected = parse_expr(expected_text.trim())
            .unwrap()
            .reduce(make_mask(WIDTH));
        let input = p7e_input(source);
        let started = Instant::now();
        let analysis = synthesize_carry_grammar(input.clone(), WIDTH, P11bLimits::default());
        let elapsed = started.elapsed();
        times.push(elapsed);
        let is_resolved = analysis.proof_verified && analysis.result.size() <= expected.size();
        resolved += usize::from(is_resolved);
        reduced += usize::from(analysis.result.size() < input.size());
        proofs += analysis.certifications_attempted;
        println!(
            "line={line} input_nodes={} result_nodes={} expected_nodes={} reduced={} resolved={is_resolved} proof_verified={} bases={} pairs={} sums={} candidates={} observation_matches={} certifications={} proved={} failure={:?} time_ms={:.3}",
            input.size(),
            analysis.result.size(),
            expected.size(),
            analysis.changed,
            analysis.proof_verified,
            analysis.bases,
            analysis.bitwise_pairs,
            analysis.additive_surfaces,
            analysis.candidates_generated,
            analysis.observation_matches,
            analysis.certifications_attempted,
            analysis.certifications_proved,
            analysis.failure,
            elapsed.as_secs_f64() * 1_000.0,
        );
        println!("result={}", analysis.result);
    }
    times.sort();
    println!("p11b_cases={}", LINES.len());
    println!("resolved={resolved}");
    println!("cost_reduced={reduced}");
    println!("certifications={proofs}");
    println!(
        "time_ms=median:{:.3},p95:{:.3},max:{:.3}",
        percentile(&times, 1, 2).as_secs_f64() * 1_000.0,
        percentile(&times, 95, 100).as_secs_f64() * 1_000.0,
        times.last().unwrap().as_secs_f64() * 1_000.0,
    );
    println!("expected_used_for_candidate_generation=false");
}

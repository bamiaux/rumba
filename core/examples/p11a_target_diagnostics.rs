use rumba_core::{
    expr::Expr,
    p11a::{P11aLimits, diagnose_target_subtrees},
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, experiment_bitwise_dependency_closure},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const LINES: [usize; 2] = [53, 486];

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

fn main() {
    for line in LINES {
        let row = QSYNTH.lines().nth(line - 1).unwrap();
        let (source_text, expected_text) = row.split_once(',').unwrap();
        let source = parse_expr(source_text.trim()).unwrap();
        let target = parse_expr(expected_text.trim())
            .unwrap()
            .reduce(make_mask(WIDTH));
        let input = p7e_input(source);
        let (analysis, diagnostics) = diagnose_target_subtrees(
            input,
            &target,
            WIDTH,
            P11aLimits::default(),
        );
        let first_missing = diagnostics
            .iter()
            .filter(|entry| entry.children_available && !entry.surface_present)
            .min_by_key(|entry| (entry.subtree.size(), entry.subtree.to_string()));

        println!("line={line} result_nodes={} target_nodes={}", analysis.result.size(), target.size());
        if let Some(entry) = first_missing {
            println!("first_missing={}", entry.subtree);
        }
        for entry in diagnostics {
            println!(
                "target_subtree={} nodes={} proof_key_present={} surface_present={} first_generated_round={:?} first_archived_round={:?} pruned_by_cost={} pruned_by_pareto={} children_available={}",
                entry.subtree,
                entry.subtree.size(),
                entry.proof_key_present,
                entry.surface_present,
                entry.first_generated_round,
                entry.first_archived_round,
                entry.pruned_by_cost,
                entry.pruned_by_pareto,
                entry.children_available,
            );
        }
        println!("expected_used_for_generation=false");
        println!("---");
    }
}

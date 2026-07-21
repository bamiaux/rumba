use rumba_core::{
    expr::Expr,
    p9::{Certification, P9Limits, analyze_linear},
    p10a::{P10aLimits, analyze_alternating_normal_forms},
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
    let mut mul_cases = 0;
    let mut eligible = 0;
    let mut changed = 0;
    let mut resolved = 0;
    let mut semantic_regressions = 0;
    let mut semantic_bindings = 0;
    let mut non_identity_bindings = 0;
    let mut bitwise_normalizations = 0;
    let mut bitwise_atom_aborts = 0;
    let mut bitwise_size_aborts = 0;
    let mut traces = Vec::new();

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
        mul_cases += 1;
        let analysis = analyze_alternating_normal_forms(
            input.clone(),
            WIDTH,
            P10aLimits::default(),
        );
        if !analysis.all_factors_keyed {
            continue;
        }
        eligible += 1;
        changed += usize::from(analysis.changed);
        let line = index + 1;
        let line_resolved = analysis.changed
            && residual_is_zero(expected.clone(), analysis.result.clone());
        resolved += usize::from(line_resolved);
        semantic_regressions += usize::from(!sample_equal(&input, &analysis.result));

        let keys = analysis
            .bindings
            .iter()
            .enumerate()
            .map(|(key_index, binding)| {
                format!(
                    "K{key_index}(width={},vars={:?},coefficients={:?},states={},dag_nodes={}):{}=>{}",
                    binding.key.width,
                    binding.key.variables,
                    binding.key.conjunction_coefficients,
                    binding.key.proof.max_reachable_states,
                    binding.key.proof.dag_nodes,
                    binding.original,
                    binding.canonical,
                )
            })
            .collect::<Vec<_>>()
            .join(" ; ");
        let first = &analysis.poly_bitwise_poly;
        let first_bitwise = first.bitwise.as_ref().unwrap();
        let second = &analysis.bitwise_poly_bitwise;
        let second_final = second.bitwise_final.as_ref().unwrap();
        semantic_bindings += analysis.bindings.len();
        non_identity_bindings += analysis
            .bindings
            .iter()
            .filter(|binding| binding.original != binding.canonical)
            .count();
        bitwise_normalizations += first_bitwise.normalized
            + second.bitwise_first.normalized
            + second_final.normalized;
        bitwise_atom_aborts += first_bitwise.aborts_atom_limit
            + second.bitwise_first.aborts_atom_limit
            + second_final.aborts_atom_limit;
        bitwise_size_aborts += first_bitwise.aborts_size_limit
            + second.bitwise_first.aborts_size_limit
            + second_final.aborts_size_limit;
        traces.push(format!(
            "qsynth_ea.csv:{line}\nSemanticKey=[{keys}]\nKeyed={}\n\
SequenceA.PolyNF={}\nSequenceA.BitwiseNF={}\nSequenceA.PolyNF2={}\n\
SequenceA.stats=poly_monomials:{}->{},bitwise_normalized={},atom_aborts={},size_aborts={},max_atoms={}\n\
SequenceB.BitwiseNF={}\nSequenceB.PolyNF={}\nSequenceB.BitwiseNF2={}\n\
SequenceB.stats=poly_monomials:{},bitwise_normalized:{}->{},atom_aborts:{}->{},size_aborts:{}->{},max_atoms:{}->{}\n\
Outcome=changed:{},resolved:{},blocker:{:?},nodes:{}->{}",
            analysis.keyed_input,
            first.poly_first.as_ref().unwrap(),
            first_bitwise.result,
            first.poly_final.as_ref().unwrap(),
            first.poly_first_monomials,
            first.poly_final_monomials,
            first_bitwise.normalized,
            first_bitwise.aborts_atom_limit,
            first_bitwise.aborts_size_limit,
            first_bitwise.max_frontier_atoms,
            second.bitwise_first.result,
            second.poly.as_ref().unwrap(),
            second_final.result,
            second.poly_monomials,
            second.bitwise_first.normalized,
            second_final.normalized,
            second.bitwise_first.aborts_atom_limit,
            second_final.aborts_atom_limit,
            second.bitwise_first.aborts_size_limit,
            second_final.aborts_size_limit,
            second.bitwise_first.max_frontier_atoms,
            second_final.max_frontier_atoms,
            analysis.changed,
            line_resolved,
            analysis.blocker,
            input.size(),
            analysis.result.size(),
        ));
    }

    assert_eq!(mul_cases, 12);
    assert_eq!(eligible, 9);
    println!("mul_cases={mul_cases}");
    println!("eligible_all_factors_p9l={eligible}");
    println!("p10a_changed={changed}");
    println!("p10a_resolved={resolved}");
    println!("semantic_regressions={semantic_regressions}");
    println!("semantic_bindings={semantic_bindings}");
    println!("non_identity_bindings={non_identity_bindings}");
    println!("bitwise_normalizations={bitwise_normalizations}");
    println!("bitwise_atom_aborts={bitwise_atom_aborts}");
    println!("bitwise_size_aborts={bitwise_size_aborts}");
    println!("sequences_fixed=2");
    println!("expected_used_for_candidate_generation=false");
    println!("traces_begin");
    println!("{}", traces.join("\n---\n"));
    println!("traces_end");
}

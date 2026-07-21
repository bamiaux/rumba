use rumba_core::{
    expr::Expr,
    p9::{Certification, P9Limits, analyze_linear},
    p10c::{P10cLimits, analyze_binary_linear_composition},
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
    let mut changed = 0;
    let mut resolved = 0;
    let mut proved = 0;
    let mut counterexamples = 0;
    let mut unknown = 0;
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
            Certification::NotEquivalent(_)
        ) {
            continue;
        }

        cases += 1;
        let analysis = analyze_binary_linear_composition(
            input.clone(),
            WIDTH,
            P10cLimits::default(),
        );
        let line_resolved = analysis.changed
            && residual_is_zero(expected.clone(), analysis.result.clone());
        changed += usize::from(analysis.changed);
        resolved += usize::from(line_resolved);
        proved += analysis.proved_candidates.len();
        counterexamples += analysis.counterexamples;
        unknown += analysis.unknown;
        let candidates = analysis
            .proved_candidates
            .iter()
            .map(|candidate| {
                format!(
                    "table={:04b},left={},right={},candidate={}",
                    candidate.truth_table,
                    candidate.left,
                    candidate.right,
                    candidate.expression,
                )
            })
            .collect::<Vec<_>>()
            .join(" ; ");
        reports.push(format!(
            "qsynth_ea.csv:{}\nsource={}\nexpected={}\nparents={}\npairs={}\n\
observation_candidates={}\ncertifications={}\nproved={}\ncounterexamples={}\nunknown={}\n\
changed={}\nresolved={}\nfailure={:?}\nnodes={}->{}\nproved_candidates=[{}]",
            index + 1,
            input,
            expected,
            analysis.parents.len(),
            analysis.pairs_considered,
            analysis.observation_candidates,
            analysis.certifications,
            analysis.proved_candidates.len(),
            analysis.counterexamples,
            analysis.unknown,
            analysis.changed,
            line_resolved,
            analysis.failure,
            input.size(),
            analysis.result.size(),
            candidates,
        ));
    }

    assert_eq!(cases, 4);
    println!("cases={cases}");
    println!("changed={changed}");
    println!("resolved={resolved}");
    println!("proved_candidates={proved}");
    println!("counterexamples={counterexamples}");
    println!("unknown={unknown}");
    println!("expected_used_for_candidate_generation=false");
    println!("reports_begin");
    println!("{}", reports.join("\n---\n"));
    println!("reports_end");
}

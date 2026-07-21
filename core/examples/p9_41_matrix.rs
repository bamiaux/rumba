use rumba_core::{
    expr::Expr,
    p8::experiment_pipeline,
    p9::{
        Certification, P9Limits, certify_equivalent, generate_affine_candidate,
        generate_bitwise_candidate,
    },
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const CASES: &str = include_str!("../../remaining_unresolved.csv");

struct Case<'a> {
    dataset: &'a str,
    line: usize,
    source: &'a str,
    expected: &'a str,
    residual: &'a str,
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn parse_case(row: &str) -> Case<'_> {
    let columns = row.splitn(6, ',').collect::<Vec<_>>();
    assert_eq!(columns.len(), 6, "expected six CSV columns");
    Case {
        dataset: unquote(columns[0]),
        line: columns[1].parse().unwrap(),
        source: unquote(columns[2]),
        expected: unquote(columns[3]),
        residual: unquote(columns[4]),
    }
}

fn csv_field(value: impl AsRef<str>) -> String {
    format!("\"{}\"", value.as_ref().replace('"', "\"\""))
}

fn structural_repr(expression: &Expr) -> String {
    fn list(tag: &str, terms: &[Expr]) -> String {
        format!(
            "{tag}[{}]",
            terms
                .iter()
                .map(structural_repr)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
    match expression {
        Expr::Var(variable) => format!("V{}", variable.0),
        Expr::Const(constant) => format!("C{:016x}", constant.get(u64::MAX)),
        Expr::Not(inner) => format!("N({})", structural_repr(inner)),
        Expr::Scale(coefficient, inner) => format!(
            "S{:016x}({})",
            coefficient.get(u64::MAX),
            structural_repr(inner)
        ),
        Expr::And(terms) => list("A", terms),
        Expr::Or(terms) => list("O", terms),
        Expr::Xor(terms) => list("X", terms),
        Expr::Add(terms) => list("D", terms),
        Expr::Mul(terms) => list("M", terms),
    }
}

fn ast_hash(expression: &Expr) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in structural_repr(expression).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn variables(expression: &Expr) -> String {
    let mut variables = expression.get_vars().into_iter().collect::<Vec<_>>();
    variables.sort();
    variables
        .into_iter()
        .map(|variable| format!("v{}", variable.0))
        .collect::<Vec<_>>()
        .join("|")
}

fn hidden_atoms(expression: &Expr) -> String {
    let Ok((_, scopes)) = diagnose_hidden_atoms(expression.clone(), WIDTH) else {
        return "diagnostic_error".to_owned();
    };
    scopes
        .iter()
        .flat_map(|scope| &scope.atoms)
        .map(|atom| format!("v{}={}", atom.atom.0, atom.original))
        .collect::<Vec<_>>()
        .join("|")
}

fn collect_mul_nodes(expression: &Expr, output: &mut Vec<String>) {
    match expression {
        Expr::Mul(terms) => {
            let rendered = terms
                .iter()
                .map(|term| {
                    let folded = term.clone().reduce(make_mask(WIDTH));
                    format!(
                        "term={};is_constant={};after_fold={};after_fold_is_constant={}",
                        term,
                        matches!(term, Expr::Const(_)),
                        folded,
                        matches!(folded, Expr::Const(_)),
                    )
                })
                .collect::<Vec<_>>()
                .join("||");
            output.push(rendered);
            for term in terms {
                collect_mul_nodes(term, output);
            }
        }
        Expr::Not(inner) | Expr::Scale(_, inner) => collect_mul_nodes(inner, output),
        Expr::And(terms) | Expr::Or(terms) | Expr::Xor(terms) | Expr::Add(terms) => {
            for term in terms {
                collect_mul_nodes(term, output);
            }
        }
        Expr::Var(_) | Expr::Const(_) => {}
    }
}

fn mul_nodes(expression: &Expr) -> String {
    let mut nodes = Vec::new();
    collect_mul_nodes(expression, &mut nodes);
    nodes.join("###")
}

fn certification_name(certification: &Certification) -> &'static str {
    match certification {
        Certification::ProvedEquivalent(_) => "ProvedEquivalent",
        Certification::NotEquivalent(_) => "False",
        Certification::Unknown(_) => "Unknown",
        Certification::Unsupported => "Unsupported",
    }
}

fn states(certification: &Certification) -> usize {
    match certification {
        Certification::ProvedEquivalent(proof) => proof.max_reachable_states,
        Certification::NotEquivalent(counterexample) => counterexample.max_reachable_states,
        Certification::Unknown(unknown) => unknown.max_reachable_states,
        Certification::Unsupported => 0,
    }
}

fn first_failure_bit(certification: &Certification) -> String {
    match certification {
        Certification::NotEquivalent(counterexample) => counterexample.differing_bit.to_string(),
        _ => String::new(),
    }
}

fn counterexample(certification: &Certification) -> String {
    match certification {
        Certification::NotEquivalent(counterexample) => counterexample
            .values
            .iter()
            .enumerate()
            .map(|(variable, value)| format!("v{variable}={value:#x}"))
            .collect::<Vec<_>>()
            .join("|"),
        _ => String::new(),
    }
}

fn candidate_text(candidate: Option<&Expr>) -> String {
    candidate.map(ToString::to_string).unwrap_or_default()
}

fn main() {
    println!(
        "dataset,line,input_stage,input_ast_hash,serialized_ast_hash,\
serialized_roundtrip_same_ast,serialized_reparse_supported,input_nodes,variables,hidden_atoms,\
python_supported,rust_supported,unsupported_mul_nodes,python_candidate,\
rust_bitwise_candidate,rust_affine_candidate,same_candidate,\
python_candidate_verified_by_rust,rust_candidate_verified_by_python,\
verify_zero_residual,verify_expected,verify_rust_bitwise_candidate,\
verify_rust_affine_candidate,states,verify_expected_states,first_failure_bit,counterexample"
    );

    let mut count = 0;
    for row in CASES.lines().skip(1).filter(|row| !row.trim().is_empty()) {
        count += 1;
        let case = parse_case(row);
        assert_eq!(case.dataset, "loki_tiny.csv");
        let source = parse_expr(case.source).unwrap();
        let expected = parse_expr(case.expected).unwrap();
        let simplified_source = simplify_mba(source.clone(), WIDTH).unwrap();
        let simplified_expected = simplify_mba(expected.clone(), WIDTH).unwrap();
        let initial_residual = simplified_expected - simplified_source;
        let (diagnosed, trace) = diagnose_hidden_atoms(initial_residual, WIDTH).unwrap();
        let scope = trace
            .iter()
            .find(|scope| scope.input == diagnosed)
            .expect("missing top-level hidden-atom scope");
        let pipeline = experiment_pipeline(scope)
            .unwrap()
            .expect("missing P7e/P8 pipeline result");
        assert!(!pipeline.residual_zero);
        let residual = pipeline.result;
        assert_eq!(residual.to_string(), case.residual);
        let serialized_residual = parse_expr(case.residual).unwrap();
        let zero = Expr::zero();

        let verify_zero = certify_equivalent(&residual, &zero, WIDTH, P9Limits::default());
        let verify_serialized_zero =
            certify_equivalent(&serialized_residual, &zero, WIDTH, P9Limits::default());
        let verify_expected =
            certify_equivalent(&source, &expected, WIDTH, P9Limits::default());
        let bitwise = generate_bitwise_candidate(&residual, WIDTH);
        let affine = generate_affine_candidate(&residual, WIDTH);
        let verify_bitwise = bitwise.as_ref().map(|candidate| {
            certify_equivalent(&residual, candidate, WIDTH, P9Limits::default())
        });
        let verify_affine =
            certify_equivalent(&residual, &affine, WIDTH, P9Limits::default());
        let unsupported = mul_nodes(&residual);
        let rust_supported = !matches!(verify_zero, Certification::Unsupported);

        let fields = [
            csv_field(case.dataset),
            case.line.to_string(),
            csv_field("p7e_p8_diff_produced"),
            csv_field(ast_hash(&residual)),
            csv_field(ast_hash(&serialized_residual)),
            (serialized_residual == residual).to_string(),
            (!matches!(verify_serialized_zero, Certification::Unsupported)).to_string(),
            residual.size().to_string(),
            csv_field(variables(&residual)),
            csv_field(hidden_atoms(&residual)),
            csv_field("unavailable"),
            rust_supported.to_string(),
            csv_field(unsupported),
            csv_field("unavailable"),
            csv_field(candidate_text(bitwise.as_ref())),
            csv_field(affine.to_string()),
            csv_field("unavailable"),
            csv_field("unavailable"),
            csv_field("unavailable"),
            csv_field(certification_name(&verify_zero)),
            csv_field(certification_name(&verify_expected)),
            csv_field(
                verify_bitwise
                    .as_ref()
                    .map(certification_name)
                    .unwrap_or("NotGenerated"),
            ),
            csv_field(certification_name(&verify_affine)),
            states(&verify_zero).to_string(),
            states(&verify_expected).to_string(),
            first_failure_bit(&verify_zero),
            csv_field(counterexample(&verify_zero)),
        ];
        println!("{}", fields.join(","));
    }
    assert_eq!(count, 41);
}

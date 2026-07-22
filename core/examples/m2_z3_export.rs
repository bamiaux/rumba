use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    path::Path,
};

use rumba_core::{
    expr::{Expr, VarId},
    p9::{P9Limits, analyze_linear},
    p11a::{P11aLimits, compress_by_proof_key, trace_zero_without_p11},
    p11b::{P11bLimits, synthesize_carry_grammar},
    parser::parse_expr,
    simplify::{
        BaselineProofStage, diagnose_baseline_steps, diagnose_hidden_atoms,
        experiment_bitwise_dependency_closure,
    },
    varint::make_mask,
};

const WIDTH: u8 = 64;
const OUTPUT: &str = "artifacts/m2_z3";
const LOKI: &str = include_str!("../../third_party/dataset/loki_tiny.csv");
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const LOKI_LINES: [usize; 82] = [
    1841, 2319, 2499, 2631, 2729, 2772, 2929, 3312, 3424, 3894, 3919, 3928, 3979,
    4081, 4087, 4220, 4310, 4549, 4581, 4583, 4752, 4900, 4917, 7246, 7492, 8173,
    8341, 8584, 8982, 9594, 9810, 11431, 12529, 12689, 13078, 13174, 13238, 13752,
    14143, 14221, 14369, 14497, 14545, 16073, 17380, 17524, 17578, 17633, 17818,
    17825, 17972, 17980, 17981, 18104, 18200, 18437, 18523, 18631, 18685, 18731,
    19290, 19299, 19329, 19565, 19597, 19704, 19728, 19966, 19996, 22103, 23004,
    23488, 23652, 23810, 23816, 24044, 24286, 24338, 24490, 24796, 24869, 24932,
];
const QSYNTH_LINES: [usize; 19] = [
    13, 25, 53, 77, 114, 125, 134, 139, 210, 234, 249, 260, 294, 309, 369, 392,
    423, 481, 486,
];
const P11A_LINES: [usize; 12] = [13, 53, 77, 125, 134, 210, 234, 260, 294, 369, 481, 486];
const P11B_LINES: [usize; 4] = [25, 114, 139, 423];

struct HistoricalCase {
    dataset: &'static str,
    line: usize,
    source: Expr,
    expected: Expr,
}

struct Rewrite {
    dataset: &'static str,
    line: usize,
    retained_pass: &'static str,
    source: Expr,
    ordinary: Expr,
    after_p7e: Expr,
    retained_candidate: Expr,
    best_pass: &'static str,
    best_candidate: Expr,
}

fn load(dataset: &'static str, csv: &str, lines: &[usize]) -> Vec<HistoricalCase> {
    lines
        .iter()
        .map(|line| {
            let row = csv.lines().nth(line - 1).unwrap();
            let (source, expected) = row.split_once(',').unwrap();
            HistoricalCase {
                dataset,
                line: *line,
                source: parse_expr(source.trim()).unwrap(),
                expected: parse_expr(expected.trim()).unwrap(),
            }
        })
        .collect()
}

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

fn select_rewrite(case: HistoricalCase) -> Rewrite {
    let source = case.source;
    let ordinary = diagnose_hidden_atoms(source.clone(), WIDTH).unwrap().0;
    let expected = diagnose_hidden_atoms(case.expected, WIDTH).unwrap().0;
    let after_p7e = p7e_input(ordinary.clone());
    let p9l_base = analyze_linear(ordinary.clone(), WIDTH, P9Limits::default());
    let p9l_after_p7e = analyze_linear(after_p7e.clone(), WIDTH, P9Limits::default());
    let p7e_resolved = diagnose_hidden_atoms(expected - after_p7e.clone(), WIDTH)
        .is_ok_and(|(residual, _)| residual == Expr::zero());
    let (retained_pass, retained_candidate) = if p7e_resolved {
        ("p7e", after_p7e.clone())
    } else if p9l_after_p7e.changed {
        ("p9l", p9l_after_p7e.result.clone())
    } else if case.dataset == "qsynth_ea.csv" && P11A_LINES.contains(&case.line) {
        let analysis = compress_by_proof_key(after_p7e.clone(), WIDTH, P11aLimits::default());
        assert!(analysis.changed && analysis.proof_verified);
        ("p11a", analysis.result)
    } else if case.dataset == "qsynth_ea.csv" && P11B_LINES.contains(&case.line) {
        let analysis = synthesize_carry_grammar(after_p7e.clone(), WIDTH, P11bLimits::default());
        assert!(analysis.changed && analysis.proof_verified);
        ("p11b", analysis.result)
    } else {
        panic!(
            "{}:{} was not resolved by P7e, P9-L, P11a, or P11b",
            case.dataset, case.line
        );
    };
    let mut best = Vec::<(&'static str, Expr)>::new();
    if after_p7e.size() < ordinary.size() {
        best.push(("p7e", after_p7e.clone()));
    }
    if p9l_base.changed {
        best.push(("p9l_base", p9l_base.result));
    }
    if p9l_after_p7e.changed {
        best.push(("p9l_after_p7e", p9l_after_p7e.result));
    }
    best.push((retained_pass, retained_candidate.clone()));
    best.sort_by_key(|(_, candidate)| (candidate.size(), candidate.to_string()));
    let (best_pass, best_candidate) = best.into_iter().next().unwrap();
    Rewrite {
        dataset: case.dataset,
        line: case.line,
        retained_pass,
        source,
        ordinary,
        after_p7e,
        retained_candidate,
        best_pass,
        best_candidate,
    }
}

fn bv_constant(value: u64) -> String {
    format!("#x{value:016x}")
}

struct SmtDag {
    prefix: String,
    names: BTreeMap<Expr, String>,
    definitions: Vec<(String, String)>,
}

impl SmtDag {
    fn new(prefix: String) -> Self {
        Self {
            prefix,
            names: BTreeMap::new(),
            definitions: Vec::new(),
        }
    }

    fn fold(&mut self, operator: &str, identity: u64, terms: &[Expr]) -> String {
        let names = terms
            .iter()
            .map(|term| self.intern(term))
            .collect::<Vec<_>>();
        names.into_iter().fold(bv_constant(identity), |left, right| {
            format!("({operator} {left} {right})")
        })
    }

    fn intern(&mut self, expression: &Expr) -> String {
        if let Some(name) = self.names.get(expression) {
            return name.clone();
        }
        let mask = make_mask(WIDTH);
        let body = match expression {
            Expr::Var(variable) => return format!("{}_v{}", self.prefix, variable.0),
            Expr::Const(constant) => return bv_constant(constant.get(mask)),
            Expr::Not(inner) => {
                let inner = self.intern(inner);
                format!("(bvnot {inner})")
            }
            Expr::Scale(coefficient, inner) => {
                let inner = self.intern(inner);
                format!("(bvmul {} {inner})", bv_constant(coefficient.get(mask)))
            }
            Expr::And(terms) => self.fold("bvand", mask, terms),
            Expr::Or(terms) => self.fold("bvor", 0, terms),
            Expr::Xor(terms) => self.fold("bvxor", 0, terms),
            Expr::Add(terms) => self.fold("bvadd", 0, terms),
            Expr::Mul(terms) => self.fold("bvmul", 1, terms),
        };
        let name = format!("{}_n{}", self.prefix, self.definitions.len());
        self.names.insert(expression.clone(), name.clone());
        self.definitions.push((name.clone(), body));
        name
    }
}

fn safe_id(kind: &str, pass: &str, rewrite: &Rewrite) -> String {
    format!(
        "{kind}_{pass}_{}_{}",
        rewrite.dataset.replace(['.', '-'], "_"),
        rewrite.line
    )
}

fn append_query(output: &mut String, id: &str, left: &Expr, right: &Expr) {
    let variables = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .collect::<BTreeSet<VarId>>();
    writeln!(output, "(echo \"{id}\")").unwrap();
    writeln!(output, "(push)").unwrap();
    for variable in variables {
        writeln!(
            output,
            "(declare-fun {id}_v{} () (_ BitVec {WIDTH}))",
            variable.0
        )
        .unwrap();
    }
    let mut dag = SmtDag::new(id.to_owned());
    let left = dag.intern(left);
    let right = dag.intern(right);
    for (name, body) in dag.definitions {
        writeln!(
            output,
            "(define-fun {name} () (_ BitVec {WIDTH}) {body})"
        )
        .unwrap();
    }
    writeln!(
        output,
        "(assert (distinct {} {}))",
        left,
        right
    )
    .unwrap();
    writeln!(
        output,
        "(check-sat-using (par-or qfbv (then simplify bit-blast sat) (then simplify bit-blast psat)))"
    )
    .unwrap();
    writeln!(output, "(pop)").unwrap();
}

fn append_bit_queries(output: &mut String, id: &str, left: &Expr, right: &Expr) {
    let variables = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .collect::<BTreeSet<VarId>>();
    writeln!(output, "(push)").unwrap();
    for variable in variables {
        writeln!(
            output,
            "(declare-fun {id}_v{} () (_ BitVec {WIDTH}))",
            variable.0
        )
        .unwrap();
    }
    let mut dag = SmtDag::new(id.to_owned());
    let left = dag.intern(left);
    let right = dag.intern(right);
    for (name, body) in dag.definitions {
        writeln!(
            output,
            "(define-fun {name} () (_ BitVec {WIDTH}) {body})"
        )
        .unwrap();
    }
    for chunk in 0..4u8 {
        let low = chunk * 16;
        let high = low + 15;
        writeln!(output, "(echo \"{id}_bits{high}_{low}\")").unwrap();
        writeln!(output, "(push)").unwrap();
        writeln!(
            output,
            "(assert (distinct ((_ extract {high} {low}) {left}) ((_ extract {high} {low}) {right})))"
        )
        .unwrap();
        writeln!(
            output,
            "(check-sat-using (par-or qfbv (then simplify bit-blast sat) (then simplify bit-blast psat)))"
        )
        .unwrap();
        writeln!(output, "(pop)").unwrap();
    }
    writeln!(output, "(pop)").unwrap();
}

fn append_residual_connector_query(
    output: &mut String,
    id: &str,
    left: &Expr,
    right: &Expr,
    residual: &Expr,
) {
    let variables = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .chain(residual.get_vars())
        .collect::<BTreeSet<VarId>>();
    writeln!(output, "(echo \"{id}\")").unwrap();
    writeln!(output, "(push)").unwrap();
    for variable in variables {
        writeln!(
            output,
            "(declare-fun {id}_v{} () (_ BitVec {WIDTH}))",
            variable.0
        )
        .unwrap();
    }
    let mut dag = SmtDag::new(id.to_owned());
    let left = dag.intern(left);
    let right = dag.intern(right);
    let residual = dag.intern(residual);
    let zero = bv_constant(0);
    for (name, body) in dag.definitions {
        writeln!(
            output,
            "(define-fun {name} () (_ BitVec {WIDTH}) {body})"
        )
        .unwrap();
    }
    writeln!(
        output,
        "(assert (not (= (= {left} {right}) (= {residual} {zero}))))"
    )
    .unwrap();
    writeln!(
        output,
        "(check-sat-using (par-or qfbv (then simplify bit-blast sat) (then simplify bit-blast psat)))"
    )
    .unwrap();
    writeln!(output, "(pop)").unwrap();
}

fn escape_tsv(expression: &Expr) -> String {
    expression.to_string().replace(['\t', '\n', '\r'], " ")
}

fn canonical_expr(expression: &Expr) -> String {
    fn append_terms(output: &mut String, operator: &str, terms: &[Expr]) {
        write!(output, "({operator}").unwrap();
        for term in terms {
            output.push(' ');
            append(output, term);
        }
        output.push(')');
    }

    fn append(output: &mut String, expression: &Expr) {
        match expression {
            Expr::Var(variable) => write!(output, "(var {})", variable.0).unwrap(),
            Expr::Const(constant) => {
                write!(output, "(const {:016x})", constant.get(make_mask(WIDTH))).unwrap()
            }
            Expr::Not(inner) => {
                output.push_str("(not ");
                append(output, inner);
                output.push(')');
            }
            Expr::Scale(coefficient, inner) => {
                write!(
                    output,
                    "(scale {:016x} ",
                    coefficient.get(make_mask(WIDTH))
                )
                .unwrap();
                append(output, inner);
                output.push(')');
            }
            Expr::And(terms) => append_terms(output, "and", terms),
            Expr::Or(terms) => append_terms(output, "or", terms),
            Expr::Xor(terms) => append_terms(output, "xor", terms),
            Expr::Add(terms) => append_terms(output, "add", terms),
            Expr::Mul(terms) => append_terms(output, "mul", terms),
        }
    }

    let mut output = String::new();
    append(&mut output, expression);
    output
}

// Stable, dependency-free artifact identifier. Canonical ASTs are retained
// alongside it, so the hash is an index and never the sole proof identity.
fn fnv1a64(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn expr_hash(expression: &Expr) -> String {
    fnv1a64(&canonical_expr(expression))
}

fn pair_hash(left: &Expr, right: &Expr) -> String {
    let mut pair = [canonical_expr(left), canonical_expr(right)];
    pair.sort();
    fnv1a64(&format!("equiv\0{}\0{}", pair[0], pair[1]))
}

fn residual_hash(left: &Expr, right: &Expr) -> String {
    fnv1a64(&canonical_expr(&(left.clone() - right.clone())))
}

fn baseline_stage_name(stage: BaselineProofStage) -> &'static str {
    match stage {
        BaselineProofStage::Reduce => "reduce",
        BaselineProofStage::BitwiseFrontier => "frontier",
        BaselineProofStage::OrdinarySolve => "solve",
        BaselineProofStage::BestRetention => "retain",
    }
}

fn export_structured() {
    let comparison = fs::read_to_string(Path::new(OUTPUT).join("rewrite_comparison.tsv")).unwrap();
    let gate = fs::read_to_string(Path::new(OUTPUT).join("gate_cases.tsv")).unwrap();
    let unresolved = gate
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            (fields.get(6) == Some(&"UNKNOWN")).then(|| fields[3].to_owned())
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(unresolved.len(), 17);

    let header = "; M2d structured obligations for the mandatory UNKNOWN cases.\n(set-logic QF_BV)\n(set-option :timeout 5000)\n";
    let mut smt = String::from(header);
    let mut edge_dump = String::from(
        "dataset\tline\trequired_id\tstructured_id\tkind\tleft_hash\tright_hash\tpair_hash\tleft_nodes\tright_nodes\tleft\tright\n",
    );
    let mut case_dump = String::from(
        "dataset\tline\tretained_pass\trequired_id\tproof_outcome\tstructured_ids\n",
    );
    let mut obligation_count = 0usize;

    for line in comparison.lines().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let required_id = fields[4];
        if !unresolved.contains(required_id) {
            continue;
        }
        let dataset = fields[0];
        let line_number = fields[1].parse::<usize>().unwrap();
        let retained_pass = fields[2];
        let source = parse_expr(fields[18]).unwrap();
        let retained = parse_expr(fields[21]).unwrap();
        let dataset_id = dataset.replace(['.', '-'], "_");
        let mut ids = Vec::new();

        let (traced_ordinary, baseline_steps) =
            diagnose_baseline_steps(source.clone(), WIDTH).unwrap();
        let ordinary = traced_ordinary;
        let after_p7e = p7e_input(ordinary.clone());
        for (index, step) in baseline_steps.iter().enumerate() {
            let id = format!(
                "structured_ordinary_{index}_{}_p{}_{}_{}",
                baseline_stage_name(step.stage),
                step.pass,
                dataset_id,
                line_number
            );
            append_query(&mut smt, &id, &step.before, &step.after);
            writeln!(
                edge_dump,
                "{dataset}\t{line_number}\t{required_id}\t{id}\tordinary_{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                baseline_stage_name(step.stage),
                expr_hash(&step.before),
                expr_hash(&step.after),
                pair_hash(&step.before, &step.after),
                step.before.size(),
                step.after.size(),
                escape_tsv(&step.before),
                escape_tsv(&step.after),
            )
            .unwrap();
            ids.push(id);
            obligation_count += 1;
        }

        let (proof_left, proof_right) = if retained_pass == "p7e" {
            (&ordinary, &retained)
        } else {
            (&after_p7e, &retained)
        };
        let residual = proof_left.clone() - proof_right.clone();
        let trace = trace_zero_without_p11(residual.clone(), WIDTH, P9Limits::default());
        let connector_id = format!("structured_connector_{dataset_id}_{line_number}");
        append_residual_connector_query(
            &mut smt,
            &connector_id,
            proof_left,
            proof_right,
            &residual,
        );
        writeln!(
            edge_dump,
            "{dataset}\t{line_number}\t{required_id}\t{connector_id}\tresidual_connector\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            expr_hash(proof_left),
            expr_hash(proof_right),
            pair_hash(proof_left, proof_right),
            proof_left.size(),
            proof_right.size(),
            escape_tsv(proof_left),
            escape_tsv(proof_right),
        )
        .unwrap();
        ids.push(connector_id);
        obligation_count += 1;

        let (traced_residual, residual_steps) =
            diagnose_baseline_steps(residual.clone(), WIDTH).unwrap();
        assert_eq!(traced_residual, trace.ordinary);
        for (index, step) in residual_steps.iter().enumerate() {
            let id = format!(
                "structured_residual_{index}_{}_p{}_{}_{}",
                baseline_stage_name(step.stage),
                step.pass,
                dataset_id,
                line_number
            );
            append_query(&mut smt, &id, &step.before, &step.after);
            writeln!(
                edge_dump,
                "{dataset}\t{line_number}\t{required_id}\t{id}\tresidual_{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                baseline_stage_name(step.stage),
                expr_hash(&step.before),
                expr_hash(&step.after),
                pair_hash(&step.before, &step.after),
                step.before.size(),
                step.after.size(),
                escape_tsv(&step.before),
                escape_tsv(&step.after),
            )
            .unwrap();
            ids.push(id);
            obligation_count += 1;
        }
        if trace.after_p7e != trace.ordinary {
            let id = format!("structured_residual_p7e_{dataset_id}_{line_number}");
            append_query(&mut smt, &id, &trace.ordinary, &trace.after_p7e);
            writeln!(
                edge_dump,
                "{dataset}\t{line_number}\t{required_id}\t{id}\tresidual_p7e\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                expr_hash(&trace.ordinary),
                expr_hash(&trace.after_p7e),
                pair_hash(&trace.ordinary, &trace.after_p7e),
                trace.ordinary.size(),
                trace.after_p7e.size(),
                escape_tsv(&trace.ordinary),
                escape_tsv(&trace.after_p7e),
            )
            .unwrap();
            ids.push(id);
            obligation_count += 1;
        }
        if trace.after_p7e != Expr::zero() {
            let id = format!("structured_residual_zero_{dataset_id}_{line_number}");
            append_query(&mut smt, &id, &trace.after_p7e, &Expr::zero());
            writeln!(
                edge_dump,
                "{dataset}\t{line_number}\t{required_id}\t{id}\tresidual_zero\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                expr_hash(&trace.after_p7e),
                expr_hash(&Expr::zero()),
                pair_hash(&trace.after_p7e, &Expr::zero()),
                trace.after_p7e.size(),
                Expr::zero().size(),
                escape_tsv(&trace.after_p7e),
                escape_tsv(&Expr::zero()),
            )
            .unwrap();
            ids.push(id);
            obligation_count += 1;
        }
        writeln!(
            case_dump,
            "{dataset}\t{line_number}\t{retained_pass}\t{required_id}\t{:?}\t{}",
            trace.proof,
            ids.join(",")
        )
        .unwrap();
    }

    let output = Path::new(OUTPUT);
    fs::write(output.join("m2d_residual_obligations.smt2"), smt).unwrap();
    fs::write(output.join("m2d_residual_edges.tsv"), edge_dump).unwrap();
    fs::write(output.join("m2d_residual_cases.tsv"), case_dump).unwrap();
    fs::write(
        output.join("m2d_residual_export_summary.txt"),
        format!(
            "roadmap=M2d\nmandatory_unknown_cases={}\nstructured_obligations={obligation_count}\n",
            unresolved.len()
        ),
    )
    .unwrap();
    println!("roadmap=M2d");
    println!("mandatory_unknown_cases={}", unresolved.len());
    println!("structured_obligations={obligation_count}");
}

fn main() {
    if env::args().any(|argument| argument == "--structured-only") {
        export_structured();
        return;
    }
    let mut cases = load("loki_tiny.csv", LOKI, &LOKI_LINES);
    cases.extend(load("qsynth_ea.csv", QSYNTH, &QSYNTH_LINES));
    assert_eq!(cases.len(), 101);

    let rewrites = cases.into_iter().map(select_rewrite).collect::<Vec<_>>();
    let pass_count = |pass| {
        rewrites
            .iter()
            .filter(|rewrite| rewrite.retained_pass == pass)
            .count()
    };
    assert_eq!(pass_count("p7e"), 16);
    assert_eq!(pass_count("p9l"), 69);
    assert_eq!(pass_count("p11a"), 12);
    assert_eq!(pass_count("p11b"), 4);
    let best_smaller = rewrites
        .iter()
        .filter(|rewrite| rewrite.best_candidate.size() < rewrite.ordinary.size())
        .count();
    let best_equal = rewrites
        .iter()
        .filter(|rewrite| rewrite.best_candidate.size() == rewrite.ordinary.size())
        .count();
    let best_larger = rewrites.len() - best_smaller - best_equal;

    let output = Path::new(OUTPUT);
    fs::create_dir_all(output).unwrap();
    let full_header = "; Generated by m2_z3_export. Z3 is an external validation oracle.\n(set-logic QF_BV)\n(set-option :timeout 5000)\n";
    let bit_header = "; Exact per-bit fallback for direct UNKNOWN results.\n(set-logic QF_BV)\n(set-option :timeout 30000)\n";
    let mut required_smt = String::from(full_header);
    let mut diagnostic_smt = required_smt.clone();
    let mut local_smt = required_smt.clone();
    let mut required_bits_smt = String::from(bit_header);
    let mut diagnostic_bits_smt = required_bits_smt.clone();
    let mut local_bits_smt = required_bits_smt.clone();
    let mut cases_dump = String::from(
        "dataset\tline\tretained_pass\trequired_id\tedge_source_ordinary_id\tedge_ordinary_p7e_id\tedge_p7e_retained_id\tdiagnostic_id\tsource_hash\tordinary_hash\tafter_p7e_hash\tretained_hash\tbest_hash\trequired_pair_hash\trequired_residual_hash\tchain_hash\n",
    );
    let mut edges_dump = String::from(
        "dataset\tline\tedge_index\tedge_id\tfrom_stage\tto_stage\tleft_hash\tright_hash\tpair_hash\tresidual_hash\tleft_nodes\tright_nodes\tleft\tright\n",
    );
    let mut comparison_dump = String::from(
        "dataset\tline\tretained_pass\tbest_pass\trequired_id\tdiagnostic_id\tsource_nodes\tordinary_nodes\tafter_p7e_nodes\tretained_nodes\tbest_nodes\tsource_hash\tordinary_hash\tafter_p7e_hash\tretained_hash\tbest_hash\trequired_pair_hash\tdiagnostic_pair_hash\tsource\tordinary\tafter_p7e\tretained_candidate\tbest_candidate\n",
    );
    for rewrite in &rewrites {
        let required_id = safe_id("required", rewrite.retained_pass, rewrite);
        let diagnostic_id = safe_id("diagnostic", rewrite.best_pass, rewrite);
        let edge_ids = [
            safe_id("edge0", "source_ordinary", rewrite),
            safe_id("edge1", "ordinary_p7e", rewrite),
            safe_id("edge2", rewrite.retained_pass, rewrite),
        ];
        let edges = [
            ("source", "ordinary", &rewrite.source, &rewrite.ordinary),
            ("ordinary", "p7e", &rewrite.ordinary, &rewrite.after_p7e),
            (
                "p7e",
                "retained",
                &rewrite.after_p7e,
                &rewrite.retained_candidate,
            ),
        ];
        append_query(
            &mut required_smt,
            &required_id,
            &rewrite.source,
            &rewrite.retained_candidate,
        );
        append_query(
            &mut diagnostic_smt,
            &diagnostic_id,
            &rewrite.ordinary,
            &rewrite.best_candidate,
        );
        append_bit_queries(
            &mut required_bits_smt,
            &required_id,
            &rewrite.source,
            &rewrite.retained_candidate,
        );
        append_bit_queries(
            &mut diagnostic_bits_smt,
            &diagnostic_id,
            &rewrite.ordinary,
            &rewrite.best_candidate,
        );
        let mut chain_material = String::new();
        for (index, ((from_stage, to_stage, left, right), edge_id)) in
            edges.iter().zip(&edge_ids).enumerate()
        {
            append_query(&mut local_smt, edge_id, left, right);
            append_bit_queries(&mut local_bits_smt, edge_id, left, right);
            let edge_pair_hash = pair_hash(left, right);
            write!(chain_material, "{index}:{edge_pair_hash};").unwrap();
            writeln!(
                edges_dump,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                rewrite.dataset,
                rewrite.line,
                index,
                edge_id,
                from_stage,
                to_stage,
                expr_hash(left),
                expr_hash(right),
                edge_pair_hash,
                residual_hash(left, right),
                left.size(),
                right.size(),
                escape_tsv(left),
                escape_tsv(right),
            )
            .unwrap();
        }
        writeln!(
            cases_dump,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            rewrite.dataset,
            rewrite.line,
            rewrite.retained_pass,
            required_id,
            edge_ids[0],
            edge_ids[1],
            edge_ids[2],
            diagnostic_id,
            expr_hash(&rewrite.source),
            expr_hash(&rewrite.ordinary),
            expr_hash(&rewrite.after_p7e),
            expr_hash(&rewrite.retained_candidate),
            expr_hash(&rewrite.best_candidate),
            pair_hash(&rewrite.source, &rewrite.retained_candidate),
            residual_hash(&rewrite.source, &rewrite.retained_candidate),
            fnv1a64(&chain_material),
        )
        .unwrap();
        writeln!(
            comparison_dump,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            rewrite.dataset,
            rewrite.line,
            rewrite.retained_pass,
            rewrite.best_pass,
            required_id,
            diagnostic_id,
            rewrite.source.size(),
            rewrite.ordinary.size(),
            rewrite.after_p7e.size(),
            rewrite.retained_candidate.size(),
            rewrite.best_candidate.size(),
            expr_hash(&rewrite.source),
            expr_hash(&rewrite.ordinary),
            expr_hash(&rewrite.after_p7e),
            expr_hash(&rewrite.retained_candidate),
            expr_hash(&rewrite.best_candidate),
            pair_hash(&rewrite.source, &rewrite.retained_candidate),
            pair_hash(&rewrite.ordinary, &rewrite.best_candidate),
            escape_tsv(&rewrite.source),
            escape_tsv(&rewrite.ordinary),
            escape_tsv(&rewrite.after_p7e),
            escape_tsv(&rewrite.retained_candidate),
            escape_tsv(&rewrite.best_candidate),
        )
        .unwrap();
    }
    fs::write(output.join("required_rewrites.smt2"), required_smt).unwrap();
    fs::write(output.join("diagnostic_best.smt2"), diagnostic_smt).unwrap();
    fs::write(output.join("local_edges.smt2"), local_smt).unwrap();
    fs::write(output.join("local_edge_bits.smt2"), local_bits_smt).unwrap();
    fs::write(
        output.join("required_rewrite_bits.smt2"),
        required_bits_smt,
    )
    .unwrap();
    fs::write(
        output.join("diagnostic_best_bits.smt2"),
        diagnostic_bits_smt,
    )
    .unwrap();
    fs::write(output.join("cases.tsv"), cases_dump).unwrap();
    fs::write(output.join("local_edges.tsv"), edges_dump).unwrap();
    fs::write(output.join("rewrite_comparison.tsv"), comparison_dump).unwrap();
    fs::write(
        output.join("export_summary.txt"),
        format!(
            "roadmap=M2\nhistorical_cases=101\nrequired_rewrites=101\ndiagnostic_best_rewrites=101\nlocal_edges=303\navailable_slice_fallback_queries=2020\np7e=16\np9l=69\np11a=12\np11b=4\nbest_smaller={best_smaller}\nbest_equal={best_equal}\nbest_larger={best_larger}\nhash=fnv1a64_of_canonical_typed_ast\nexpected_used_for_candidate_generation=false\n",
        ),
    )
    .unwrap();
    println!("output={OUTPUT}");
    println!("roadmap=M2");
    println!("historical_cases={}", rewrites.len());
    println!("p7e={}", pass_count("p7e"));
    println!("p9l={}", pass_count("p9l"));
    println!("p11a={}", pass_count("p11a"));
    println!("p11b={}", pass_count("p11b"));
    println!("best_smaller={best_smaller}");
    println!("best_equal={best_equal}");
    println!("best_larger={best_larger}");
    println!("expected_used_for_candidate_generation=false");
}

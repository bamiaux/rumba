use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

use rumba_core::{
    expr::{Expr, VarId},
    p11a::{P11aLimits, compress_by_proof_key},
    p11b::{P11bLimits, synthesize_carry_grammar},
    parser::parse_expr,
    simplify::{
        diagnose_hidden_atoms, experiment_bitwise_dependency_closure,
    },
    varint::make_mask,
};

const WIDTH: u8 = 64;
const OUTPUT: &str = "artifacts/m2_z3";
const QSYNTH: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");
const DEFAULT_LINES: [usize; 17] = [
    13, 25, 53, 77, 125, 134, 139, 210, 234, 249, 260, 294, 369, 392, 423, 481,
    486,
];
const P7E_LINES: [usize; 2] = [249, 392];
const P11B_LINES: [usize; 3] = [25, 139, 423];

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

fn retained(line: usize, ordinary: Expr) -> Expr {
    let after_p7e = p7e_input(ordinary);
    if P7E_LINES.contains(&line) {
        after_p7e
    } else if P11B_LINES.contains(&line) {
        synthesize_carry_grammar(after_p7e, WIDTH, P11bLimits::default()).result
    } else {
        compress_by_proof_key(after_p7e, WIDTH, P11aLimits::default()).result
    }
}

#[derive(Clone)]
struct Lemma {
    source: Expr,
    candidate: Expr,
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
        writeln!(output, "(define-fun {name} () (_ BitVec {WIDTH}) {body})").unwrap();
    }
    writeln!(output, "(assert (distinct {left} {right}))").unwrap();
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
        writeln!(output, "(define-fun {name} () (_ BitVec {WIDTH}) {body})").unwrap();
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

fn escape_tsv(expression: &Expr) -> String {
    expression.to_string().replace(['\t', '\n', '\r'], " ")
}

fn rebuild_children(
    expression: Expr,
    cache: &mut BTreeMap<Expr, Expr>,
    lemmas: &mut BTreeMap<(Expr, Expr), Lemma>,
) -> Expr {
    match expression {
        Expr::Var(_) | Expr::Const(_) => expression,
        Expr::Not(inner) => Expr::Not(Box::new(simplify_structured(*inner, cache, lemmas))),
        Expr::Scale(coefficient, inner) => Expr::Scale(
            coefficient,
            Box::new(simplify_structured(*inner, cache, lemmas)),
        ),
        Expr::And(terms) => Expr::And(
            terms
                .into_iter()
                .map(|term| simplify_structured(term, cache, lemmas))
                .collect(),
        ),
        Expr::Or(terms) => Expr::Or(
            terms
                .into_iter()
                .map(|term| simplify_structured(term, cache, lemmas))
                .collect(),
        ),
        Expr::Xor(terms) => Expr::Xor(
            terms
                .into_iter()
                .map(|term| simplify_structured(term, cache, lemmas))
                .collect(),
        ),
        Expr::Add(terms) => Expr::Add(
            terms
                .into_iter()
                .map(|term| simplify_structured(term, cache, lemmas))
                .collect(),
        ),
        Expr::Mul(terms) => Expr::Mul(
            terms
                .into_iter()
                .map(|term| simplify_structured(term, cache, lemmas))
                .collect(),
        ),
    }
}

fn simplify_structured(
    expression: Expr,
    cache: &mut BTreeMap<Expr, Expr>,
    lemmas: &mut BTreeMap<(Expr, Expr), Lemma>,
) -> Expr {
    if let Some(result) = cache.get(&expression) {
        return result.clone();
    }
    let original = expression.clone();
    let with_children = rebuild_children(expression, cache, lemmas);
    let result = if with_children.size() <= 256 {
        diagnose_hidden_atoms(with_children.clone(), WIDTH)
            .ok()
            .map(|(candidate, _)| candidate)
            .filter(|candidate| {
                (candidate.size(), candidate.to_string())
                    < (with_children.size(), with_children.to_string())
            })
            .map_or_else(
                || with_children.clone(),
                |candidate| {
                    lemmas
                        .entry((with_children.clone(), candidate.clone()))
                        .or_insert_with(|| Lemma {
                            source: with_children.clone(),
                            candidate: candidate.clone(),
                        });
                    candidate
                },
            )
    } else {
        with_children
    };
    cache.insert(original, result.clone());
    result
}

fn main() {
    let requested = std::env::args()
        .skip(1)
        .map(|value| value.parse::<usize>().unwrap())
        .collect::<Vec<_>>();
    let lines = if requested.is_empty() {
        DEFAULT_LINES.as_slice()
    } else {
        requested.as_slice()
    };
    let header = "; M2d structured congruence obligations.\n(set-logic QF_BV)\n(set-option :timeout 5000)\n";
    let bit_header = "; M2d exact 16-bit slice fallback.\n(set-logic QF_BV)\n(set-option :timeout 30000)\n";
    let mut lemma_smt = String::from(header);
    let mut bridge_smt = String::from(header);
    let mut lemma_bits = String::from(bit_header);
    let mut bridge_bits = String::from(bit_header);
    let mut manifest = String::from(
        "dataset\tline\tbridge_id\tlemma_count\tlemma_ids\tsource_nf_nodes\tfinal_nf_nodes\tsource_nf\tfinal_nf\n",
    );
    let mut lemma_manifest = String::from(
        "dataset\tline\tlemma_id\tsource_nodes\tcandidate_nodes\tsource\tcandidate\n",
    );
    let mut total_lemmas = 0usize;
    for line in lines {
        let row = QSYNTH.lines().nth(line - 1).unwrap();
        let (source, _) = row.split_once(',').unwrap();
        let source = parse_expr(source.trim()).unwrap();
        let ordinary = diagnose_hidden_atoms(source.clone(), WIDTH).unwrap().0;
        let final_candidate = retained(*line, ordinary.clone());
        let mut cache = BTreeMap::new();
        let mut lemmas = BTreeMap::new();
        let structured = simplify_structured(source.clone(), &mut cache, &mut lemmas);
        let structured_final =
            simplify_structured(final_candidate.clone(), &mut cache, &mut lemmas);
        let variables = source.get_vars().into_iter().collect::<BTreeSet<_>>();
        let max_lemma_source = lemmas
            .values()
            .map(|lemma| lemma.source.size())
            .max()
            .unwrap_or(0);
        let max_lemma_candidate = lemmas
            .values()
            .map(|lemma| lemma.candidate.size())
            .max()
            .unwrap_or(0);
        let mut lemma_ids = Vec::new();
        for (index, lemma) in lemmas.values().enumerate() {
            let id = format!("structured_lemma_qsynth_ea_csv_{line}_{index}");
            append_query(&mut lemma_smt, &id, &lemma.source, &lemma.candidate);
            append_bit_queries(&mut lemma_bits, &id, &lemma.source, &lemma.candidate);
            writeln!(
                lemma_manifest,
                "qsynth_ea.csv\t{}\t{}\t{}\t{}\t{}\t{}",
                line,
                id,
                lemma.source.size(),
                lemma.candidate.size(),
                escape_tsv(&lemma.source),
                escape_tsv(&lemma.candidate),
            )
            .unwrap();
            lemma_ids.push(id);
        }
        total_lemmas += lemma_ids.len();
        let bridge_id = format!("structured_bridge_qsynth_ea_csv_{line}");
        append_query(
            &mut bridge_smt,
            &bridge_id,
            &structured,
            &structured_final,
        );
        append_bit_queries(
            &mut bridge_bits,
            &bridge_id,
            &structured,
            &structured_final,
        );
        writeln!(
            manifest,
            "qsynth_ea.csv\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            line,
            bridge_id,
            lemma_ids.len(),
            lemma_ids.join(","),
            structured.size(),
            structured_final.size(),
            escape_tsv(&structured),
            escape_tsv(&structured_final),
        )
        .unwrap();
        println!(
            "line={line} vars={} source={} structured={} ordinary={} final={} structured_final={} reached_ordinary={} structured_match={} lemmas={} max_lemma_source={} max_lemma_candidate={}",
            variables.len(),
            source.size(),
            structured.size(),
            ordinary.size(),
            final_candidate.size(),
            structured_final.size(),
            structured == ordinary,
            structured == structured_final,
            lemmas.len(),
            max_lemma_source,
            max_lemma_candidate,
        );
    }
    let output = Path::new(OUTPUT);
    fs::create_dir_all(output).unwrap();
    fs::write(output.join("structured_lemmas.smt2"), lemma_smt).unwrap();
    fs::write(output.join("structured_bridges.smt2"), bridge_smt).unwrap();
    fs::write(output.join("structured_lemma_bits.smt2"), lemma_bits).unwrap();
    fs::write(output.join("structured_bridge_bits.smt2"), bridge_bits).unwrap();
    fs::write(output.join("structured_cases.tsv"), manifest).unwrap();
    fs::write(output.join("structured_lemmas.tsv"), lemma_manifest).unwrap();
    fs::write(
        output.join("structured_export_summary.txt"),
        format!(
            "cases={}\nlemmas={total_lemmas}\nbridges={}\ncongruence=bottom_up\n",
            lines.len(),
            lines.len(),
        ),
    )
    .unwrap();
    println!("structured_cases={}", lines.len());
    println!("structured_lemmas={total_lemmas}");
}

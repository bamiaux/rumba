//! Temporary phase-three measurement harness; parse and cloning are untimed.
#[path = "support/corpus.rs"]
mod corpus;

use rumba_core::{expr::Expr, parser::parse_expr, simplify::simplify_mba};
use std::{fs, hint::black_box, io::Write, time::Instant};

fn bitwise(e: &Expr) -> bool {
    match e {
        Expr::Const(c) => *c == 0 || *c == u64::MAX,
        Expr::Var(_) => true,
        Expr::Not(e) => bitwise(e),
        Expr::And(es) | Expr::Or(es) | Expr::Xor(es) => es.iter().all(bitwise),
        _ => false,
    }
}

fn linear(e: &Expr) -> bool {
    let scaled = |e: &Expr| match e {
        Expr::Const(_) => true,
        Expr::Scale(_, e) => bitwise(e),
        _ => bitwise(e),
    };
    match e {
        Expr::Add(es) => es.iter().all(scaled),
        _ => scaled(e),
    }
}

fn cpu_ns() -> u64 {
    fs::read_to_string("/proc/self/schedstat")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "quality" {
        let (mut ok, mut okz, mut ng) = (0, 0, 0);
        for dataset in corpus::DATASETS {
            for row in corpus::rows(dataset.name, dataset.contents) {
                let mba = parse_expr(row.mba).unwrap();
                let truth = parse_expr(row.ground_truth).unwrap();
                let Ok(actual) = simplify_mba(mba.clone(), 64) else {
                    ng += 1;
                    continue;
                };
                assert!(actual.sem_equal(&truth, 64, 200).is_ok(), "{}", row.source);
                match simplify_mba(truth.clone(), 64) {
                    Ok(expected) if actual == expected => ok += 1,
                    Ok(_) => match simplify_mba((mba - truth).reduce(64), 64) {
                        Ok(residual) if residual == Expr::zero() => okz += 1,
                        _ => ng += 1,
                    },
                    Err(_) => ng += 1,
                }
            }
        }
        println!("OK={ok} OKZ={okz} NG={ng}");
        return;
    }
    let mut cases: Vec<_> = corpus::DATASETS
        .iter()
        .flat_map(|dataset| {
            corpus::rows(dataset.name, dataset.contents)
                .into_iter()
                .map(|row| {
                    let _ = row.ground_truth;
                    (row.source, parse_expr(row.mba).unwrap())
                })
        })
        .collect();
    if args[1] != "profile"
        && let Some(path) = args.get(3)
    {
        let ids = fs::read_to_string(path).unwrap();
        let ids: std::collections::HashSet<_> = ids.lines().collect();
        cases.retain(|(id, _)| ids.contains(id.as_str()));
    }
    match args[1].as_str() {
        "profile" => {
            use std::io::BufRead;
            for (_, e) in &cases {
                let _ = black_box(simplify_mba(e.clone(), 64));
            }
            let mut control = fs::OpenOptions::new().write(true).open(&args[2]).unwrap();
            let mut ack = std::io::BufReader::new(fs::File::open(&args[3]).unwrap());
            let mut response = String::new();
            writeln!(control, "enable").unwrap();
            ack.read_line(&mut response).unwrap();
            for (_, e) in cases {
                let _ = black_box(simplify_mba(e, 64));
            }
            writeln!(control, "disable").unwrap();
            response.clear();
            ack.read_line(&mut response).unwrap();
        }
        "cohort" => {
            for (id, e) in cases {
                if linear(&e.reduce(64)) {
                    println!("{id}");
                }
            }
        }
        "snapshot" => {
            let mut out = std::io::BufWriter::new(fs::File::create(&args[2]).unwrap());
            for (id, e) in cases {
                let result = simplify_mba(e, 64);
                let rendered = result.as_ref().map(|e| e.repr(64, false, false));
                writeln!(out, "{id}\t{result:?}\t{rendered:?}").unwrap();
            }
        }
        "once" => {
            for (_, e) in cases {
                let _ = black_box(simplify_mba(e, 64));
            }
        }
        "bench" => {
            let mut runs = Vec::new();
            let mut cpu_runs = Vec::new();
            for run in 0..6 {
                let inputs: Vec<Expr> = cases.iter().map(|(_, e)| e.clone()).collect();
                let cpu_start = cpu_ns();
                let start = Instant::now();
                for e in inputs {
                    let _ = black_box(simplify_mba(e, 64));
                }
                let elapsed = start.elapsed().as_secs_f64();
                let cpu = (cpu_ns() - cpu_start) as f64 / 1e9;
                println!(
                    "run={run} count={} seconds={elapsed:.9} cpu={cpu:.9}",
                    cases.len()
                );
                if run != 0 {
                    runs.push(elapsed);
                    cpu_runs.push(cpu);
                }
            }
            runs.sort_by(f64::total_cmp);
            println!("median={:.9}", runs[2]);
            cpu_runs.sort_by(f64::total_cmp);
            println!("cpu_median={:.9}", cpu_runs[2]);
        }
        _ => panic!("expected snapshot PATH, once, or bench [unused COHORT]"),
    }
}

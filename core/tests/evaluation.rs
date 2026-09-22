// Exercise the native batched evaluator, including large tables. The optional
// JIT has its own tests in src/jit.rs and is outside this optimization's scope.
#![cfg(not(feature = "jit"))]

use rand::{Rng, SeedableRng, rngs::StdRng};
use rumba_core::expr::Expr;

// Independent scalar oracle: ordinary wrapping u64 operations, no VarInt or
// production interpreter. Include empty operators and unreduced expressions.
fn scalar(e: &Expr, vars: &[u64]) -> u64 {
    match e {
        Expr::Var(v) => vars[v.0],
        Expr::Const(c) => *c,
        Expr::Not(e) => !scalar(e, vars),
        Expr::Scale(c, e) => c.wrapping_mul(scalar(e, vars)),
        Expr::And(es) => es.iter().fold(u64::MAX, |a, e| a & scalar(e, vars)),
        Expr::Or(es) => es.iter().fold(0, |a, e| a | scalar(e, vars)),
        Expr::Xor(es) => es.iter().fold(0, |a, e| a ^ scalar(e, vars)),
        Expr::Add(es) => es.iter().fold(0u64, |a, e| a.wrapping_add(scalar(e, vars))),
        Expr::Mul(es) => es.iter().fold(1u64, |a, e| a.wrapping_mul(scalar(e, vars))),
    }
}

fn expression(rng: &mut StdRng, depth: usize, variables: usize) -> Expr {
    if depth == 0 {
        return if variables != 0 && rng.random() {
            Expr::Var(rng.random_range(0..variables).into())
        } else {
            Expr::Const(rng.random())
        };
    }
    match rng.random_range(0..7) {
        0 => !expression(rng, depth - 1, variables),
        1 => rng.random::<u64>() * expression(rng, depth - 1, variables),
        operator => {
            let children = (0..rng.random_range(0..=3))
                .map(|_| expression(rng, depth - 1, variables))
                .collect();
            [Expr::And, Expr::Or, Expr::Xor, Expr::Add, Expr::Mul][operator - 2](children)
        }
    }
}

#[test]
fn generated_evaluations_match_scalar_oracle() {
    let mut rng = StdRng::seed_from_u64(0x006c_616e_6573);
    for variables in 0..=12 {
        for _ in 0..12 {
            let e = expression(&mut rng, 3, variables);
            let expected: Vec<_> = (0..1 << variables)
                .map(|assignment| {
                    let vars: Vec<_> = (0..variables).map(|bit| (assignment >> bit) & 1).collect();
                    scalar(&e, &vars)
                })
                .collect();
            for n in [0, 1, 2, 7, 8, 16, 31, 32, 63, 64] {
                let mask = if n == 64 { u64::MAX } else { (1 << n) - 1 };
                assert_eq!(
                    e.truth_table(variables, n),
                    expected.iter().map(|v| v & mask).collect::<Vec<_>>()
                );
                let vars: Vec<u64> = (0..variables).map(|_| rng.random()).collect();
                assert_eq!(e.eval(&vars, n), scalar(&e, &vars) & mask);
                if n != 0 {
                    assert_eq!(
                        scalar(&e.clone().reduce(n), &vars) & mask,
                        scalar(&e, &vars) & mask
                    );
                }
            }
        }
    }
}

#[test]
fn empty_operators_have_their_identity_in_large_truth_tables() {
    for operator in [Expr::And, Expr::Or, Expr::Xor, Expr::Add, Expr::Mul] {
        let e = operator(Vec::new());
        assert_eq!(e.truth_table(11, 64), vec![scalar(&e, &[]); 1 << 11]);
    }
}

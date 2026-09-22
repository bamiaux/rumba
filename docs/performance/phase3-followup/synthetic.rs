//! Build as a rumba-core example in each isolated checkout, then compare stdout.
use rand::{Rng, SeedableRng, rngs::StdRng};
use rumba_core::{expr::Expr, simplify::simplify_mba};

fn expression(rng: &mut StdRng, depth: usize) -> Expr {
    if depth == 0 {
        return if rng.random_range(0..4) == 0 {
            Expr::Const(rng.random())
        } else {
            Expr::Var(rng.random_range(0..4).into())
        };
    }
    let a = expression(rng, depth - 1);
    let b = expression(rng, depth - 1);
    match rng.random_range(0..8) {
        0 => !a,
        1 => a & b,
        2 => a | b,
        3 => a ^ b,
        4 => a + b,
        5 => a * b,
        6 => Expr::Scale(
            [0, 1, 2, u64::MAX, rng.random()][rng.random_range(0..5)],
            Box::new(a),
        ),
        _ => Expr::Add(vec![a.clone(), b, a]),
    }
}

fn main() {
    let mut rng = StdRng::seed_from_u64(0x52554d4241);
    for n in 0..=64 {
        for i in 0..512 {
            let e = expression(&mut rng, 3);
            println!("reduce {n} {i}: {:?}", e.reduce(n));
        }
    }
    let mut rng = StdRng::seed_from_u64(0x52554d4241);
    for n in [1, 2, 3, 8, 16, 32, 64] {
        for i in 0..128 {
            let e = expression(&mut rng, 3);
            let result = simplify_mba(e, n);
            let rendered = result.as_ref().map(|e| e.repr(n, false, false));
            println!("solve {n} {i}: {result:?} {rendered:?}");
        }
    }
}

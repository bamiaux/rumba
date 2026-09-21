#[cfg(test)]
mod phase3_packed_tests {
    use super::*;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    fn boolean(rng: &mut StdRng, q: usize, depth: usize) -> Expr {
        if depth == 0 || rng.random_range(0..4) == 0 {
            return if q == 0 || rng.random_range(0..4) == 0 {
                Expr::Const(if rng.random_range(0..2) == 0 { 0 } else { u64::MAX })
            } else { Expr::Var(rng.random_range(0..q).into()) };
        }
        let a = boolean(rng, q, depth - 1);
        let b = boolean(rng, q, depth - 1);
        match rng.random_range(0..4) { 0 => !a, 1 => a & b, 2 => a | b, _ => a ^ b }
    }

    #[test]
    fn packed_truth_and_coefficients_all_word_widths() {
        let mut rng = StdRng::seed_from_u64(0x52554d4241);
        let cache = LocalCache::new();
        for n in 1..=64 {
            for q in 0..=6 {
                for _ in 0..32 {
                    let a = boolean(&mut rng, q, 4);
                    let b = boolean(&mut rng, q, 4);
                    let e = Expr::Add(vec![
                        Expr::Const(rng.random()),
                        rng.random::<u64>() * a,
                        rng.random::<u64>() * Expr::Mul(vec![b]),
                    ]);
                    let solver = MBASolver::new(&cache, &e, n);
                    let packed = solver.packed_signature(&e, q).unwrap();
                    let scalar = solver.calc_signature(&e, q);
                    assert_eq!(packed, scalar, "n={n} q={q} {e:?}");
                    assert_eq!(solver.make_conjunction_sum(packed, q), solver.reference_conjunction_sum(scalar, q));
                }
            }
        }
    }

    #[test]
    fn packed_rejects_non_bitwise_arithmetic_and_partial_masks() {
        let cache = LocalCache::new();
        let x = Expr::Var(0.into());
        let solver = MBASolver::new(&cache, &x, 64);
        for e in [x.clone() * x.clone(), !(x.clone() + x.clone()), x & Expr::Const(7)] {
            assert!(solver.packed_signature(&e, 1).is_none(), "{e:?}");
        }
    }
}

#[cfg(test)]
mod phase3_differential_tests {
    use super::*;
    use rand::{Rng, SeedableRng, rngs::StdRng};
    fn expression(rng: &mut StdRng, depth: usize) -> Expr {
        if depth == 0 {
            return if rng.random_range(0..4) == 0 { Expr::Const(rng.random()) }
                else { Expr::Var(rng.random_range(0..4).into()) };
        }
        let a = expression(rng, depth - 1);
        let b = expression(rng, depth - 1);
        match rng.random_range(0..8) {
            0 => !a, 1 => a & b, 2 => a | b, 3 => a ^ b, 4 => a + b, 5 => a * b,
            6 => Expr::Scale([0, 1, 2, u64::MAX, rng.random()][rng.random_range(0..5)], Box::new(a)),
            _ => Expr::Add(vec![a.clone(), b, a]),
        }
    }
    #[test]
    fn reducer_matches_reference_on_arbitrary_trees_and_all_masks() {
        let mut rng = StdRng::seed_from_u64(0x52554d4241);
        for n in 0..=64 {
            for _ in 0..512 {
                let e = expression(&mut rng, 3);
                assert_eq!(e.clone().reduce(n), crate::phase3_reference_reduce::reduce(e.clone(), n), "n={n}, e={e:?}");
            }
        }
    }
}

use super::{Expr, VarId};

#[test]
fn variables_include_sparse_ids_under_every_operator() {
    let leaf = Expr::Var(VarId(usize::MAX));
    let e = Expr::Add(vec![
        Expr::Const(42),
        !leaf.clone(),
        Expr::Scale(0, Box::new(Expr::Var(VarId(17)))),
        Expr::And(vec![leaf.clone()]),
        Expr::Or(vec![leaf.clone()]),
        Expr::Xor(vec![leaf.clone()]),
        Expr::Mul(vec![leaf]),
    ]);
    assert_eq!(
        e.get_vars(),
        [VarId(17), VarId(usize::MAX)].into_iter().collect()
    );
    assert!(Expr::Const(42).get_vars().is_empty());
    assert!(Expr::Add(vec![]).get_vars().is_empty());
}

#[test]
fn wide_truth_tables_match_scalar_evaluation_across_masks_and_dispatch_thresholds() {
    for t in [0, 1, 2, 7, 8, 10, 11, 14] {
        let x = if t == 0 {
            Expr::Const(1)
        } else {
            Expr::Var(VarId(0))
        };
        let y = if t < 2 {
            Expr::Const(u64::MAX)
        } else {
            Expr::Var(VarId(t - 1))
        };
        let z = if t < 3 {
            Expr::Const(17)
        } else {
            Expr::Var(VarId(t / 2))
        };
        let expression = Expr::Add(vec![
            Expr::Scale(u64::MAX, Box::new(!(x.clone() ^ y.clone()))),
            Expr::Mul(vec![x.clone() + Expr::Const(u64::MAX), y.clone() | z]),
            x & y,
            Expr::Const(u64::MAX),
        ]);
        for n in [0, 1, 7, 32, 64] {
            let table = expression.truth_table(t, n);
            let mut vars = vec![0; t];
            for (assignment, value) in table.into_iter().enumerate() {
                for (j, v) in vars.iter_mut().enumerate() {
                    *v = ((assignment >> j) & 1) as u64;
                }
                assert_eq!(
                    value,
                    expression.eval(&vars, n),
                    "t={t} n={n} assignment={assignment}"
                );
            }
        }
    }
    for expression in [
        Expr::And(vec![]),
        Expr::Or(vec![]),
        Expr::Xor(vec![]),
        Expr::Add(vec![]),
        Expr::Mul(vec![]),
    ] {
        assert_eq!(
            expression.truth_table(8, 64),
            vec![expression.eval(&[0; 8], 64); 256]
        );
    }
}

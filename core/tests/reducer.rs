use rumba_core::expr::Expr;

fn var(index: usize) -> Expr {
    Expr::Var(index.into())
}

fn and(lhs: Expr, rhs: Expr) -> Expr {
    lhs & rhs
}

fn or(lhs: Expr, rhs: Expr) -> Expr {
    lhs | rhs
}

fn xor(lhs: Expr, rhs: Expr) -> Expr {
    lhs ^ rhs
}

fn add(lhs: Expr, rhs: Expr) -> Expr {
    lhs + rhs
}

fn mul(lhs: Expr, rhs: Expr) -> Expr {
    lhs * rhs
}

fn left_associated(op: fn(Expr, Expr) -> Expr, terms: &[Expr]) -> Expr {
    terms
        .iter()
        .cloned()
        .reduce(op)
        .expect("AC test needs at least one term")
}

#[test]
fn associative_commutative_reductions_ignore_construction_order() {
    let operators = [and, or, xor, add, mul];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let terms = [var(0), var(1), var(2)];

    for op in operators {
        let expected = left_associated(op, &terms).reduce(64);
        for permutation in permutations {
            let reordered: Vec<_> = permutation
                .iter()
                .map(|&index| terms[index].clone())
                .collect();
            assert_eq!(
                left_associated(op, &reordered).reduce(64),
                expected,
                "operator is not permutation-independent"
            );
        }

        let right_associated = op(terms[0].clone(), op(terms[1].clone(), terms[2].clone()));
        assert_eq!(right_associated.reduce(64), expected);
    }
}

#[test]
fn xor_cancellation_is_construction_order_independent() {
    let x = var(0);
    let y = var(1);
    let z = var(2);
    let expected = y.clone();
    let forms = [
        (x.clone() ^ y.clone()) ^ x.clone(),
        x.clone() ^ (y.clone() ^ x.clone()),
        (y.clone() ^ x.clone()) ^ x.clone(),
        y.clone() ^ x.clone() ^ x.clone(),
        x.clone() ^ z.clone() ^ y.clone() ^ x.clone() ^ z.clone(),
    ];

    for form in forms {
        assert_eq!(form.reduce(64), expected);
    }
}

#[test]
fn de_morgan_outputs_are_ac_canonical() {
    let x = var(0);
    let y = var(1);
    let z = var(2);

    assert_eq!(
        (!(z.clone() & x.clone() & y.clone())).reduce(64),
        ((!z.clone()) | (!x.clone()) | (!y.clone())).reduce(64)
    );
    assert_eq!(
        (!(z | x | y)).reduce(64),
        ((!var(2)) & (!var(0)) & (!var(1))).reduce(64)
    );
}

#[test]
fn generated_sums_and_xors_preserve_coefficients_and_parity() {
    use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};

    let mut rng = StdRng::seed_from_u64(0x7265_6475_6365);
    let bases = [
        var(0),
        var(31),
        var(511),
        var(0) & var(31),
        var(31) * var(511),
    ];
    for n in 1..=64 {
        let mask = u64::MAX >> (64 - n);
        for _ in 0..40 {
            let mut sums = Vec::new();
            let mut xors = Vec::new();
            let mut coefficients = [0u64; 5];
            let mut parity = [false; 5];
            for _ in 0..rng.random_range(0..=40) {
                let i = rng.random_range(0..bases.len());
                let coefficient = [0, 1, mask, rng.random()][rng.random_range(0..4)];
                sums.push(coefficient * bases[i].clone());
                coefficients[i] = coefficients[i].wrapping_add(coefficient) & mask;
                xors.push(bases[i].clone());
                parity[i] ^= true;
            }
            let expected_sum = Expr::Add(
                bases
                    .iter()
                    .zip(coefficients)
                    .map(|(base, c)| c * base.clone())
                    .collect(),
            )
            .reduce(n);
            let expected_xor = Expr::Xor(
                bases
                    .iter()
                    .zip(parity)
                    .filter(|(_, odd)| *odd)
                    .map(|(base, _)| base.clone())
                    .collect(),
            )
            .reduce(n);
            for _ in 0..3 {
                sums.shuffle(&mut rng);
                xors.shuffle(&mut rng);
                assert_eq!(Expr::Add(sums.clone()).reduce(n), expected_sum);
                assert_eq!(Expr::Xor(xors.clone()).reduce(n), expected_xor);
            }
        }
    }
}

#[test]
fn zero_width_sum_discards_implicit_unit_coefficients() {
    assert_eq!((var(0) + var(1)).reduce(0), Expr::zero());
}

#[test]
fn variable_traversal_handles_sparse_ids_and_every_operator() {
    use std::collections::BTreeSet;
    let expression = Expr::Add(vec![
        !var(29),
        17 * var(1001),
        var(0) & var(29),
        var(77) | var(0),
        var(77) ^ var(1001),
        var(1001) * var(29),
        Expr::Const(123),
        Expr::And(vec![]),
    ]);
    assert_eq!(
        expression
            .get_vars()
            .into_iter()
            .map(|v| v.0)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 29, 77, 1001])
    );
    assert!(Expr::Const(123).get_vars().is_empty());
}

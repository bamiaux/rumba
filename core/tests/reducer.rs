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

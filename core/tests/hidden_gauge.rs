#![cfg(feature = "parse")]

use rumba_core::{expr::Expr, parser::parse_expr, simplify};

const BIT_COUNT: u8 = 64;

fn assert_equivalent(source: &str, assignments: &[&[u64]], source_variables: usize) {
    let expression = parse_expr(source).expect("source expression must parse");
    let simplified = simplify::simplify_mba(expression.clone(), BIT_COUNT)
        .expect("hidden-gauge expression must simplify");

    assert!(
        simplified
            .get_vars()
            .iter()
            .all(|variable| variable.0 < source_variables),
        "simplified expression leaked an internal coordinate: {simplified}"
    );

    for values in assignments {
        assert_eq!(
            expression.eval(values, BIT_COUNT),
            simplified.eval(values, BIT_COUNT),
            "simplification changed {source} for {values:?}: {simplified}"
        );
    }
}

#[test]
fn hidden_definitions_use_word_width_after_subwidth_simplification() {
    let values = [
        &[0, 0][..],
        &[1, 2][..],
        &[15, 16][..],
        &[16, 15][..],
        &[0x1234, u64::MAX][..],
        &[u64::MAX, 0x8000_0000_0000_0000][..],
    ];

    assert_equivalent("(v0 + v1) & 15", &values, 2);
    assert_equivalent("v0 & 15", &values, 2);
    assert_equivalent("v0 | 15", &values, 2);
    assert_equivalent("v0 ^ 15", &values, 2);
}

#[test]
fn recursively_nested_hidden_definitions_are_restored() {
    let values = [
        &[0, 0][..],
        &[1, 2][..],
        &[15, 16][..],
        &[0x1234, u64::MAX][..],
        &[u64::MAX, 0x8000_0000_0000_0000][..],
    ];

    assert_equivalent("(v0 * (v0 | 2)) ^ 1", &values, 1);
}

#[test]
fn nested_hidden_construction_routes_share_the_same_result() {
    let sources = [
        "(v0 * (v0 | 2)) * ((v0 * (v0 | 2)) | 3)",
        "((v0 * (v0 | 2)) | 3) * (v0 * (v0 | 2))",
        "(v0 * (v0 | 2)) * ((v0 * (2 | v0)) | 3)",
    ];
    let values = [
        &[0][..],
        &[1][..],
        &[2][..],
        &[15][..],
        &[0x1234][..],
        &[u64::MAX][..],
    ];

    let expected = simplify::simplify_mba(
        parse_expr(sources[0]).expect("nested hidden expression must parse"),
        BIT_COUNT,
    )
    .expect("nested hidden expression must simplify");
    for source in &sources {
        let expression = parse_expr(source).expect("nested hidden expression must parse");
        let simplified = simplify::simplify_mba(expression.clone(), BIT_COUNT)
            .expect("nested hidden expression must simplify");
        assert_eq!(simplified, expected, "construction route changed {source}");
        for assignments in values {
            assert_eq!(
                expression.eval(assignments, BIT_COUNT),
                simplified.eval(assignments, BIT_COUNT),
                "simplification changed {source}"
            );
        }
    }
}

#[test]
fn nested_hidden_routes_are_equivalent_exhaustively_at_small_widths() {
    let sources = [
        "(v0 * (v0 | 2)) * ((v0 * (v0 | 2)) | 3)",
        "(v0 * (v0 | 2)) * ((v0 * (2 | v0)) | 3)",
        "((v0 * (v0 | 2)) | 3) * (v0 * (v0 | 2))",
    ];

    for width in 1..=4 {
        let mask = (1u64 << width) - 1;
        for source in sources {
            let expression = parse_expr(source).expect("nested hidden expression must parse");
            let simplified = simplify::simplify_mba(expression.clone(), width)
                .expect("nested hidden expression must simplify");
            for value in 0..=mask {
                assert_eq!(
                    expression.eval(&[value], width),
                    simplified.eval(&[value], width),
                    "nested hidden simplification changed {source} at width={width}, value={value}"
                );
            }
        }
    }
}

#[test]
fn complementary_hidden_orbits_close_without_extra_coordinates() {
    let source = "-v3 \
        + (v3 & (-1 - v2*v3 - v3*v3 - v3*(v2 & (-1-v2-v3)))) \
        + (v3 & (2*v2*v3 - v3*(v2 & (v2+v3)) + v3*v3))";
    let expression = parse_expr(source).expect("source expression must parse");
    let simplified = simplify::simplify_mba(expression, BIT_COUNT)
        .expect("complementary hidden orbits must simplify");

    assert_eq!(simplified, Expr::zero());
}

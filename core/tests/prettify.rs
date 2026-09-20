#![cfg(feature = "parse")]

use rumba_core::{parser::parse_expr, simplify};

#[derive(Clone, Copy)]
struct PrettifyCase {
    width: u8,
    source: &'static str,
    expected: &'static str,
}

fn case(width: u8, source: &'static str, expected: &'static str) -> PrettifyCase {
    PrettifyCase {
        width,
        source,
        expected,
    }
}

fn assert_cases(cases: impl IntoIterator<Item = PrettifyCase>) {
    for case in cases {
        let source = parse_expr(case.source).expect("source expression must parse");
        let expected = parse_expr(case.expected).expect("expected expression must parse");
        let actual = simplify::simplify_mba(source, case.width)
            .expect("simplification must succeed")
            .to_string();
        assert_eq!(actual, expected.to_string(), "source: {}", case.source);
    }
}

#[test]
fn prettifies_basic_forms_from_a_table() {
    assert_cases([
        // or
        case(32, "v0 + v1 + 0xffffffffffffffff*(v0&v1)", "v0|v1"),
        // xor
        case(32, "v0 + v1 + 0xfffffffffffffffe*(v0&v1)", "v0^v1"),
        // not
        case(32, "0xffffffffffffffff + 0xffffffffffffffff*v0", "~v0"),
        // or on eight bits
        case(8, "v0 + v1 + 0xff*(v0&v1)", "v0|v1"),
        // xor on eight bits
        case(8, "v0 + v1 + 0xfe*(v0&v1)", "v0^v1"),
        // narrow coefficient on a wider word is unchanged
        case(32, "v0 + v1 + 0xff*(v0&v1)", "v0 + v1 + 0xff*(v0&v1)"),
        // nested output
        case(
            32,
            "(v0 + v1 + 0xffffffffffffffff*(v0&v1))&v2",
            "v2&(v0|v1)",
        ),
        // non-matching coefficient is unchanged
        case(
            32,
            "v0 + v1 + 0xfffffffd*(v0&v1)",
            "v0 + v1 + 0xfffffffd*(v0&v1)",
        ),
        // nested constructors are flattened
        case(64, "(v0&v1)&v2", "v0&v1&v2"),
        // and of complements uses De Morgan
        case(8, "~v0&~v1", "~(v0|v1)"),
        // or of complements uses De Morgan
        case(8, "~v0|~v1", "~(v0&v1)"),
        // xor complements cancel
        case(8, "~v0^~v1", "v0^v1"),
    ]);
}

#[test]
fn prettifies_added_forms_from_a_table() {
    assert_cases([
        // scaled or
        case(8, "3*v0 + 3*v1 + 253*(v0&v1)", "3*(v0|v1)"),
        // scaled xor
        case(8, "3*v0 + 3*v1 + 250*(v0&v1)", "3*(v0^v1)"),
        // scaled tuple inside a larger sum
        case(8, "3*v0 + 3*v1 + 253*(v0&v1) + v2", "v2 + 3*(v0|v1)"),
        // shared context with or
        case(64, "(v2&v0) + (v2&v1) - (v2&v0&v1)", "v2&(v0|v1)"),
        // shared context with xor
        case(64, "(v2&v0) + (v2&v1) - 2*(v2&v0&v1)", "v2&(v0^v1)"),
        // grouped conjunctions
        case(
            64,
            "(v0&v1) + (v2&v3) - ((v0&v1)&(v2&v3))",
            "(v0&v1)|(v2&v3)",
        ),
        // multiple tuples are reentered
        case(
            64,
            "(v0 + v1 - (v0&v1)) + (v2 + v3 - 2*(v2&v3))",
            "(v0|v1) + (v2^v3)",
        ),
        // shared context is retained in the output
        case(
            64,
            "(v2&v3&v0) + (v2&v3&v1) - (v2&v3&v0&v1)",
            "v2&v3&(v0|v1)",
        ),
        // complement pair inside a larger sum
        case(
            64,
            "v2 + (0xffffffffffffffff + 0xffffffffffffffff*v0)",
            "v2 + ~v0",
        ),
        // scaled difference
        case(8, "3*v0 + 253*(v0&v1)", "3*(v0&~v1)"),
        // scaled complement
        case(8, "3 + 3*v0", "253*~v0"),
        // complement chooses the smaller De Morgan form
        case(8, "3 + 3*(~v0&v1)", "253*(~v1|v0)"),
    ]);
}

#[test]
fn prettified_outputs_preserve_boolean_identities() {
    let cases = [
        ("3*v0 + 3*v1 + 13*(v0&v1)", "3*(v0|v1)"),
        ("3*v0 + 3*v1 + 10*(v0&v1)", "3*(v0^v1)"),
        ("3*v0 + 13*(v0&v1)", "3*(v0&~v1)"),
        ("3 + 3*v0", "253*~v0"),
        ("~v0|~v1", "~(v0&v1)"),
        ("~v0^~v1", "v0^v1"),
    ];

    for (source, expected) in cases {
        let source = parse_expr(source).expect("source expression must parse");
        let expected = parse_expr(expected).expect("expected expression must parse");
        for x_value in 0..16 {
            for y_value in 0..16 {
                assert_eq!(
                    source.eval(&[x_value, y_value], 4),
                    expected.eval(&[x_value, y_value], 4),
                    "identity failed for x={x_value}, y={y_value}: {source}"
                );
            }
        }
    }
}

#[test]
fn prettification_preserves_semantics_through_the_solver() {
    let cases = ["v0 | v1", "v0 ^ v1", "~v0", "~(v0 & v1)"];

    for source in cases {
        let expression = parse_expr(source).expect("source expression must parse");
        let simplified = simplify::simplify_mba(expression.clone(), 32).unwrap();
        assert!(
            expression.sem_equal(&simplified, 32, 1000).is_ok(),
            "{source} became {simplified}"
        );
    }
}

#![cfg(feature = "parse")]

use rumba_core::{expr::Expr, parser::parse_expr, simplify};

const BIT_COUNT: u8 = 64;

#[test]
fn projector_defect_equivalence_is_exhaustive_at_small_widths() {
    for width in 1..=4 {
        let mask = if width == 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        for observer in 0..=mask {
            for lhs in 0..=mask {
                for rhs in 0..=mask {
                    let defect = lhs ^ rhs;
                    let observed = observer & defect == 0;
                    let empty_observer = 0u64;

                    assert_eq!(observer & (lhs ^ lhs), 0);
                    assert_eq!(empty_observer & defect, 0);
                    assert_eq!(mask & defect == 0, lhs == rhs);
                    assert_eq!(observed, (observer & lhs) == (observer & rhs));

                    if observed {
                        for context in 0..=mask {
                            assert_eq!(context & observer & lhs, context & observer & rhs);
                        }
                    }
                }
            }
        }
    }
}

fn assert_zero(source: &str) {
    let expression = parse_expr(source).expect("source expression must parse");
    assert_eq!(
        simplify::simplify_mba(expression, BIT_COUNT),
        Ok(Expr::zero()),
        "Projector–Defect regression failed for {source}"
    );
}

#[test]
fn predecessor_and_order_certificates_rewrite_source_contexts() {
    // Valuation/predecessor relation under a non-empty AND context.
    assert_zero(
        "-v3 - (v3 & 2*v4 & (v3+v4)) + (v3 & 2*v4) + (v3 & (v3+v4)) \
         + (v3 & (-1 - 3*v4 - v3 + ((2*v4) & (v3+v4))))",
    );

    // Direct and complement hidden observers share the same certificate path.
    assert_zero(
        "-v0 + (v0 & (-1 - 3*v0 - v2 - (v2 & (-1 - 2*v0)))) \
         + (v0 & (2*v2 + 3*v0 - (v2 & 2*v0)))",
    );
}

fn assert_contextual_rewrite(
    relation: Expr,
    before: Expr,
    after: Expr,
    observer: Expr,
    observer_zero: u64,
) {
    for width in 1..=4 {
        let mask = (1u64 << width) - 1;
        for a in 0..=mask {
            for b in 0..=mask {
                for c in 0..=mask {
                    for d in 0..=mask {
                        let vars = [a, b, c, d];
                        if observer.eval(&vars, width) == observer_zero {
                            assert_eq!(
                                relation.eval(&vars, width),
                                0,
                                "observer restriction is not reflected by the lowered relation: width={width}, vars={vars:?}, observer={}, relation={}",
                                observer.eval(&vars, width),
                                relation.eval(&vars, width),
                            );
                        }
                        if relation.eval(&vars, width) == 0 {
                            assert_eq!(
                                before.eval(&vars, width),
                                after.eval(&vars, width),
                                "contextual substitution changed a value under its relation: width={width}, vars={vars:?}, relation={}, before={}, after={}",
                                relation.eval(&vars, width),
                                before.eval(&vars, width),
                                after.eval(&vars, width),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn contextual_substitution_handles_direct_and_complemented_observers() {
    let x0 = Expr::Var(0.into());
    let x1 = Expr::Var(1.into());
    let x2 = Expr::Var(2.into());
    let x3 = Expr::Var(3.into());

    let direct_observer = x0.clone();
    let direct_relation =
        (direct_observer.clone() & x1.clone()) - (direct_observer.clone() & x2.clone());
    assert_contextual_rewrite(
        direct_relation,
        x0.clone() & x1.clone() & x3.clone(),
        x0.clone() & x2.clone() & x3.clone(),
        direct_observer,
        0,
    );

    let complemented_observer = !x0.clone();
    let complemented_relation =
        (complemented_observer.clone() & x1.clone()) - (complemented_observer.clone() & x2.clone());
    assert_contextual_rewrite(
        complemented_relation,
        x0.clone() & x1.clone() & x3.clone(),
        (x1.clone() & x3.clone()) - (x2.clone() & x3.clone())
            + (x0.clone() & x2.clone() & x3.clone()),
        complemented_observer,
        0,
    );
}

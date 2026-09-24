//! Exact changes of polarity in a sparse sum of conjunctions.
//!
//! The trust boundary is the extraction below: each additive term must be a
//! constant or a constant multiple of a conjunction of existing expression
//! atoms. Atoms are opaque word-valued expressions. For every word value A,
//! `A = -1 - ~A`, and for a nonempty conjunction M,
//! `A & M = M - (~A & M)`. No solver or sampled equality check is involved.

use std::collections::{BTreeMap, BTreeSet};

use crate::{expr::Expr, prettify::prettify, varint::make_mask};

type Coefficients = BTreeMap<Vec<usize>, u64>;

struct Carrier {
    atoms: Vec<Expr>,
    coefficients: Coefficients,
}

fn add_coefficient(coefficients: &mut Coefficients, key: Vec<usize>, value: u64, mask: u64) {
    let sum = coefficients
        .get(&key)
        .copied()
        .unwrap_or(0)
        .wrapping_add(value)
        & mask;
    if sum == 0 {
        coefficients.remove(&key);
    } else {
        coefficients.insert(key, sum);
    }
}

fn extract(expression: &Expr, mask: u64) -> Option<Carrier> {
    let terms = match expression {
        Expr::Add(terms) => terms.as_slice(),
        _ => std::slice::from_ref(expression),
    };
    let mut raw = Vec::with_capacity(terms.len());
    let mut atoms = BTreeSet::new();
    for term in terms {
        let (coefficient, body) = match term {
            Expr::Scale(coefficient, body) => (*coefficient & mask, body.as_ref()),
            body => (1, body),
        };
        if coefficient == 0 {
            continue;
        }
        let factors = match body {
            Expr::Const(value) => {
                raw.push((Vec::new(), coefficient.wrapping_mul(*value) & mask));
                continue;
            }
            Expr::And(factors) => factors.clone(),
            atom => vec![atom.clone()],
        };
        if factors.is_empty()
            || factors
                .iter()
                .any(|factor| matches!(factor, Expr::Const(_)))
        {
            return None;
        }
        for factor in &factors {
            atoms.insert(factor.clone());
        }
        raw.push((factors, coefficient));
    }
    let atoms: Vec<_> = atoms.into_iter().collect();
    let mut coefficients = Coefficients::new();
    for (factors, coefficient) in raw {
        let mut key = factors
            .iter()
            .map(|factor| atoms.binary_search(factor).expect("collected atom"))
            .collect::<Vec<_>>();
        key.sort_unstable();
        key.dedup();
        add_coefficient(&mut coefficients, key, coefficient, mask);
    }
    Some(Carrier {
        atoms,
        coefficients,
    })
}

/// Reexpresses one coordinate in its complemented basis. The key still names
/// the same coordinate; `flipped` tells the renderer which polarity it has.
fn flip(coefficients: &mut Coefficients, coordinate: usize, mask: u64) {
    let affected = coefficients
        .iter()
        .filter(|(key, _)| key.binary_search(&coordinate).is_ok())
        .map(|(key, coefficient)| (key.clone(), *coefficient))
        .collect::<Vec<_>>();
    for (key, coefficient) in affected {
        let mut rest = key.clone();
        rest.retain(|index| *index != coordinate);
        // The empty conjunction is the all-ones word, whose numeric value is -1.
        let rest_coefficient = if rest.is_empty() {
            coefficient.wrapping_neg()
        } else {
            coefficient
        };
        add_coefficient(coefficients, rest, rest_coefficient, mask);
        coefficients.insert(key, coefficient.wrapping_neg() & mask);
    }
}

fn render(carrier: &Carrier, coefficients: &Coefficients, flipped: &[usize], mask: u64) -> Expr {
    let mut terms = Vec::with_capacity(coefficients.len());
    for (key, coefficient) in coefficients {
        if key.is_empty() {
            terms.push(Expr::Const(*coefficient));
            continue;
        }
        let mut factors = key
            .iter()
            .map(|index| {
                let atom = carrier.atoms[*index].clone();
                if flipped.contains(index) {
                    match atom {
                        Expr::Not(inner) => *inner,
                        atom => !atom,
                    }
                } else {
                    atom
                }
            })
            .collect::<Vec<_>>();
        factors.sort_unstable();
        let body = if factors.len() == 1 {
            factors.pop().expect("one factor")
        } else {
            Expr::And(factors)
        };
        terms.push(match coefficient & mask {
            1 => body,
            coefficient => Expr::Scale(coefficient, Box::new(body)),
        });
    }
    terms.sort_unstable();
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().expect("one term"),
        _ => Expr::Add(terms),
    }
}

fn candidate(carrier: &Carrier, coefficients: &Coefficients, indices: &[usize], n: u8) -> Expr {
    prettify(render(carrier, coefficients, indices, make_mask(n)), n)
}

/// Makes one bounded pass over the incumbent, its single flips and its pairs.
/// Strict improvement keeps the incumbent on ties. Reversing each flip restores
/// the sparse map, so candidate orientations do not copy the full carrier.
pub(crate) fn bounded_polarity(expression: Expr, n: u8) -> Expr {
    let mask = make_mask(n);
    let Some(carrier) = extract(&expression, mask) else {
        return expression;
    };
    let mut coefficients = carrier.coefficients.clone();
    let mut best = expression;
    for first in 0..carrier.atoms.len() {
        flip(&mut coefficients, first, mask);
        let one = candidate(&carrier, &coefficients, &[first], n);
        if one.size() < best.size() {
            best = one;
        }
        for second in (first + 1)..carrier.atoms.len() {
            flip(&mut coefficients, second, mask);
            let two = candidate(&carrier, &coefficients, &[first, second], n);
            if two.size() < best.size() {
                best = two;
            }
            flip(&mut coefficients, second, mask);
        }
        flip(&mut coefficients, first, mask);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::VarId;

    #[test]
    fn flips_are_involutive_and_exact_at_small_widths() {
        for width in 1..=4 {
            let mask = make_mask(width);
            let a = Expr::Var(VarId(0));
            let b = Expr::Var(VarId(1));
            let expression = Expr::Add(vec![
                Expr::Const(3),
                Expr::Scale(5, Box::new(a.clone())),
                Expr::Scale(7, Box::new(Expr::And(vec![a, b]))),
            ]);
            let carrier = extract(&expression, mask).expect("sparse carrier");
            for subset in 0..(1usize << carrier.atoms.len()) {
                let indices = (0..carrier.atoms.len())
                    .filter(|index| subset & (1 << index) != 0)
                    .collect::<Vec<_>>();
                let mut coefficients = carrier.coefficients.clone();
                for &coordinate in &indices {
                    flip(&mut coefficients, coordinate, mask);
                }
                let changed = render(&carrier, &coefficients, &indices, mask);
                let prettified = prettify(changed.clone(), width);
                for x in 0..=mask {
                    for y in 0..=mask {
                        assert_eq!(
                            expression.eval(&[x, y], width),
                            changed.eval(&[x, y], width)
                        );
                        assert_eq!(
                            expression.eval(&[x, y], width),
                            prettified.eval(&[x, y], width)
                        );
                    }
                }
                for &coordinate in indices.iter().rev() {
                    flip(&mut coefficients, coordinate, mask);
                }
                assert_eq!(coefficients, carrier.coefficients);
            }
        }
    }

    #[test]
    fn unsupported_constant_conjunction_keeps_incumbent() {
        let expression = Expr::And(vec![Expr::Var(VarId(0)), Expr::Const(3)]);
        assert!(extract(&expression, u64::MAX).is_none());
        assert_eq!(bounded_polarity(expression.clone(), 64), expression);
    }
}

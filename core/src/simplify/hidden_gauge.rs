//! Hidden Gauge: complement-orbit interning for hidden definitions.
//!
//! Hidden definitions remain in the solver's single `non_linear_components`
//! map. References are expanded only while comparing an incoming definition
//! with the existing resident definitions; no parallel structural forest is
//! retained.

use crate::expr::{Expr, VarId};

use log::debug;

use super::MBASolver;

fn canonicalize<C: super::LinearCache>(solver: &MBASolver<'_, C>, e: Expr, mask: u64) -> Expr {
    match e {
        Expr::Var(variable) => solver
            .non_linear_components
            .get_by_left(&variable)
            .map(|definition| canonicalize(solver, definition.clone(), mask))
            .unwrap_or(Expr::Var(variable)),
        e => e
            .map(|child| canonicalize(solver, child, mask))
            .reduce_masked(mask),
    }
}

fn complement(e: Expr, mask: u64) -> Expr {
    (-e - Expr::make_const(1)).reduce_masked(mask)
}

fn height(e: &Expr) -> usize {
    match e {
        Expr::Var(_) | Expr::Const(_) => 0,
        Expr::Not(child) | Expr::Scale(_, child) => height(child) + 1,
        Expr::And(children)
        | Expr::Or(children)
        | Expr::Xor(children)
        | Expr::Add(children)
        | Expr::Mul(children) => children.iter().map(height).max().map_or(0, |h| h + 1),
    }
}

fn structural_key(e: &Expr, key: &mut Vec<u8>) {
    match e {
        Expr::Var(variable) => {
            key.push(0);
            key.extend_from_slice(&(variable.0 as u64).to_be_bytes());
        }
        Expr::Const(constant) => {
            key.push(1);
            key.extend_from_slice(&constant.to_be_bytes());
        }
        Expr::Not(child) => {
            key.push(2);
            structural_key(child, key);
        }
        Expr::Scale(coefficient, child) => {
            key.push(3);
            key.extend_from_slice(&coefficient.to_be_bytes());
            structural_key(child, key);
        }
        Expr::And(children)
        | Expr::Or(children)
        | Expr::Xor(children)
        | Expr::Add(children)
        | Expr::Mul(children) => {
            let tag = match e {
                Expr::And(_) => 4,
                Expr::Or(_) => 5,
                Expr::Xor(_) => 6,
                Expr::Add(_) => 7,
                Expr::Mul(_) => 8,
                _ => unreachable!(),
            };
            key.push(tag);
            key.extend_from_slice(&(children.len() as u64).to_be_bytes());
            for child in children {
                structural_key(child, key);
            }
        }
    }
}

fn orientation_is_complemented(e: &Expr, complement: &Expr) -> bool {
    let e_height = height(e);
    let complement_height = height(complement);
    match complement_height.cmp(&e_height) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => {
            let mut e_key = Vec::new();
            let mut complement_key = Vec::new();
            structural_key(e, &mut e_key);
            structural_key(complement, &mut complement_key);
            complement_key < e_key
        }
    }
}

/// Find the resident hidden coordinate whose canonical definition is `e` or
/// its complement, returning the requested orientation when one exists.
fn find_orbit<C: super::LinearCache>(
    solver: &MBASolver<'_, C>,
    e: &Expr,
    mask: u64,
) -> Option<(VarId, bool)> {
    for (variable, definition) in solver.non_linear_components.iter() {
        let definition = canonicalize(solver, definition.clone(), mask);
        if &definition == e {
            return Some((*variable, false));
        }
        if complement(definition, mask) == *e {
            return Some((*variable, true));
        }
    }
    None
}

/// Intern `e` in its exact structural complement orbit and return the
/// coordinate expression with the requested orientation.
pub(super) fn intern<C: super::LinearCache>(solver: &mut MBASolver<'_, C>, e: Expr) -> Expr {
    // Hidden coordinates are consumed later at the solver's WORD width.
    // Their complement orbit must therefore use that same width, rather than
    // a narrower dynamic mask used while simplifying the definition.
    let mask = solver.mask;
    let original = e;
    let e = canonicalize(solver, original.clone(), mask);
    let note = canonicalize(solver, complement(original.clone(), mask), mask);
    // Height is the primary orientation rule. The explicit tagged key makes
    // the tie-break stable without inheriting `Expr` enum discriminant order
    // through its derived `Ord` implementation.
    let (representative, complemented) = if orientation_is_complemented(&e, &note) {
        (note.clone(), true)
    } else {
        (e.clone(), false)
    };

    if let Some((variable, stored_complemented)) = find_orbit(solver, &representative, mask) {
        let complemented = complemented ^ stored_complemented;
        return if complemented {
            !Expr::Var(variable)
        } else {
            Expr::Var(variable)
        };
    }

    let variable: VarId = solver.t.into();
    let definition = if complemented {
        complement(original, mask)
    } else {
        original
    };
    debug_assert!(definition.get_vars().into_iter().all(|referenced| {
        solver
            .non_linear_components
            .get_by_left(&referenced)
            .is_none_or(|_| referenced.0 < variable.0)
    }));
    debug!(
        "Creating hidden-gauge variable v{} for e={}",
        variable, definition
    );
    solver.t += 1;
    solver.non_linear_components.insert(variable, definition);

    if complemented {
        !Expr::Var(variable)
    } else {
        Expr::Var(variable)
    }
}

//! Hidden Gauge: complement-orbit interning for hidden definitions.
//!
//! Each hidden definition is assigned an exact structural key as it is
//! created. The key uses the already assigned key of an earlier hidden
//! reference, so it never depends on that reference's physical `VarId`.
//! Commutative children are sorted before the parent key is formed.

use crate::expr::{Expr, VarId};

use log::debug;

use super::MBASolver;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct StructuralKey {
    height: usize,
    shape: StructuralShape,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum StructuralShape {
    Source(VarId),
    Const(u64),
    Not(Box<StructuralKey>),
    Scale(u64, Box<StructuralKey>),
    And(Vec<StructuralKey>),
    Or(Vec<StructuralKey>),
    Xor(Vec<StructuralKey>),
    Add(Vec<StructuralKey>),
    Mul(Vec<StructuralKey>),
}

impl StructuralKey {
    fn new(shape: StructuralShape) -> Self {
        let height = match &shape {
            StructuralShape::Source(_) | StructuralShape::Const(_) => 0,
            StructuralShape::Not(child) | StructuralShape::Scale(_, child) => child.height + 1,
            StructuralShape::And(children)
            | StructuralShape::Or(children)
            | StructuralShape::Xor(children)
            | StructuralShape::Add(children)
            | StructuralShape::Mul(children) => children
                .iter()
                .map(|child| child.height)
                .max()
                .map_or(0, |height| height + 1),
        };
        Self { height, shape }
    }
}

fn key_of<C: super::LinearCache>(solver: &MBASolver<'_, C>, e: &Expr, mask: u64) -> StructuralKey {
    match e {
        Expr::Var(variable) => solver
            .hidden_gauge_keys
            .get(variable)
            .cloned()
            .unwrap_or_else(|| StructuralKey::new(StructuralShape::Source(*variable))),
        Expr::Const(constant) => StructuralKey::new(StructuralShape::Const(*constant & mask)),
        Expr::Not(child) => {
            StructuralKey::new(StructuralShape::Not(Box::new(key_of(solver, child, mask))))
        }
        Expr::Scale(coefficient, child) => StructuralKey::new(StructuralShape::Scale(
            *coefficient & mask,
            Box::new(key_of(solver, child, mask)),
        )),
        Expr::And(children) => StructuralKey::new(StructuralShape::And(sorted_children(
            solver, children, mask,
        ))),
        Expr::Or(children) => {
            StructuralKey::new(StructuralShape::Or(sorted_children(solver, children, mask)))
        }
        Expr::Xor(children) => StructuralKey::new(StructuralShape::Xor(sorted_children(
            solver, children, mask,
        ))),
        Expr::Add(children) => StructuralKey::new(StructuralShape::Add(sorted_children(
            solver, children, mask,
        ))),
        Expr::Mul(children) => StructuralKey::new(StructuralShape::Mul(sorted_children(
            solver, children, mask,
        ))),
    }
}

fn sorted_children<C: super::LinearCache>(
    solver: &MBASolver<'_, C>,
    children: &[Expr],
    mask: u64,
) -> Vec<StructuralKey> {
    let mut keys: Vec<_> = children
        .iter()
        .map(|child| key_of(solver, child, mask))
        .collect();
    keys.sort_unstable();
    keys
}

fn complement(e: Expr, mask: u64) -> Expr {
    (-e - Expr::make_const(1)).reduce_masked(mask)
}

/// Intern `e` in its exact structural complement orbit and return the
/// coordinate expression with the requested orientation.
pub(super) fn intern<C: super::LinearCache>(
    solver: &mut MBASolver<'_, C>,
    e: Expr,
    mask: u64,
) -> Expr {
    let note = complement(e.clone(), mask);

    if let Some(variable) = solver.non_linear_components.get_by_right(&e) {
        debug_assert!(solver.hidden_gauge_keys.contains_key(variable));
        return Expr::Var(*variable);
    }
    if let Some(variable) = solver.non_linear_components.get_by_right(&note) {
        debug_assert!(solver.hidden_gauge_keys.contains_key(variable));
        return !Expr::Var(*variable);
    }

    let key = key_of(solver, &e, mask);
    let note_key = key_of(solver, &note, mask);
    let (orbit, complemented) = if note_key < key {
        (note_key, true)
    } else {
        (key, false)
    };

    if let Some(variable) = solver.hidden_gauge_orbits.get(&orbit).copied() {
        return if complemented {
            !Expr::Var(variable)
        } else {
            Expr::Var(variable)
        };
    }

    let variable: VarId = solver.t.into();
    let definition = if complemented { note } else { e };
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
    solver.hidden_gauge_keys.insert(variable, orbit.clone());
    solver.hidden_gauge_orbits.insert(orbit, variable);
    solver.non_linear_components.insert(variable, definition);

    if complemented {
        !Expr::Var(variable)
    } else {
        Expr::Var(variable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::{utils::cache::LocalCache, varint::make_mask};

    fn reduced_key<C: super::super::LinearCache>(
        solver: &MBASolver<'_, C>,
        e: Expr,
        mask: u64,
    ) -> StructuralKey {
        key_of(solver, &e.reduce_masked(mask), mask)
    }

    #[test]
    fn structural_key_tolerates_commutative_double_complement() {
        let mask = make_mask(4);
        let cache = LocalCache::new();
        let root = Expr::Var(VarId(0));
        let solver = MBASolver::new(&cache, &root, 4);
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let shapes = [
            x.clone() ^ y.clone(),
            x.clone() & y.clone(),
            x.clone() + y.clone() + Expr::make_const(3),
            x.clone() * y.clone(),
            !x.clone(),
            3u64 * x.clone(),
            (x & y) + (!Expr::Var(VarId(2))) + 5u64 * Expr::Var(VarId(1)),
        ];

        for shape in shapes {
            let reduced = shape.reduce_masked(mask);
            let twice = complement(complement(reduced.clone(), mask), mask);
            assert_eq!(
                reduced_key(&solver, twice, mask),
                reduced_key(&solver, reduced, mask)
            );
        }
    }

    #[test]
    fn structural_key_orders_height_before_shape() {
        let mask = make_mask(4);
        let cache = LocalCache::new();
        let root = Expr::Var(VarId(0));
        let solver = MBASolver::new(&cache, &root, 4);
        let shallow = Expr::Mul(vec![Expr::Var(VarId(0)), Expr::Var(VarId(1))]);
        let deep = Expr::Not(Box::new(Expr::Not(Box::new(Expr::Var(VarId(0))))));

        assert!(key_of(&solver, &shallow, mask) < key_of(&solver, &deep, mask));
    }

    fn expand(e: &Expr, definitions: &BTreeMap<VarId, Expr>) -> Expr {
        match e {
            Expr::Var(variable) => definitions
                .get(variable)
                .map(|definition| expand(definition, definitions))
                .unwrap_or_else(|| e.clone()),
            _ => e.clone().map(|child| expand(&child, definitions)),
        }
    }

    #[test]
    fn same_complement_orbit_reuses_one_hidden_coordinate() {
        let mask_bits = 4;
        let mask = make_mask(mask_bits);
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let definition = x.clone() & y.clone();
        let complement_definition = complement(definition.clone(), mask);
        let cache = LocalCache::new();
        let mut solver = MBASolver::new(&cache, &definition, mask_bits);

        let direct = intern(&mut solver, definition.clone(), mask);
        let complemented = intern(&mut solver, complement_definition.clone(), mask);

        assert_eq!(solver.non_linear_components.len(), 1);
        let definitions: BTreeMap<_, _> = solver
            .non_linear_components
            .iter()
            .map(|(variable, definition)| (*variable, definition.clone()))
            .collect();
        for xv in 0..16u64 {
            for yv in 0..16u64 {
                let source = [xv, yv, 0, 0];
                assert_eq!(
                    expand(&direct, &definitions).eval(&source, mask_bits),
                    definition.eval(&source, mask_bits)
                );
                assert_eq!(
                    expand(&complemented, &definitions).eval(&source, mask_bits),
                    complement_definition.eval(&source, mask_bits)
                );
            }
        }
    }
}

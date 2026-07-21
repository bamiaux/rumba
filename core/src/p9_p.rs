//! Global bounded polynomial-bitwise projection over `Z/(2^n)`.
//!
//! Unlike [`crate::p9_poly`], this experiment solves for all polynomial
//! coefficients at once. Observations only generate a candidate; acceptance
//! requires the complete source-minus-candidate expression to reduce to zero
//! through the existing exact polynomial/PCT path.

use std::time::{Duration, Instant};

use crate::{
    expr::{Expr, VarId},
    simplify::simplify_mba,
    varint::{VarInt, make_mask},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P9PLimits {
    pub max_variables: usize,
    pub max_degree: usize,
    pub max_monomials: usize,
    pub evaluation_multiplier: usize,
    pub max_evaluations: usize,
    pub holdout_evaluations: usize,
    pub max_candidate_nodes: usize,
}

impl Default for P9PLimits {
    fn default() -> Self {
        Self {
            max_variables: 3,
            max_degree: 2,
            max_monomials: 128,
            evaluation_multiplier: 4,
            max_evaluations: 512,
            holdout_evaluations: 64,
            max_candidate_nodes: 512,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P9PFailure {
    VariableBudget,
    MonomialBudget,
    EvaluationBudget,
    HowellNoSolution,
    HoldoutRejected,
    CandidateBudget,
    PctRejected,
}

#[derive(Clone, Debug)]
pub struct P9PAnalysis {
    pub result: Expr,
    pub candidate: Option<Expr>,
    pub changed: bool,
    pub degree: usize,
    pub variables: usize,
    pub monomials: usize,
    pub evaluations: usize,
    pub system_solved: bool,
    pub holdout_passed: bool,
    pub candidate_certified: bool,
    pub failure: Option<P9PFailure>,
    pub basis_time: Duration,
    pub system_time: Duration,
    pub certification_time: Duration,
}

fn modular_add(left: u64, right: u64, mask: u64) -> u64 {
    left.wrapping_add(right) & mask
}

fn modular_sub(left: u64, right: u64, mask: u64) -> u64 {
    left.wrapping_sub(right) & mask
}

fn modular_mul(left: u64, right: u64, mask: u64) -> u64 {
    left.wrapping_mul(right) & mask
}

fn inverse_odd(value: u64, mask: u64) -> u64 {
    debug_assert_eq!(value & 1, 1);
    let mut inverse = 1u64;
    for _ in 0..6 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(value.wrapping_mul(inverse)));
    }
    inverse & mask
}

/// Solves `matrix * x = rhs (mod 2^width)` with invertible row and column
/// operations. Pivots are normalized to powers of two, so no even element is
/// ever inverted.
fn solve_modular_power_of_two(
    mut matrix: Vec<Vec<u64>>,
    mut rhs: Vec<u64>,
    width: u8,
) -> Option<Vec<u64>> {
    assert!(width != 0 && width <= 64);
    assert_eq!(matrix.len(), rhs.len());
    let rows = matrix.len();
    let columns = matrix.first().map_or(0, Vec::len);
    assert!(matrix.iter().all(|row| row.len() == columns));
    let mask = make_mask(width);
    #[cfg(debug_assertions)]
    let original_matrix = matrix.clone();
    #[cfg(debug_assertions)]
    let original_rhs = rhs.clone();
    for row in &mut matrix {
        for value in row {
            *value &= mask;
        }
    }
    for value in &mut rhs {
        *value &= mask;
    }

    let mut transform = vec![vec![0u64; columns]; columns];
    for (index, row) in transform.iter_mut().enumerate() {
        row[index] = 1;
    }

    let mut rank = 0;
    while rank < rows && rank < columns {
        let pivot = (rank..rows)
            .flat_map(|row| (rank..columns).map(move |column| (row, column)))
            .filter(|(row, column)| matrix[*row][*column] != 0)
            .min_by_key(|(row, column)| matrix[*row][*column].trailing_zeros());
        let Some((pivot_row, pivot_column)) = pivot else {
            break;
        };

        matrix.swap(rank, pivot_row);
        rhs.swap(rank, pivot_row);
        if pivot_column != rank {
            for row in &mut matrix {
                row.swap(rank, pivot_column);
            }
            for row in &mut transform {
                row.swap(rank, pivot_column);
            }
        }

        let valuation = matrix[rank][rank].trailing_zeros() as usize;
        let unit = matrix[rank][rank] >> valuation;
        let inverse = inverse_odd(unit, mask);
        for value in &mut matrix[rank] {
            *value = modular_mul(*value, inverse, mask);
        }
        rhs[rank] = modular_mul(rhs[rank], inverse, mask);
        debug_assert_eq!(matrix[rank][rank], 1u64 << valuation);

        for row in 0..rows {
            if row == rank || matrix[row][rank] == 0 {
                continue;
            }
            let quotient = matrix[row][rank] >> valuation;
            for column in 0..columns {
                matrix[row][column] = modular_sub(
                    matrix[row][column],
                    modular_mul(quotient, matrix[rank][column], mask),
                    mask,
                );
            }
            rhs[row] = modular_sub(
                rhs[row],
                modular_mul(quotient, rhs[rank], mask),
                mask,
            );
        }

        for column in 0..columns {
            if column == rank || matrix[rank][column] == 0 {
                continue;
            }
            let quotient = matrix[rank][column] >> valuation;
            for row in 0..rows {
                matrix[row][column] = modular_sub(
                    matrix[row][column],
                    modular_mul(quotient, matrix[row][rank], mask),
                    mask,
                );
            }
            for row in 0..columns {
                transform[row][column] = modular_sub(
                    transform[row][column],
                    modular_mul(quotient, transform[row][rank], mask),
                    mask,
                );
            }
        }
        rank += 1;
    }

    let mut diagonal_solution = vec![0u64; columns];
    for row in 0..rank {
        let pivot = matrix[row][row];
        let valuation = pivot.trailing_zeros() as usize;
        let divisibility_mask = pivot.wrapping_sub(1);
        if rhs[row] & divisibility_mask != 0 {
            return None;
        }
        diagonal_solution[row] = rhs[row] >> valuation;
    }
    if (rank..rows).any(|row| rhs[row] != 0) {
        return None;
    }

    let mut solution = vec![0u64; columns];
    for row in 0..columns {
        for column in 0..columns {
            solution[row] = modular_add(
                solution[row],
                modular_mul(transform[row][column], diagonal_solution[column], mask),
                mask,
            );
        }
    }
    #[cfg(debug_assertions)]
    debug_assert!(original_matrix.iter().zip(original_rhs).all(|(row, expected)| {
        row.iter()
            .zip(&solution)
            .fold(0u64, |sum, (left, right)| {
                modular_add(sum, modular_mul(*left, *right, mask), mask)
            })
            == (expected & mask)
    }));
    Some(solution)
}

fn conjunction_basis(variables: &[VarId], mask: u64) -> Vec<Expr> {
    (1usize..(1usize << variables.len()))
        .map(|subset| {
            Expr::And(
                variables
                    .iter()
                    .enumerate()
                    .filter(|(position, _)| subset & (1usize << position) != 0)
                    .map(|(_, variable)| Expr::Var(*variable))
                    .collect(),
            )
            .reduce(mask)
        })
        .collect()
}

fn append_monomials(
    basis: &[Expr],
    degree: usize,
    start: usize,
    factors: &mut Vec<Expr>,
    monomials: &mut Vec<Expr>,
    mask: u64,
) {
    if factors.len() == degree {
        monomials.push(match factors.len() {
            0 => Expr::make_const(1),
            1 => factors[0].clone(),
            _ => Expr::Mul(factors.clone()).reduce(mask),
        });
        return;
    }
    for index in start..basis.len() {
        factors.push(basis[index].clone());
        append_monomials(basis, degree, index, factors, monomials, mask);
        factors.pop();
    }
}

fn polynomial_basis(variables: &[VarId], degree: usize, mask: u64) -> Vec<Expr> {
    let conjunctions = conjunction_basis(variables, mask);
    let mut monomials = vec![Expr::make_const(1)];
    for current_degree in 1..=degree {
        append_monomials(
            &conjunctions,
            current_degree,
            0,
            &mut Vec::with_capacity(current_degree),
            &mut monomials,
            mask,
        );
    }
    monomials
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn evaluation_values(
    variables: &[VarId],
    sample: usize,
    salt: u64,
    mask: u64,
    width: u8,
    exhaustive: bool,
) -> Vec<u64> {
    let max_variable = variables.iter().map(|variable| variable.0).max().unwrap_or(0);
    let mut values = vec![0u64; max_variable + 1];
    for (position, variable) in variables.iter().enumerate() {
        let value = if exhaustive {
            ((sample >> (width as usize * position)) as u64) & mask
        } else {
            match sample {
            0 => 0,
            1 => 1,
            sample if sample == position + 2 => 1,
            sample if sample < variables.len() + 2 => 0,
            _ => splitmix64(
                salt ^ (sample as u64).wrapping_mul(0xd134_2543_de82_ef95)
                    ^ position as u64,
            ),
            }
        };
        values[variable.0] = value & mask;
    }
    values
}

fn make_candidate(coefficients: &[u64], monomials: &[Expr], mask: u64) -> Expr {
    let mut terms = coefficients
        .iter()
        .zip(monomials)
        .filter_map(|(coefficient, monomial)| {
            let coefficient = coefficient & mask;
            (coefficient != 0)
                .then(|| Expr::scale(VarInt::from(coefficient), monomial.clone()))
        })
        .collect::<Vec<_>>();
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap(),
        _ => Expr::Add(terms),
    }
    .reduce(mask)
}

fn unchanged(
    expression: Expr,
    degree: usize,
    variables: usize,
    monomials: usize,
    evaluations: usize,
    failure: P9PFailure,
    basis_time: Duration,
    system_time: Duration,
) -> P9PAnalysis {
    P9PAnalysis {
        result: expression,
        candidate: None,
        changed: false,
        degree,
        variables,
        monomials,
        evaluations,
        system_solved: false,
        holdout_passed: false,
        candidate_certified: false,
        failure: Some(failure),
        basis_time,
        system_time,
        certification_time: Duration::ZERO,
    }
}

pub fn analyze_global_polynomial(
    expression: Expr,
    width: u8,
    degree: usize,
    limits: P9PLimits,
) -> P9PAnalysis {
    let mask = make_mask(width);
    let variables = expression.get_vars().into_iter().collect::<Vec<_>>();
    if variables.len() > limits.max_variables {
        return unchanged(
            expression,
            degree,
            variables.len(),
            0,
            0,
            P9PFailure::VariableBudget,
            Duration::ZERO,
            Duration::ZERO,
        );
    }
    if degree == 0 || degree > limits.max_degree {
        return unchanged(
            expression,
            degree,
            variables.len(),
            0,
            0,
            P9PFailure::MonomialBudget,
            Duration::ZERO,
            Duration::ZERO,
        );
    }

    let basis_started = Instant::now();
    let monomials = polynomial_basis(&variables, degree, mask);
    let basis_time = basis_started.elapsed();
    if monomials.len() > limits.max_monomials {
        return unchanged(
            expression,
            degree,
            variables.len(),
            monomials.len(),
            0,
            P9PFailure::MonomialBudget,
            basis_time,
            Duration::ZERO,
        );
    }
    let minimum_evaluations = monomials
        .len()
        .saturating_mul(limits.evaluation_multiplier)
        .max(monomials.len());
    let domain_bits = width as usize * variables.len();
    let exhaustive_domain = (domain_bits < usize::BITS as usize)
        .then(|| 1usize << domain_bits)
        .filter(|domain| *domain <= limits.max_evaluations);
    let evaluations = exhaustive_domain.map_or(minimum_evaluations, |domain| {
        minimum_evaluations.max(domain)
    });
    if evaluations > limits.max_evaluations {
        return unchanged(
            expression,
            degree,
            variables.len(),
            monomials.len(),
            evaluations,
            P9PFailure::EvaluationBudget,
            basis_time,
            Duration::ZERO,
        );
    }

    let system_started = Instant::now();
    let mut matrix = Vec::with_capacity(evaluations);
    let mut rhs = Vec::with_capacity(evaluations);
    for sample in 0..evaluations {
        let values = evaluation_values(
            &variables,
            sample,
            0x5039_502d_4745_4e31,
            mask,
            width,
            exhaustive_domain.is_some(),
        );
        matrix.push(
            monomials
                .iter()
                .map(|monomial| monomial.eval(&values).get(mask))
                .collect(),
        );
        rhs.push(expression.eval(&values).get(mask));
    }
    let coefficients = solve_modular_power_of_two(matrix, rhs, width);
    let system_time = system_started.elapsed();
    let Some(coefficients) = coefficients else {
        return unchanged(
            expression,
            degree,
            variables.len(),
            monomials.len(),
            evaluations,
            P9PFailure::HowellNoSolution,
            basis_time,
            system_time,
        );
    };

    let raw_candidate = make_candidate(&coefficients, &monomials, mask);
    let candidate = simplify_mba(raw_candidate.clone(), width).unwrap_or(raw_candidate);
    if candidate.size() > limits.max_candidate_nodes {
        let mut analysis = unchanged(
            expression,
            degree,
            variables.len(),
            monomials.len(),
            evaluations,
            P9PFailure::CandidateBudget,
            basis_time,
            system_time,
        );
        analysis.system_solved = true;
        return analysis;
    }

    let holdout_passed = (0..limits.holdout_evaluations).all(|sample| {
        let values = evaluation_values(
            &variables,
            sample + evaluations,
            0x5039_502d_484f_4c44,
            mask,
            width,
            false,
        );
        expression.eval(&values).get(mask) == candidate.eval(&values).get(mask)
    });
    if !holdout_passed {
        return P9PAnalysis {
            result: expression,
            candidate: Some(candidate),
            changed: false,
            degree,
            variables: variables.len(),
            monomials: monomials.len(),
            evaluations,
            system_solved: true,
            holdout_passed: false,
            candidate_certified: false,
            failure: Some(P9PFailure::HoldoutRejected),
            basis_time,
            system_time,
            certification_time: Duration::ZERO,
        };
    }

    let certification_started = Instant::now();
    let certified = simplify_mba(expression.clone() - candidate.clone(), width)
        == Ok(Expr::zero());
    let certification_time = certification_started.elapsed();
    let changed = certified && candidate.size() < expression.size();
    P9PAnalysis {
        result: if changed { candidate.clone() } else { expression },
        candidate: Some(candidate),
        changed,
        degree,
        variables: variables.len(),
        monomials: monomials.len(),
        evaluations,
        system_solved: true,
        holdout_passed: true,
        candidate_certified: certified,
        failure: (!certified).then_some(P9PFailure::PctRejected),
        basis_time,
        system_time,
        certification_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn satisfies(matrix: &[Vec<u64>], rhs: &[u64], solution: &[u64], mask: u64) -> bool {
        matrix.iter().zip(rhs).all(|(row, expected)| {
            row.iter()
                .zip(solution)
                .fold(0u64, |sum, (left, right)| {
                    modular_add(sum, modular_mul(*left, *right, mask), mask)
                })
                == (*expected & mask)
        })
    }

    #[test]
    fn modular_solver_handles_even_pivots_without_dividing_by_them() {
        let matrix = vec![vec![2]];
        let rhs = vec![2];
        let solution = solve_modular_power_of_two(matrix.clone(), rhs.clone(), 2).unwrap();
        assert!(satisfies(&matrix, &rhs, &solution, 3));
        assert_eq!(solve_modular_power_of_two(vec![vec![2]], vec![1], 2), None);
    }

    #[test]
    fn modular_solver_matches_bruteforce_for_small_systems() {
        let mask = 3;
        for seed in 0..128u64 {
            let matrix = vec![
                vec![splitmix64(seed) & mask, splitmix64(seed + 1) & mask],
                vec![splitmix64(seed + 2) & mask, splitmix64(seed + 3) & mask],
            ];
            let rhs = vec![splitmix64(seed + 4) & mask, splitmix64(seed + 5) & mask];
            let brute_force_solution = (0..=mask)
                .flat_map(|left| (0..=mask).map(move |right| vec![left, right]))
                .find(|candidate| satisfies(&matrix, &rhs, candidate, mask));
            let solution = solve_modular_power_of_two(matrix.clone(), rhs.clone(), 2);
            assert_eq!(solution.is_some(), brute_force_solution.is_some());
            if let Some(solution) = solution {
                assert!(satisfies(&matrix, &rhs, &solution, mask));
            }
        }
    }

    #[test]
    fn modular_solver_recovers_constructed_rectangular_systems() {
        for (rows, columns, width) in [(8, 5, 4), (20, 10, 8), (40, 16, 16)] {
            let mask = make_mask(width);
            for seed in 0..8u64 {
                let matrix = (0..rows)
                    .map(|row| {
                        (0..columns)
                            .map(|column| {
                                splitmix64(
                                    seed ^ (row as u64).wrapping_mul(131)
                                        ^ (column as u64).wrapping_mul(17),
                                ) & mask
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let expected_solution = (0..columns)
                    .map(|column| splitmix64(seed ^ column as u64 ^ 0x534f_4c56) & mask)
                    .collect::<Vec<_>>();
                let rhs = matrix
                    .iter()
                    .map(|row| {
                        row.iter().zip(&expected_solution).fold(
                            0u64,
                            |sum, (coefficient, value)| {
                                modular_add(
                                    sum,
                                    modular_mul(*coefficient, *value, mask),
                                    mask,
                                )
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                let solution = solve_modular_power_of_two(
                    matrix.clone(),
                    rhs.clone(),
                    width,
                )
                .expect("a system built from a witness must have a solution");
                assert!(satisfies(&matrix, &rhs, &solution, mask));
            }
        }
    }

    #[test]
    fn global_degree_two_projection_recovers_a_polynomial_bitwise_expression() {
        let x = Expr::Var(VarId(0));
        let y = Expr::Var(VarId(1));
        let expression = Expr::Mul(vec![x, y]);
        for width in [64] {
            let analysis = analyze_global_polynomial(
                expression.clone(),
                width,
                2,
                P9PLimits::default(),
            );
            assert!(analysis.system_solved);
            assert!(analysis.holdout_passed);
            assert!(
                analysis.candidate_certified,
                "width={width} failure={:?} candidate={:?}",
                analysis.failure,
                analysis.candidate
            );
        }
    }
}

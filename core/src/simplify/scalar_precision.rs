//! Width-aware scalar normalization used after polynomial solving.
//!
//! A coefficient with `v2(c) = g` only observes the low `k - g` bits of its
//! child in a `k`-bit ring. Keeping that fact explicit prevents a local-width
//! result from being silently treated as a full-width word when hidden
//! coordinates are restored.

use crate::expr::Expr;

fn mask(k: u8) -> u64 {
    match k {
        0 => 0,
        64.. => u64::MAX,
        _ => (1u64 << k) - 1,
    }
}

fn v2(c: u64, k: u8) -> u8 {
    let c = c & mask(k);
    if c == 0 {
        k
    } else {
        (c.trailing_zeros() as u8).min(k)
    }
}

fn canonical(c: u64, k: u8) -> u64 {
    if k == 0 {
        return 0;
    }
    let m = mask(k);
    let mut r = c & m;
    if k < 64 && r & (1u64 << (k - 1)) != 0 {
        r |= !m;
    }
    r
}

pub(super) fn normalize(e: Expr, k: u8) -> Expr {
    if k == 0 {
        return Expr::zero();
    }

    match e {
        Expr::Const(c) => Expr::make_const(canonical(c, k)),
        Expr::Scale(c, x) => {
            let r = c & mask(k);
            if r == 0 {
                return Expr::zero();
            }
            let grade = v2(r, k);
            let x = normalize(*x, k - grade);
            if r == 1 {
                x
            } else {
                Expr::scale(canonical(r, k), x)
            }
        }
        Expr::Mul(xs) => {
            let mut coefficient = 1u64;
            let mut rest = Vec::with_capacity(xs.len());
            let mut had_constant = false;
            for x in xs {
                if let Expr::Const(c) = x {
                    coefficient = coefficient.wrapping_mul(c) & mask(k);
                    had_constant = true;
                } else {
                    rest.push(x);
                }
            }
            if !had_constant {
                return Expr::Mul(rest.into_iter().map(|x| normalize(x, k)).collect());
            }
            if coefficient == 0 {
                return Expr::zero();
            }
            let grade = v2(coefficient, k);
            let mut rest = rest
                .into_iter()
                .map(|x| normalize(x, k - grade))
                .collect::<Vec<_>>();
            let core = match rest.len() {
                0 => Expr::make_const(1),
                1 => rest.remove(0),
                _ => Expr::Mul(rest),
            };
            if coefficient == 1 {
                core
            } else {
                Expr::scale(canonical(coefficient, k), core)
            }
        }
        other => other.map(|x| normalize(x, k)),
    }
}

//! Native low-prefix quotient used by the first RUMBA reducer.
//!
//! Query-local, algebraic, fail-closed. No Forest, automaton, worklist,
//! corpus routing, truth-table carrier, or global mutable state.

#![allow(
    clippy::collapsible_if,
    clippy::manual_checked_ops,
    clippy::manual_is_multiple_of,
    clippy::needless_range_loop
)]

use std::collections::{BTreeMap, BTreeSet};

use crate::expr::{Expr, VarId};

use super::reduce_masked_plain;

const WORD_BITS: u8 = 64;
const RUMBA_TD_LIMIT: usize = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Unsupported;

type Result<T> = std::result::Result<T, Unsupported>;

#[inline]
fn low_mask(bits: u8) -> u64 {
    if bits == 0 {
        return 0;
    }
    if bits >= 64 {
        return u64::MAX;
    }
    (1u64 << bits) - 1
}

#[inline]
fn pow2(c: u64) -> bool {
    c != 0 && c.is_power_of_two()
}

#[inline]
fn v2(c: u64) -> u8 {
    if c == 0 {
        return WORD_BITS;
    }
    c.trailing_zeros().min(64) as u8
}

#[inline]
fn add_mod(a: u64, b: u64, bits: u8) -> u64 {
    a.wrapping_add(b) & low_mask(bits)
}

#[inline]
fn mul_mod(a: u64, b: u64, bits: u8) -> u64 {
    a.wrapping_mul(b) & low_mask(bits)
}

#[inline]
fn signed_residue(value: u64, bits: u8) -> u64 {
    let value = value & low_mask(bits);
    if bits == 0 || bits >= 64 {
        return value;
    }
    let modulus = 1u64 << bits;
    if value < (modulus >> 1) {
        value
    } else {
        value.wrapping_sub(modulus)
    }
}

#[inline]
fn better(a: &Expr, b: &Expr) -> bool {
    (a.size(), a) < (b.size(), b)
}

/// Returns the total dyadic shift and the sole non-constant child when an
/// expression is exactly `2^s * child` syntactically. This is a local operator
/// law, not a multi-node pattern catalogue.
fn dyadic_shift(e: &Expr) -> Option<(u8, &Expr)> {
    match e {
        Expr::Scale(c, child) if pow2(*c) => Some((c.trailing_zeros() as u8, child)),
        Expr::Mul(children) => {
            let mut shift = 0u16;
            let mut body = None;
            for child in children {
                match child {
                    Expr::Const(c) if pow2(*c) => {
                        shift = shift.saturating_add(c.trailing_zeros() as u16);
                    }
                    Expr::Const(_) => return None,
                    other if body.is_none() => body = Some(other),
                    _ => return None,
                }
            }
            Some((shift.min(255) as u8, body?))
        }
        _ => None,
    }
}

fn typed_frontier(e: &Expr) -> Vec<Expr> {
    fn go(e: &Expr, seen: &mut BTreeSet<Expr>, out: &mut Vec<Expr>) {
        let mut leaf = |x: &Expr| {
            if seen.insert(x.clone()) {
                out.push(x.clone());
            }
        };

        match e {
            Expr::Const(_) => {}
            Expr::Var(_) => leaf(e),
            Expr::Not(child) => go(child, seen, out),
            Expr::And(children) | Expr::Or(children) | Expr::Xor(children) => {
                for child in children {
                    go(child, seen, out);
                }
            }
            _ => {
                if let Some((_, child)) = dyadic_shift(e) {
                    go(child, seen, out);
                    return;
                }
                leaf(e);
            }
        }
    }

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    go(e, &mut seen, &mut out);
    out
}

// -----------------------------------------------------------------------------
// Symbolic Boolean ANF view (Shadow K/L/A)
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
struct BoolAnf {
    // GF(2) polynomial: set of idempotent monomials. Empty monomial = 1.
    terms: BTreeSet<BTreeSet<usize>>,
}

impl BoolAnf {
    fn one() -> Self {
        Self {
            terms: [BTreeSet::new()].into_iter().collect(),
        }
    }

    fn var(i: usize) -> Self {
        Self {
            terms: [[i].into_iter().collect()].into_iter().collect(),
        }
    }

    fn is_zero(&self) -> bool {
        self.terms.is_empty()
    }

    fn size(&self) -> usize {
        self.terms.iter().map(|m| 1 + m.len()).sum()
    }

    fn xor(&self, other: &Self) -> Self {
        let mut out = self.terms.clone();
        for mon in &other.terms {
            if !out.insert(mon.clone()) {
                out.remove(mon);
            }
        }
        Self { terms: out }
    }

    fn not(&self) -> Self {
        self.xor(&Self::one())
    }

    fn and(&self, other: &Self) -> Result<Self> {
        if self.is_zero() || other.is_zero() {
            return Ok(Self::default());
        }

        let mut out = BTreeSet::new();
        for left in &self.terms {
            for right in &other.terms {
                let mon = left.union(right).copied().collect::<BTreeSet<_>>();
                if !out.insert(mon.clone()) {
                    out.remove(&mon);
                }
            }
        }
        let result = Self { terms: out };
        if result.size() > self.size() + other.size() {
            return Err(Unsupported);
        }
        Ok(result)
    }

    fn or(&self, other: &Self) -> Result<Self> {
        let product = self.and(other)?;
        let result = self.xor(other).xor(&product);
        if result.size() > self.size() + other.size() + 1 {
            return Err(Unsupported);
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Cylinder {
    lo: u8,
    hi: u8,
    factors: Vec<(u8, BoolAnf)>,
}

impl Cylinder {
    fn factor_map(&self) -> BTreeMap<u8, BoolAnf> {
        self.factors.iter().cloned().collect()
    }
}

struct View {
    k: u8,
}

impl View {
    fn new(k: u8) -> Self {
        Self { k }
    }

    fn make(&self, lo: u8, hi: u8, factors: BTreeMap<u8, BoolAnf>) -> Option<Cylinder> {
        let lo = lo.min(self.k);
        let hi = hi.min(self.k);
        if lo >= hi {
            return None;
        }

        let mut out = Vec::new();
        for (offset, factor) in factors {
            if factor.is_zero() {
                return None;
            }
            if factor != BoolAnf::one() {
                out.push((offset, factor));
            }
        }
        Some(Cylinder {
            lo,
            hi,
            factors: out,
        })
    }

    fn disjoint(&self, a: &Cylinder, b: &Cylinder) -> bool {
        if a.lo.max(b.lo) >= a.hi.min(b.hi) {
            return true;
        }
        let aa = a.factor_map();
        let bb = b.factor_map();
        for offset in aa.keys().filter(|offset| bb.contains_key(offset)) {
            let Some(left) = aa.get(offset) else { continue };
            let Some(right) = bb.get(offset) else {
                continue;
            };
            match left.and(right) {
                Ok(product) if product.is_zero() => return true,
                _ => {}
            }
        }
        false
    }

    fn merge(&self, a: &Cylinder, b: &Cylinder) -> Result<Option<Cylinder>> {
        if a.factors == b.factors && (a.hi == b.lo || b.hi == a.lo) {
            return Ok(self.make(a.lo.min(b.lo), a.hi.max(b.hi), a.factor_map()));
        }
        if a.lo != b.lo || a.hi != b.hi {
            return Ok(None);
        }

        let aa = a.factor_map();
        let bb = b.factor_map();
        let mut common = BTreeMap::new();
        let mut diff = Vec::new();
        let offsets = aa.keys().chain(bb.keys()).copied().collect::<BTreeSet<_>>();
        for offset in offsets {
            let left = aa.get(&offset).cloned().unwrap_or_else(BoolAnf::one);
            let right = bb.get(&offset).cloned().unwrap_or_else(BoolAnf::one);
            if left == right {
                if left != BoolAnf::one() {
                    common.insert(offset, left);
                }
                continue;
            }
            diff.push((offset, left, right));
        }
        if diff.len() != 1 {
            return Ok(None);
        }

        let (offset, left, right) = diff.remove(0);
        if !left.and(&right)?.is_zero() {
            return Ok(None);
        }
        let union = left.or(&right)?;
        if union != BoolAnf::one() {
            common.insert(offset, union);
        }
        Ok(self.make(a.lo, a.hi, common))
    }

    fn canon<I>(&self, cylinders: I) -> Result<Vec<Cylinder>>
    where
        I: IntoIterator<Item = Option<Cylinder>>,
    {
        let mut current = cylinders.into_iter().flatten().collect::<BTreeSet<_>>();
        loop {
            let items = current.iter().cloned().collect::<Vec<_>>();
            let mut found = None;
            'outer: for i in 0..items.len() {
                for j in (i + 1)..items.len() {
                    if let Some(merged) = self.merge(&items[i], &items[j])? {
                        found = Some((items[i].clone(), items[j].clone(), merged));
                        break 'outer;
                    }
                }
            }
            let Some((left, right, merged)) = found else {
                break;
            };
            current.remove(&left);
            current.remove(&right);
            current.insert(merged);
        }

        let items = current.into_iter().collect::<Vec<_>>();
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                if !self.disjoint(&items[i], &items[j]) {
                    return Err(Unsupported);
                }
            }
        }
        Ok(items)
    }

    fn ones(&self) -> Vec<Cylinder> {
        self.make(0, self.k, BTreeMap::new()).into_iter().collect()
    }

    fn constant(&self, c: u64) -> Result<Vec<Cylinder>> {
        let c = c & low_mask(self.k);
        let mut out = Vec::new();
        let mut i = 0u8;
        while i < self.k {
            if (c >> i) & 1 == 0 {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < self.k && ((c >> j) & 1) != 0 {
                j += 1;
            }
            out.push(self.make(i, j, BTreeMap::new()));
            i = j;
        }
        self.canon(out)
    }

    fn var(&self, i: usize) -> Vec<Cylinder> {
        let factors = [(0, BoolAnf::var(i))].into_iter().collect();
        self.make(0, self.k, factors).into_iter().collect()
    }

    fn intersect(&self, a: &Cylinder, b: &Cylinder) -> Result<Option<Cylinder>> {
        let mut factors = a.factor_map();
        for (offset, factor) in &b.factors {
            let old = factors.get(offset).cloned().unwrap_or_else(BoolAnf::one);
            factors.insert(*offset, old.and(factor)?);
        }
        Ok(self.make(a.lo.max(b.lo), a.hi.min(b.hi), factors))
    }

    fn and(&self, a: &[Cylinder], b: &[Cylinder]) -> Result<Vec<Cylinder>> {
        let mut products = Vec::new();
        for left in a {
            for right in b {
                products.push(self.intersect(left, right)?);
            }
        }
        let result = self.canon(products)?;
        if result.len() > a.len() + b.len() {
            return Err(Unsupported);
        }
        Ok(result)
    }

    fn sub_cylinder(&self, b: &Cylinder, a: &Cylinder) -> Result<Vec<Cylinder>> {
        if self.disjoint(a, b) {
            return Ok(vec![b.clone()]);
        }

        let mut out = Vec::new();
        if b.lo < a.lo {
            if let Some(c) = self.make(b.lo, b.hi.min(a.lo), b.factor_map()) {
                out.push(c);
            }
        }
        if b.hi > a.hi {
            if let Some(c) = self.make(b.lo.max(a.hi), b.hi, b.factor_map()) {
                out.push(c);
            }
        }

        let lo = a.lo.max(b.lo);
        let hi = a.hi.min(b.hi);
        if lo < hi {
            let mut prefix = b.factor_map();
            for (offset, af) in &a.factors {
                let bf = prefix.get(offset).cloned().unwrap_or_else(BoolAnf::one);
                let bad = bf.and(&af.not())?;
                if !bad.is_zero() {
                    let mut factors = prefix.clone();
                    factors.insert(*offset, bad);
                    if let Some(c) = self.make(lo, hi, factors) {
                        out.push(c);
                    }
                }
                let good = bf.and(af)?;
                if good.is_zero() {
                    break;
                }
                if good == BoolAnf::one() {
                    prefix.remove(offset);
                } else {
                    prefix.insert(*offset, good);
                }
            }
        }
        if out.len() > 1 {
            return Err(Unsupported);
        }
        Ok(out)
    }

    fn sub(&self, a: &[Cylinder], b: &[Cylinder]) -> Result<Vec<Cylinder>> {
        let mut current = a.to_vec();
        for y in b {
            let mut next = Vec::new();
            for x in &current {
                next.extend(self.sub_cylinder(x, y)?);
            }
            next.sort();
            next.dedup();
            if next.len() > a.len() + b.len() {
                return Err(Unsupported);
            }
            current = next;
            if current.is_empty() {
                break;
            }
        }
        self.canon(current.into_iter().map(Some))
    }

    fn or(&self, a: &[Cylinder], b: &[Cylinder]) -> Result<Vec<Cylinder>> {
        let mut input = a.iter().cloned().map(Some).collect::<Vec<_>>();
        input.extend(self.sub(b, a)?.into_iter().map(Some));
        let result = self.canon(input)?;
        if result.len() > a.len() + b.len() {
            return Err(Unsupported);
        }
        Ok(result)
    }

    fn xor(&self, a: &[Cylinder], b: &[Cylinder]) -> Result<Vec<Cylinder>> {
        let mut input = self.sub(a, b)?.into_iter().map(Some).collect::<Vec<_>>();
        input.extend(self.sub(b, a)?.into_iter().map(Some));
        let result = self.canon(input)?;
        if result.len() > a.len() + b.len() {
            return Err(Unsupported);
        }
        Ok(result)
    }

    fn not(&self, a: &[Cylinder]) -> Result<Vec<Cylinder>> {
        let ones = self.ones();
        let result = self.sub(&ones, a)?;
        if result.len() > a.len() + 1 {
            return Err(Unsupported);
        }
        Ok(result)
    }

    fn shift_cylinder(&self, c: &Cylinder, shift: u8) -> Option<Cylinder> {
        let factors = c
            .factors
            .iter()
            .map(|(offset, factor)| (offset.saturating_add(shift), factor.clone()))
            .collect();
        self.make(
            c.lo.saturating_add(shift),
            c.hi.saturating_add(shift),
            factors,
        )
    }

    fn shift(&self, a: &[Cylinder], shift: u8) -> Result<Vec<Cylinder>> {
        let result = self.canon(a.iter().map(|c| self.shift_cylinder(c, shift)))?;
        if result.len() > a.len() {
            return Err(Unsupported);
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ShadowAtom {
    K(u64),
    L(usize),
    A(Expr),
}

type ShadowPoly = BTreeMap<ShadowAtom, u64>;

struct ShadowKla {
    k: u8,
    mask: u64,
    leaves: Vec<Expr>,
    leaf_index: BTreeMap<Expr, usize>,
    view: View,
    shadow_memo: BTreeMap<Expr, ShadowPoly>,
    valuation_memo: BTreeMap<Expr, u8>,
    view_memo: BTreeMap<Expr, Vec<Cylinder>>,
}

impl ShadowKla {
    fn new(k: u8, leaves: Vec<Expr>) -> Self {
        let leaf_index = leaves
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, e)| (e, i))
            .collect();
        Self {
            k,
            mask: low_mask(k),
            leaves,
            leaf_index,
            view: View::new(k),
            shadow_memo: BTreeMap::new(),
            valuation_memo: BTreeMap::new(),
            view_memo: BTreeMap::new(),
        }
    }

    fn clean(&self, mut poly: ShadowPoly) -> ShadowPoly {
        poly.retain(|_, c| {
            *c &= self.mask;
            *c != 0
        });
        poly
    }

    fn add(&self, a: &ShadowPoly, b: &ShadowPoly, scale: u64) -> ShadowPoly {
        let mut out = a.clone();
        for (atom, coefficient) in b {
            let value = add_mod(
                out.get(atom).copied().unwrap_or(0),
                coefficient.wrapping_mul(scale),
                self.k,
            );
            if value == 0 {
                out.remove(atom);
            } else {
                out.insert(atom.clone(), value);
            }
        }
        out
    }

    fn scale(&self, a: &ShadowPoly, scale: u64) -> ShadowPoly {
        self.clean(
            a.iter()
                .map(|(atom, coefficient)| (atom.clone(), mul_mod(*coefficient, scale, self.k)))
                .collect(),
        )
    }

    fn is_low_constant(&self, e: &Expr) -> bool {
        matches!(e, Expr::Const(c) if (*c & self.mask) == self.mask)
    }

    fn candidate(&self, children: &[Expr]) -> Expr {
        fn add(this: &ShadowKla, e: &Expr, out: &mut BTreeSet<Expr>) {
            if this.is_low_constant(e) {
                return;
            }
            if let Expr::And(children) = e {
                for child in children {
                    add(this, child, out);
                }
                return;
            }
            out.insert(e.clone());
        }

        let mut factors = BTreeSet::new();
        for child in children {
            add(self, child, &mut factors);
        }
        if factors.is_empty() {
            return Expr::Const(self.mask);
        }
        if factors.len() == 1 {
            return factors.into_iter().next().unwrap_or(Expr::Const(self.mask));
        }
        Expr::And(factors.into_iter().collect())
    }

    fn shadow(&mut self, e: &Expr) -> Result<ShadowPoly> {
        if let Some(hit) = self.shadow_memo.get(e) {
            return Ok(hit.clone());
        }

        let poly = if let Some(index) = self.leaf_index.get(e).copied() {
            [(ShadowAtom::L(index), 1)].into_iter().collect()
        } else {
            match e {
                Expr::Const(c) => {
                    let c = *c & self.mask;
                    if c == 0 {
                        BTreeMap::new()
                    } else {
                        [(ShadowAtom::K(c), 1)].into_iter().collect()
                    }
                }
                _ if let Some((shift, child)) = dyadic_shift(e) => {
                    let coefficient = if shift >= 64 { 0 } else { 1u64 << shift };
                    let child = self.shadow(child)?;
                    self.scale(&child, coefficient)
                }
                Expr::Xor(children) | Expr::Or(children) => {
                    let Some(first) = children.first() else {
                        return Err(Unsupported);
                    };
                    let mut current_expr = first.clone();
                    let mut current = self.shadow(first)?;
                    let scale = if matches!(e, Expr::Xor(_)) {
                        u64::MAX - 1
                    } else {
                        u64::MAX
                    };
                    for child in &children[1..] {
                        let child_poly = self.shadow(child)?;
                        current = self.add(&current, &child_poly, 1);
                        let atom =
                            ShadowAtom::A(self.candidate(&[current_expr.clone(), child.clone()]));
                        current = self.add(&current, &[(atom, 1)].into_iter().collect(), scale);
                        current_expr = if matches!(e, Expr::Xor(_)) {
                            Expr::Xor(vec![current_expr, child.clone()])
                        } else {
                            Expr::Or(vec![current_expr, child.clone()])
                        };
                    }
                    current
                }
                Expr::Not(child) => {
                    let base = [(ShadowAtom::K(self.mask), 1)].into_iter().collect();
                    let child = self.shadow(child)?;
                    self.add(&base, &child, u64::MAX)
                }
                Expr::And(children) => {
                    let filtered = children
                        .iter()
                        .filter(|child| !self.is_low_constant(child))
                        .cloned()
                        .collect::<Vec<_>>();
                    if filtered.is_empty() {
                        [(ShadowAtom::K(self.mask), 1)].into_iter().collect()
                    } else if filtered.len() == 1 {
                        self.shadow(&filtered[0])?
                    } else {
                        [(ShadowAtom::A(self.candidate(&filtered)), 1)]
                            .into_iter()
                            .collect()
                    }
                }
                _ => return Err(Unsupported),
            }
        };

        let poly = self.clean(poly);
        self.shadow_memo.insert(e.clone(), poly.clone());
        Ok(poly)
    }

    fn valuation(&mut self, e: &Expr) -> u8 {
        if let Some(hit) = self.valuation_memo.get(e).copied() {
            return hit;
        }

        let value = if self.leaf_index.contains_key(e) {
            0
        } else {
            match e {
                Expr::Const(c) => v2(*c & self.mask).min(self.k),
                _ if let Some((shift, child)) = dyadic_shift(e) => {
                    self.k.min(shift.saturating_add(self.valuation(child)))
                }
                Expr::And(children) => children
                    .iter()
                    .map(|child| self.valuation(child))
                    .max()
                    .unwrap_or(self.k),
                Expr::Or(children) | Expr::Xor(children) => children
                    .iter()
                    .map(|child| self.valuation(child))
                    .min()
                    .unwrap_or(self.k),
                _ => 0,
            }
        };
        self.valuation_memo.insert(e.clone(), value);
        value
    }

    fn drop_terms(&mut self, poly: &ShadowPoly) -> ShadowPoly {
        let mut out = BTreeMap::new();
        for (atom, coefficient) in poly {
            let valuation = match atom {
                ShadowAtom::A(e) => self.valuation(e),
                ShadowAtom::K(c) => v2(*c),
                ShadowAtom::L(_) => 0,
            };
            if v2(*coefficient).saturating_add(valuation) < self.k {
                out.insert(atom.clone(), *coefficient);
            }
        }
        self.clean(out)
    }

    fn view(&mut self, e: &Expr) -> Result<Vec<Cylinder>> {
        if let Some(hit) = self.view_memo.get(e) {
            return Ok(hit.clone());
        }

        let result = if let Some(index) = self.leaf_index.get(e).copied() {
            self.view.var(index)
        } else {
            match e {
                Expr::Const(c) => self.view.constant(*c)?,
                _ if let Some((shift, child)) = dyadic_shift(e) => {
                    let child = self.view(child)?;
                    self.view.shift(&child, shift)?
                }
                Expr::And(children) | Expr::Or(children) | Expr::Xor(children) => {
                    let filtered = if matches!(e, Expr::And(_)) {
                        children
                            .iter()
                            .filter(|child| !self.is_low_constant(child))
                            .collect::<Vec<_>>()
                    } else {
                        children.iter().collect::<Vec<_>>()
                    };
                    let mut current = if let Some(first) = filtered.first() {
                        self.view(first)?
                    } else {
                        self.view.ones()
                    };
                    for child in filtered.iter().skip(1) {
                        let right = self.view(child)?;
                        current = match e {
                            Expr::And(_) => self.view.and(&current, &right)?,
                            Expr::Or(_) => self.view.or(&current, &right)?,
                            Expr::Xor(_) => self.view.xor(&current, &right)?,
                            _ => unreachable!(),
                        };
                    }
                    current
                }
                Expr::Not(child) => {
                    let child = self.view(child)?;
                    self.view.not(&child)?
                }
                _ => return Err(Unsupported),
            }
        };

        self.view_memo.insert(e.clone(), result.clone());
        Ok(result)
    }

    fn norm_a(&mut self, poly: &ShadowPoly) -> Result<BTreeMap<Cylinder, u64>> {
        let mut out = BTreeMap::new();
        for (atom, coefficient) in self.clean(poly.clone()) {
            let ShadowAtom::A(e) = atom else {
                return Err(Unsupported);
            };
            let shift = v2(coefficient);
            if shift >= self.k {
                continue;
            }
            let odd = (coefficient >> shift) & low_mask(self.k - shift);
            for cylinder in self.view(&e)? {
                if let Some(shifted) = self.view.shift_cylinder(&cylinder, shift) {
                    let value = add_mod(out.get(&shifted).copied().unwrap_or(0), odd, self.k);
                    if value == 0 {
                        out.remove(&shifted);
                    } else {
                        out.insert(shifted, value);
                    }
                }
            }
        }
        out.retain(|cylinder, coefficient| v2(*coefficient).saturating_add(cylinder.lo) < self.k);
        Ok(out)
    }

    fn project(&mut self, e: &Expr) -> Result<Option<Expr>> {
        let raw = self.shadow(e)?;
        if !raw.iter().any(|(atom, coefficient)| {
            matches!(atom, ShadowAtom::A(_)) && (*coefficient & self.mask) != 0
        }) {
            return Ok(None);
        }

        let projected = self.drop_terms(&raw);
        let mut base = BTreeMap::new();
        let mut residual = BTreeMap::new();
        for (atom, coefficient) in projected {
            match atom {
                ShadowAtom::A(_) => {
                    residual.insert(atom, coefficient);
                }
                ShadowAtom::K(_) | ShadowAtom::L(_) => {
                    base.insert(atom, coefficient);
                }
            }
        }
        if !residual.is_empty() && !self.norm_a(&residual)?.is_empty() {
            return Ok(None);
        }

        let mut terms = Vec::new();
        let mut constant = 0u64;
        for (atom, coefficient) in self.clean(base) {
            match atom {
                ShadowAtom::K(c) => {
                    constant = add_mod(constant, mul_mod(coefficient, c, self.k), self.k);
                }
                ShadowAtom::L(index) => {
                    let leaf = self.leaves.get(index).cloned().ok_or(Unsupported)?;
                    terms.push(if coefficient == 1 {
                        leaf
                    } else {
                        Expr::scale(coefficient, leaf)
                    });
                }
                ShadowAtom::A(_) => return Err(Unsupported),
            }
        }
        if constant != 0 {
            terms.push(Expr::Const(constant));
        }
        if terms.is_empty() {
            return Ok(Some(Expr::zero()));
        }
        if terms.len() == 1 {
            return Ok(terms.pop());
        }
        Ok(Some(Expr::Add(terms)))
    }
}

fn shadow_project(e: &Expr, k: u8) -> Option<Expr> {
    let leaves = typed_frontier(e);
    if leaves.is_empty() {
        return None;
    }
    ShadowKla::new(k, leaves).project(e).ok().flatten()
}

// -----------------------------------------------------------------------------
// Cell additive quotient + exact DisjointPair support-2 module
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
struct CellMonomial(Vec<(usize, usize)>);

type CellPoly = BTreeMap<CellMonomial, u64>;

fn v2_factorial(mut n: usize) -> usize {
    let mut out = 0usize;
    while n != 0 {
        n /= 2;
        out += n;
    }
    out
}

fn falling(x: u64, n: usize) -> u64 {
    let mut out = 1u64;
    for j in 0..n {
        out = out.wrapping_mul(x.wrapping_sub(j as u64));
    }
    out
}

fn binomial(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut out = 1u128;
    for i in 0..k {
        out = out * (n - i) as u128 / (i + 1) as u128;
    }
    out as u64
}

fn stirling(n: usize, k: usize) -> u64 {
    if n == 0 && k == 0 {
        return 1;
    }
    if n == 0 || k == 0 || k > n {
        return 0;
    }
    let mut row = vec![0u64; k + 1];
    row[0] = 1;
    for i in 1..=n {
        let mut next = vec![0u64; k + 1];
        for j in 1..=i.min(k) {
            next[j] = (j as u64).wrapping_mul(row[j]).wrapping_add(row[j - 1]);
        }
        row = next;
    }
    row[k]
}

fn frontier(w: u8) -> Vec<(usize, usize)> {
    let mut vals = Vec::new();
    let mut i = 0usize;
    while v2_factorial(i) < w as usize {
        vals.push(i);
        i += 1;
    }
    let mut out = Vec::new();
    for a in &vals {
        for b in &vals {
            if v2_factorial(*a) + v2_factorial(*b) < w as usize {
                out.push((*a, *b));
            }
        }
    }
    out
}

fn odd_inverse(a: u64, bits: u8) -> u64 {
    debug_assert!(a & 1 == 1);
    let mut x = 1u64;
    for _ in 0..6 {
        x = x.wrapping_mul(2u64.wrapping_sub(a.wrapping_mul(x)));
    }
    x & low_mask(bits)
}

fn row_basis(rows: Vec<Vec<u64>>, w: u8) -> Vec<Vec<u64>> {
    let mask = low_mask(w);
    let mut matrix = rows
        .into_iter()
        .map(|row| row.into_iter().map(|x| x & mask).collect::<Vec<_>>())
        .filter(|row| row.iter().any(|x| *x != 0))
        .collect::<Vec<_>>();
    if matrix.is_empty() {
        return Vec::new();
    }

    let columns = matrix[0].len();
    let mut rank = 0usize;
    for col in 0..columns {
        let mut best = None;
        for i in rank..matrix.len() {
            let value = matrix[i][col] & mask;
            if value == 0 {
                continue;
            }
            let key = (v2(value), matrix[i].clone(), i);
            if best
                .as_ref()
                .is_none_or(|old: &(u8, Vec<u64>, usize)| key < *old)
            {
                best = Some(key);
            }
        }
        let Some((_, _, pivot)) = best else { continue };
        matrix.swap(rank, pivot);
        let x = matrix[rank][col] & mask;
        let shift = v2(x);
        let divisor = if shift >= 64 { 0 } else { 1u64 << shift };
        let bits = w.saturating_sub(shift);
        let odd = (x >> shift) & low_mask(bits);
        let inverse = odd_inverse(odd, bits);
        for value in &mut matrix[rank] {
            *value = value.wrapping_mul(inverse) & mask;
        }
        let pivot_row = matrix[rank].clone();
        for row in matrix.iter_mut().skip(rank + 1) {
            let a = row[col] & mask;
            if a == 0 {
                continue;
            }
            let quotient = if divisor == 0 {
                0
            } else {
                (a / divisor) & low_mask(bits)
            };
            for (left, right) in row.iter_mut().zip(&pivot_row) {
                *left = left.wrapping_sub(quotient.wrapping_mul(*right)) & mask;
            }
        }
        rank += 1;
        if rank == matrix.len() {
            break;
        }
    }

    let mut basis = matrix[..rank]
        .iter()
        .filter(|row| row.iter().any(|x| *x != 0))
        .cloned()
        .collect::<Vec<_>>();
    let pivots = basis
        .iter()
        .filter_map(|row| {
            let col = row.iter().position(|x| (*x & mask) != 0)?;
            Some((col, v2(row[col])))
        })
        .collect::<Vec<_>>();
    for rr in (0..basis.len()).rev() {
        let (col, shift) = pivots[rr];
        let divisor = 1u64 << shift;
        let bits = w - shift;
        let pivot = basis[rr].clone();
        for i in 0..rr {
            let a = basis[i][col] & mask;
            if a == 0 || a % divisor != 0 {
                continue;
            }
            let quotient = (a / divisor) & low_mask(bits);
            for (left, right) in basis[i].iter_mut().zip(&pivot) {
                *left = left.wrapping_sub(quotient.wrapping_mul(*right)) & mask;
            }
        }
    }
    basis
}

fn translate(
    row: &[u64],
    alphas: &[(usize, usize)],
    index: &BTreeMap<(usize, usize), usize>,
    h: u64,
    axis: u8,
    w: u8,
) -> Vec<u64> {
    let mask = low_mask(w);
    let mut out = vec![0u64; alphas.len()];
    for (target, (i, j)) in alphas.iter().copied().enumerate() {
        let mut sum = 0u64;
        if axis == 0 {
            for r in 0..=i {
                let Some(source) = index.get(&(r, j)).copied() else {
                    continue;
                };
                sum = sum.wrapping_add(
                    binomial(i, r)
                        .wrapping_mul(falling(h, i - r))
                        .wrapping_mul(row[source]),
                );
            }
        } else {
            for r in 0..=j {
                let Some(source) = index.get(&(i, r)).copied() else {
                    continue;
                };
                sum = sum.wrapping_add(
                    binomial(j, r)
                        .wrapping_mul(falling(h, j - r))
                        .wrapping_mul(row[source]),
                );
            }
        }
        out[target] = sum & mask;
    }
    out
}

struct PairModule {
    alphas: Vec<(usize, usize)>,
    index: BTreeMap<(usize, usize), usize>,
    basis: Vec<Vec<u64>>,
}

impl PairModule {
    fn new(w: u8) -> Self {
        let alphas = frontier(w);
        let index = alphas
            .iter()
            .copied()
            .enumerate()
            .map(|(i, alpha)| (alpha, i))
            .collect::<BTreeMap<_, _>>();
        let mut e = vec![0u64; alphas.len()];
        if let Some(i) = index.get(&(0, 0)).copied() {
            e[i] = 1;
        }
        let mut basis = vec![e];
        for bit in 0..w {
            let h = 1u64 << bit;
            let current = basis.clone();
            let mut rows = current.clone();
            rows.extend(
                current
                    .iter()
                    .map(|row| translate(row, &alphas, &index, h, 0, w)),
            );
            rows.extend(
                current
                    .iter()
                    .map(|row| translate(row, &alphas, &index, h, 1, w)),
            );
            basis = row_basis(rows, w);
        }
        Self {
            alphas,
            index,
            basis,
        }
    }

    fn key(&self, terms: &BTreeMap<(usize, usize), u64>, w: u8) -> Vec<u64> {
        let mask = low_mask(w);
        let mut coefficients = vec![0u64; self.alphas.len()];
        for ((i, j), coefficient) in terms {
            for r in 0..=*i {
                let sr = stirling(*i, r);
                if sr == 0 {
                    continue;
                }
                for s in 0..=*j {
                    let ss = stirling(*j, s);
                    if ss == 0 {
                        continue;
                    }
                    let Some(k) = self.index.get(&(r, s)).copied() else {
                        continue;
                    };
                    coefficients[k] = coefficients[k]
                        .wrapping_add(coefficient.wrapping_mul(sr).wrapping_mul(ss))
                        & mask;
                }
            }
        }
        self.basis
            .iter()
            .map(|row| {
                row.iter()
                    .zip(&coefficients)
                    .fold(0u64, |sum, (a, b)| sum.wrapping_add(a.wrapping_mul(*b)))
                    & mask
            })
            .collect()
    }
}

impl CellMonomial {
    fn support(&self) -> Vec<usize> {
        self.0.iter().map(|(i, _)| *i).collect()
    }

    fn exponent(&self, index: usize) -> usize {
        for (i, exponent) in &self.0 {
            if *i == index {
                return *exponent;
            }
            if *i > index {
                break;
            }
        }
        0
    }

    fn beta(&self) -> usize {
        let mut exponents = self.0.iter().map(|(_, e)| *e).collect::<Vec<_>>();
        exponents.sort_unstable_by(|a, b| b.cmp(a));
        exponents.into_iter().enumerate().map(|(i, e)| i * e).sum()
    }

    fn mul(&self, other: &Self) -> Self {
        let mut out = self.0.iter().copied().collect::<BTreeMap<_, _>>();
        for (index, exponent) in &other.0 {
            *out.entry(*index).or_default() += *exponent;
        }
        Self(out.into_iter().filter(|(_, e)| *e != 0).collect())
    }
}

fn minterm_set(e: &Expr, vars: &[VarId]) -> Option<BTreeSet<usize>> {
    if vars.len() > RUMBA_TD_LIMIT {
        return None;
    }
    let positions = vars
        .iter()
        .copied()
        .enumerate()
        .map(|(i, v)| (v, i))
        .collect::<BTreeMap<_, _>>();
    let universe = (0..(1usize << vars.len())).collect::<BTreeSet<_>>();
    let mut memo = BTreeMap::<Expr, BTreeSet<usize>>::new();

    fn go(
        e: &Expr,
        positions: &BTreeMap<VarId, usize>,
        universe: &BTreeSet<usize>,
        memo: &mut BTreeMap<Expr, BTreeSet<usize>>,
    ) -> Option<BTreeSet<usize>> {
        if let Some(hit) = memo.get(e) {
            return Some(hit.clone());
        }
        let result = match e {
            Expr::Var(v) => {
                let i = *positions.get(v)?;
                universe
                    .iter()
                    .copied()
                    .filter(|cell| ((cell >> i) & 1) != 0)
                    .collect()
            }
            Expr::Const(0) => BTreeSet::new(),
            Expr::Const(u64::MAX) => universe.clone(),
            Expr::Const(_) => return None,
            Expr::Not(child) => {
                let child = go(child, positions, universe, memo)?;
                universe.difference(&child).copied().collect()
            }
            Expr::And(children) => {
                let mut out = universe.clone();
                for child in children {
                    let child = go(child, positions, universe, memo)?;
                    out = out.intersection(&child).copied().collect();
                }
                out
            }
            Expr::Or(children) => {
                let mut out = BTreeSet::new();
                for child in children {
                    let child = go(child, positions, universe, memo)?;
                    out.extend(child);
                }
                out
            }
            Expr::Xor(children) => {
                let mut out = BTreeSet::new();
                for child in children {
                    let child = go(child, positions, universe, memo)?;
                    out = out.symmetric_difference(&child).copied().collect();
                }
                out
            }
            _ => return None,
        };
        memo.insert(e.clone(), result.clone());
        Some(result)
    }

    go(e, &positions, &universe, &mut memo)
}

fn cell_affine_from_minterms(on: &BTreeSet<usize>, q: usize) -> CellPoly {
    let d = 1usize << q;
    let mut out = BTreeMap::new();
    if on.contains(&0) {
        out.insert(CellMonomial::default(), u64::MAX);
        for cell in 1..d {
            if !on.contains(&cell) {
                out.insert(CellMonomial(vec![(cell - 1, 1)]), u64::MAX);
            }
        }
        return out;
    }
    for cell in on {
        if *cell != 0 {
            out.insert(CellMonomial(vec![(*cell - 1, 1)]), 1);
        }
    }
    out
}

fn cell_add(a: &CellPoly, b: &CellPoly, w: u8) -> CellPoly {
    let mask = low_mask(w);
    let mut out = a.clone();
    for (monomial, coefficient) in b {
        let value = out
            .get(monomial)
            .copied()
            .unwrap_or(0)
            .wrapping_add(*coefficient)
            & mask;
        if value == 0 {
            out.remove(monomial);
        } else {
            out.insert(monomial.clone(), value);
        }
    }
    out
}

fn cell_scale(a: &CellPoly, c: u64, w: u8) -> CellPoly {
    let mask = low_mask(w);
    a.iter()
        .filter_map(|(monomial, coefficient)| {
            let value = coefficient.wrapping_mul(c) & mask;
            (value != 0).then(|| (monomial.clone(), value))
        })
        .collect()
}

fn cell_mul(a: &CellPoly, b: &CellPoly, w: u8) -> Result<CellPoly> {
    let mask = low_mask(w);
    let mut out = BTreeMap::new();
    for (left, lc) in a {
        for (right, rc) in b {
            let monomial = left.mul(right);
            let coefficient = lc.wrapping_mul(*rc) & mask;
            if coefficient == 0 {
                continue;
            }
            let support = monomial.support();
            if support.len() >= 3 {
                if v2(coefficient) as usize + monomial.beta() >= w as usize {
                    continue;
                }
                return Err(Unsupported);
            }
            let value = out
                .get(&monomial)
                .copied()
                .unwrap_or(0u64)
                .wrapping_add(coefficient)
                & mask;
            if value == 0 {
                out.remove(&monomial);
            } else {
                out.insert(monomial, value);
            }
        }
    }
    Ok(out)
}

fn cell_poly(e: &Expr, vars: &[VarId], w: u8) -> Result<CellPoly> {
    fn go(
        e: &Expr,
        vars: &[VarId],
        w: u8,
        memo: &mut BTreeMap<Expr, CellPoly>,
    ) -> Result<CellPoly> {
        if let Some(hit) = memo.get(e) {
            return Ok(hit.clone());
        }
        let mask = low_mask(w);
        let result = if let Some(on) = minterm_set(e, vars) {
            cell_affine_from_minterms(&on, vars.len())
                .into_iter()
                .filter_map(|(m, c)| {
                    let c = c & mask;
                    (c != 0).then_some((m, c))
                })
                .collect()
        } else {
            match e {
                Expr::Const(c) => [(CellMonomial::default(), *c & mask)].into_iter().collect(),
                Expr::Add(children) => {
                    let mut out = BTreeMap::new();
                    for child in children {
                        out = cell_add(&out, &go(child, vars, w, memo)?, w);
                    }
                    out
                }
                Expr::Scale(c, child) => cell_scale(&go(child, vars, w, memo)?, *c, w),
                Expr::Mul(children) => {
                    let mut out = [(CellMonomial::default(), 1)].into_iter().collect();
                    for child in children {
                        out = cell_mul(&out, &go(child, vars, w, memo)?, w)?;
                    }
                    out
                }
                Expr::Not(child) => {
                    let base = [(CellMonomial::default(), mask)].into_iter().collect();
                    let child = cell_scale(&go(child, vars, w, memo)?, u64::MAX, w);
                    cell_add(&base, &child, w)
                }
                _ => return Err(Unsupported),
            }
        };
        memo.insert(e.clone(), result.clone());
        Ok(result)
    }

    go(e, vars, w, &mut BTreeMap::new())
}

fn univar_key(terms: &BTreeMap<usize, u64>, w: u8) -> Vec<u64> {
    let mut out = Vec::new();
    let max_degree = terms.keys().copied().max().unwrap_or(0);
    for j in 0..=max_degree {
        let shift = v2_factorial(j);
        if shift >= w as usize {
            break;
        }
        let bits = w - shift as u8;
        let mut sum = 0u64;
        for (n, coefficient) in terms.range(j..) {
            sum = sum.wrapping_add(coefficient.wrapping_mul(stirling(*n, j)));
        }
        out.push(sum & low_mask(bits));
    }
    while out.last().is_some_and(|x| *x == 0) {
        out.pop();
    }
    out
}

fn cell_expr(cell: usize, vars: &[VarId]) -> Expr {
    let mut terms = Vec::with_capacity(vars.len());
    for (i, var) in vars.iter().copied().enumerate() {
        let expr = Expr::Var(var);
        terms.push(if ((cell >> i) & 1) != 0 { expr } else { !expr });
    }
    if terms.is_empty() {
        Expr::Const(u64::MAX)
    } else if terms.len() == 1 {
        terms.pop().unwrap_or(Expr::Const(u64::MAX))
    } else {
        Expr::And(terms)
    }
}

fn fall_expr(x: &Expr, degree: usize) -> Expr {
    if degree == 0 {
        return Expr::Const(1);
    }
    Expr::Mul(
        (0..degree)
            .map(|t| x.clone() - Expr::Const(t as u64))
            .collect(),
    )
}

fn coefficient_mask(w: u8, degree: usize) -> u64 {
    let shift = v2_factorial(degree).min(w as usize);
    low_mask(w - shift as u8)
}

fn translate_key(key: &[u64], h: u64, w: u8) -> Vec<u64> {
    let mut out = Vec::new();
    for r in 0..key.len() {
        let mask = coefficient_mask(w, r);
        let mut sum = 0u64;
        for j in r..key.len() {
            sum = sum.wrapping_add(
                key[j]
                    .wrapping_mul(binomial(j, r))
                    .wrapping_mul(falling(h, j - r)),
            );
        }
        out.push(sum & mask);
    }
    while out.last().is_some_and(|x| *x == 0) {
        out.pop();
    }
    out
}

fn render_univar(key: &[u64], cell: Expr, w: u8) -> Expr {
    if key.is_empty() {
        return Expr::zero();
    }
    let translated = translate_key(key, 2, w);
    let n = key.len().max(translated.len());
    let periodic = (0..n).all(|j| {
        let left = key.get(j).copied().unwrap_or(0);
        let right = translated.get(j).copied().unwrap_or(0);
        left.wrapping_sub(right) & coefficient_mask(w, j) == 0
    });

    if periodic {
        let mask = low_mask(w);
        let f0 = key[0] & mask;
        let f1 = f0.wrapping_add(key.get(1).copied().unwrap_or(0)) & mask;
        let slope = f1.wrapping_sub(f0) & mask;
        let mut terms = Vec::new();
        if f0 != 0 {
            terms.push(Expr::Const(f0));
        }
        if slope != 0 {
            let bit = Expr::And(vec![cell, Expr::Const(1)]);
            terms.push(if slope == 1 {
                bit
            } else {
                Expr::scale(slope, bit)
            });
        }
        return match terms.len() {
            0 => Expr::zero(),
            1 => terms.pop().unwrap_or_else(Expr::zero),
            _ => Expr::Add(terms),
        };
    }

    let mut terms = Vec::new();
    for (degree, coefficient) in key.iter().copied().enumerate() {
        let bits = w.saturating_sub(v2_factorial(degree).min(w as usize) as u8);
        let coefficient = coefficient & coefficient_mask(w, degree);
        if coefficient == 0 {
            continue;
        }
        let factor = fall_expr(&cell, degree);
        terms.push(if coefficient == 1 {
            factor
        } else {
            Expr::scale(signed_residue(coefficient, bits), factor)
        });
    }
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Add(terms),
    }
}

fn project_additive(e: &Expr, w: u8) -> Result<Option<Expr>> {
    let mut vars = e.get_vars().into_iter().collect::<Vec<_>>();
    vars.sort_unstable();
    if vars.len() > RUMBA_TD_LIMIT {
        return Err(Unsupported);
    }
    let poly = cell_poly(e, &vars, w)?;
    let mask = low_mask(w);
    let mut groups = BTreeMap::<Vec<usize>, BTreeMap<CellMonomial, u64>>::new();
    let mut constant = 0u64;
    for (monomial, coefficient) in poly {
        let coefficient = coefficient & mask;
        if coefficient == 0 {
            continue;
        }
        let support = monomial.support();
        if support.is_empty() {
            constant = constant.wrapping_add(coefficient) & mask;
        } else {
            groups
                .entry(support)
                .or_default()
                .insert(monomial, coefficient);
        }
    }

    let mut univariate = BTreeMap::<usize, Vec<u64>>::new();
    let mut pair_module = None::<PairModule>;
    for (support, group) in groups {
        if support.len() == 1 {
            let i = support[0];
            let terms = group
                .iter()
                .map(|(m, c)| (m.exponent(i), *c))
                .collect::<BTreeMap<_, _>>();
            univariate.insert(i, univar_key(&terms, w));
            continue;
        }
        if support.len() != 2 {
            return Ok(None);
        }
        let i = support[0];
        let j = support[1];
        let terms = group
            .iter()
            .map(|(m, c)| ((m.exponent(i), m.exponent(j)), *c))
            .collect::<BTreeMap<_, _>>();
        let module = pair_module.get_or_insert_with(|| PairModule::new(w));
        if module.key(&terms, w).iter().any(|x| *x != 0) {
            return Ok(None);
        }
    }

    let mut out = Vec::new();
    if constant != 0 {
        out.push(Expr::Const(signed_residue(constant, w)));
    }
    for (cell, key) in univariate {
        if key.iter().all(|x| *x == 0) {
            continue;
        }
        let expr = render_univar(&key, cell_expr(cell + 1, &vars), w);
        match expr {
            Expr::Add(children) => out.extend(children),
            Expr::Const(0) => {}
            other => out.push(other),
        }
    }
    Ok(Some(match out.len() {
        0 => Expr::zero(),
        1 => out.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Add(out),
    }))
}

fn flatten_add<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Add(children) = e {
        for child in children {
            flatten_add(child, out);
        }
        return;
    }
    out.push(e);
}

fn split_coefficient(e: &Expr) -> (u64, Expr) {
    match e {
        Expr::Scale(c, child) => {
            let (d, core) = split_coefficient(child);
            (c.wrapping_mul(d), core)
        }
        Expr::Mul(children) => {
            let mut coefficient = 1u64;
            let mut factors = Vec::new();
            for child in children {
                let (c, core) = split_coefficient(child);
                coefficient = coefficient.wrapping_mul(c);
                factors.push(core);
            }
            (coefficient, Expr::Mul(factors))
        }
        _ => (1, e.clone()),
    }
}

fn render_terms(items: &[(u64, Expr)], mask: u64) -> Expr {
    let mut terms = Vec::new();
    for (coefficient, expr) in items {
        if *coefficient == 0 {
            continue;
        }
        terms.push(if *coefficient == 1 {
            expr.clone()
        } else {
            Expr::scale(*coefficient, expr.clone())
        });
    }
    let expr = match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Add(terms),
    };
    reduce_masked_plain(expr, mask)
}

fn project_dyadic(e: &Expr, k: u8, full_mask: u64) -> Result<Expr> {
    let mask = low_mask(k);
    let mut flattened = Vec::new();
    flatten_add(e, &mut flattened);
    let mut odd = Vec::new();
    let mut filtered = Vec::new();
    for term in flattened {
        let (coefficient, core) = split_coefficient(term);
        let coefficient = coefficient & mask;
        if coefficient == 0 {
            continue;
        }
        if v2(coefficient) == 0 {
            odd.push((coefficient, core));
        } else {
            filtered.push((coefficient, core));
        }
    }

    let mut out = odd;
    if !filtered.is_empty() {
        let Some(shift) = filtered.iter().map(|(c, _)| v2(*c)).min() else {
            return Ok(render_terms(&out, full_mask));
        };
        let divided = filtered
            .iter()
            .map(|(c, e)| (*c >> shift, e.clone()))
            .collect::<Vec<_>>();
        let rendered = render_terms(&divided, full_mask);
        match project_additive(&rendered, k - shift)? {
            Some(projected) => out.push((1u64 << shift, projected)),
            None => out.extend(filtered),
        }
    }
    Ok(render_terms(&out, full_mask))
}

fn cell_project(e: &Expr, k: u8, full_mask: u64) -> Option<Expr> {
    match project_additive(e, k) {
        Ok(Some(expr)) => return Some(expr),
        Ok(None) => {}
        Err(_) => return None,
    }
    project_dyadic(e, k, full_mask).ok()
}

// -----------------------------------------------------------------------------
// Low-demand propagation
// -----------------------------------------------------------------------------

fn has_bitwise(e: &Expr, memo: &mut BTreeMap<Expr, bool>) -> bool {
    if let Some(hit) = memo.get(e).copied() {
        return hit;
    }
    let hit = match e {
        Expr::And(_) | Expr::Or(_) | Expr::Xor(_) | Expr::Not(_) => true,
        Expr::Scale(_, child) => has_bitwise(child, memo),
        Expr::Add(children) | Expr::Mul(children) => {
            children.iter().any(|child| has_bitwise(child, memo))
        }
        Expr::Var(_) | Expr::Const(_) => false,
    };
    memo.insert(e.clone(), hit);
    hit
}

fn v2_lower(e: &Expr) -> u8 {
    match e {
        Expr::Const(c) => v2(*c),
        Expr::Scale(c, child) => v2(*c).saturating_add(v2_lower(child)).min(WORD_BITS),
        Expr::Mul(children) => children
            .iter()
            .fold(0u8, |sum, child| sum.saturating_add(v2_lower(child)))
            .min(WORD_BITS),
        Expr::Add(children) | Expr::And(children) | Expr::Or(children) | Expr::Xor(children) => {
            children.iter().map(v2_lower).min().unwrap_or(WORD_BITS)
        }
        Expr::Var(_) | Expr::Not(_) => 0,
    }
}

fn and_width(e: &Expr, k: u8) -> u8 {
    let Expr::And(children) = e else { return k };
    let mut constant = u64::MAX;
    let mut seen = false;
    for child in children {
        if let Expr::Const(c) = child {
            constant &= *c;
            seen = true;
        }
    }
    if !seen {
        return k;
    }
    let x = constant & low_mask(k);
    if x == 0 || x == low_mask(k) || x & x.wrapping_add(1) != 0 {
        return k;
    }
    64 - x.leading_zeros() as u8
}

fn child_widths(e: &Expr, k: u8) -> Vec<u8> {
    match e {
        Expr::Var(_) | Expr::Const(_) => Vec::new(),
        Expr::Not(_) => vec![k],
        Expr::And(children) => {
            let width = and_width(e, k);
            children
                .iter()
                .map(|child| {
                    if matches!(child, Expr::Const(_)) {
                        k
                    } else {
                        width
                    }
                })
                .collect()
        }
        Expr::Scale(c, _) => vec![k.saturating_sub(v2(*c))],
        Expr::Or(children) | Expr::Xor(children) | Expr::Add(children) => vec![k; children.len()],
        Expr::Mul(children) => {
            let valuations = children.iter().map(v2_lower).collect::<Vec<_>>();
            let total = valuations
                .iter()
                .copied()
                .fold(0u8, |sum, v| sum.saturating_add(v))
                .min(WORD_BITS);
            children
                .iter()
                .zip(valuations)
                .map(|(child, v)| {
                    if matches!(child, Expr::Const(_)) {
                        k
                    } else {
                        k.saturating_sub(total.saturating_sub(v))
                    }
                })
                .collect()
        }
    }
}

fn rebuild(e: &Expr, children: Vec<Expr>) -> Expr {
    match e {
        Expr::Var(_) | Expr::Const(_) => e.clone(),
        Expr::Not(_) => Expr::Not(Box::new(
            children.into_iter().next().unwrap_or_else(Expr::zero),
        )),
        Expr::Scale(c, _) => Expr::Scale(
            *c,
            Box::new(children.into_iter().next().unwrap_or_else(Expr::zero)),
        ),
        Expr::And(_) => Expr::And(children),
        Expr::Or(_) => Expr::Or(children),
        Expr::Xor(_) => Expr::Xor(children),
        Expr::Add(_) => Expr::Add(children),
        Expr::Mul(_) => Expr::Mul(children),
    }
}

pub(super) fn project_low(e: &Expr, k: u8, full_mask: u64) -> Expr {
    fn go(e: &Expr, width: u8, full_mask: u64, bitwise: &mut BTreeMap<Expr, bool>) -> Expr {
        let active = width > 0 && width < WORD_BITS && has_bitwise(e, bitwise);
        if active {
            if let Some(projected) = shadow_project(e, width) {
                if better(&projected, e) {
                    return projected;
                }
            }
        }
        let widths = child_widths(e, width);
        if widths.is_empty() {
            return e.clone();
        }
        let contracted = widths.iter().any(|child_width| *child_width < width);
        if active && !contracted {
            if let Some(projected) = cell_project(e, width, full_mask) {
                if better(&projected, e) {
                    return projected;
                }
            }
        }

        let source_children: Vec<&Expr> = match e {
            Expr::Not(child) | Expr::Scale(_, child) => vec![child],
            Expr::And(children)
            | Expr::Or(children)
            | Expr::Xor(children)
            | Expr::Add(children)
            | Expr::Mul(children) => children.iter().collect(),
            Expr::Var(_) | Expr::Const(_) => Vec::new(),
        };
        let mut changed = false;
        let mut children = Vec::with_capacity(source_children.len());
        for (child, child_width) in source_children.into_iter().zip(widths) {
            let projected = go(child, child_width, full_mask, bitwise);
            changed |= projected != *child;
            children.push(projected);
        }
        let rebuilt = if changed {
            rebuild(e, children)
        } else {
            e.clone()
        };
        if !active || !contracted {
            return rebuilt;
        }
        if let Some(projected) = cell_project(&rebuilt, width, full_mask) {
            if better(&projected, &rebuilt) {
                return projected;
            }
        }
        rebuilt
    }

    go(e, k, full_mask, &mut BTreeMap::new())
}

pub(super) fn upper_bound(e: &Expr, full: u64) -> Option<u64> {
    fn go(e: &Expr, full: u64, memo: &mut BTreeMap<Expr, Option<u64>>) -> Option<u64> {
        if let Some(hit) = memo.get(e) {
            return *hit;
        }
        let result = match e {
            Expr::Const(c) => Some(*c & full),
            Expr::And(children) => children
                .iter()
                .filter_map(|child| go(child, full, memo))
                .min(),
            Expr::Or(children) | Expr::Xor(children) => {
                let bounds = children
                    .iter()
                    .map(|child| go(child, full, memo))
                    .collect::<Option<Vec<_>>>()?;
                let bits = bounds
                    .iter()
                    .map(|b| 64 - b.leading_zeros() as u8)
                    .max()
                    .unwrap_or(0);
                Some(low_mask(bits))
            }
            Expr::Scale(c, child) => {
                let bound = go(child, full, memo)?;
                c.checked_mul(bound).filter(|value| *value <= full)
            }
            Expr::Mul(children) => {
                let mut product = 1u64;
                for child in children {
                    let bound = go(child, full, memo)?;
                    product = product.checked_mul(bound)?;
                    if product > full {
                        return None;
                    }
                }
                Some(product)
            }
            Expr::Add(children) => {
                let mut sum = 0u64;
                for child in children {
                    let bound = go(child, full, memo)?;
                    sum = sum.checked_add(bound)?;
                    if sum > full {
                        return None;
                    }
                }
                Some(sum)
            }
            Expr::Var(_) | Expr::Not(_) => None,
        };
        memo.insert(e.clone(), result);
        result
    }

    go(e, full, &mut BTreeMap::new())
}

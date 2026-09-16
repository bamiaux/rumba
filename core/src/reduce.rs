use rustc_hash::FxHashMap as HashMap;

use crate::{expr::Expr, varint::make_mask};

mod low_prefix;

/// Distributes an expression
fn distribute<F>(
    v: &mut [Expr],
    self_cst: fn(Vec<Expr>) -> Expr,
    child_cst: fn(Vec<Expr>) -> Expr,
    extractor: F,
) -> Option<Expr>
where
    F: Fn(&mut [Expr]) -> Option<(usize, Vec<Expr>)>,
{
    if let Some((idx, terms)) = extractor(v) {
        let mut distributed_terms: Vec<Expr> = Vec::with_capacity(terms.len());

        for term in terms {
            let mut copy = v.to_vec();
            copy[idx] = term;
            distributed_terms.push(self_cst(copy));
        }

        return Some(child_cst(distributed_terms));
    }

    None
}

macro_rules! distribute {
    ($self:ident, $child:ident, $v:expr) => {
        distribute($v, Expr::$self, Expr::$child, distribute!(@extractor $child))
    };

    (@extractor $variant:ident) => {
        |v: &mut [Expr]| {
            v.iter_mut().enumerate().find_map(|(i, e)| match e {
                Expr::$variant(v) => Some((i, std::mem::take(v))),
                _ => None,
            })
        }
    };
}

/// Remove pairs of elements in a vector
fn remove_pairs(v: Vec<Expr>) -> Vec<Expr> {
    let mut reduced = Vec::with_capacity(v.len());
    let mut iter = v.into_iter().peekable();

    while let Some(current) = iter.next() {
        let mut count = 1;

        while let Some(next) = iter.peek() {
            if *next == current {
                count += 1;
                iter.next();
                continue;
            }

            break;
        }

        if (count & 1) == 1 {
            reduced.push(current);
        }
    }

    reduced
}

/// Deduplicates a list
fn dedupe(mut v: Vec<Expr>) -> Vec<Expr> {
    // TODO: Somehow using hashset here makes the program crash
    // We need a better comparison function
    v.sort();
    v.dedup();
    v
}

struct Reducer {
    mask: u64,
    allow_low_prefix: bool,
}

fn is_low_mask(mask: u64) -> bool {
    mask != 0 && mask & mask.wrapping_add(1) == 0
}

fn is_bit_valued(e: &Expr) -> bool {
    match e {
        Expr::Const(c) => *c <= 1,
        Expr::Not(child) => is_bit_valued(child),
        Expr::And(children) => {
            children.iter().any(|child| matches!(child, Expr::Const(1)))
                || children.iter().all(is_bit_valued)
        }
        Expr::Or(children) | Expr::Xor(children) => children.iter().all(is_bit_valued),
        _ => false,
    }
}

/// The result enum for a flattening handler
#[must_use]
enum FlattenResult {
    Expr(Expr),
    Vec(Vec<Expr>),
    None,
}

impl Reducer {
    /// Flattens an expression by concatenating any same typed child expression into it
    fn flatten<F>(&self, v: Vec<Expr>, mut handler: F) -> Vec<Expr>
    where
        F: FnMut(Expr) -> FlattenResult,
    {
        let mut flat = Vec::with_capacity(v.len());

        let mut stack: Vec<_> = v.into_iter().map(|e| self.reduce_masked(e)).collect();

        while let Some(e) = stack.pop() {
            match handler(e) {
                FlattenResult::Vec(mut v) => {
                    stack.append(&mut v);
                }

                FlattenResult::Expr(e) => flat.push(e),

                FlattenResult::None => {}
            }
        }

        flat
    }

    pub fn group_terms(&self, exprs: Vec<Expr>) -> Expr {
        let initial_len = exprs.len();

        let mut map =
            HashMap::<Expr, u64>::with_capacity_and_hasher(initial_len, Default::default());

        for e in exprs.into_iter() {
            if let Expr::Scale(c, e) = e {
                let count = map.entry(*e).or_insert(0);
                *count = count.wrapping_add(c);
            } else {
                let count = map.entry(e).or_insert(0);
                *count = count.wrapping_add(1);
            }
        }

        let mut out = Vec::with_capacity(map.len());

        for (e, mut count) in map {
            count &= self.mask;

            if count == 0 {
                continue;
            }

            out.push(Expr::scale(count, e));
        }

        out.sort();

        if initial_len > out.len() {
            self.reduce_masked(Expr::Add(out))
        } else {
            Expr::Add(out)
        }
    }

    /// Reduces a not node
    fn reduce_not(&self, expr: Expr) -> Expr {
        match expr {
            // !!x = x
            Expr::Not(x) => self.reduce_masked(*x),

            Expr::Const(v) => Expr::Const(!v),

            // De Morgan's laws
            Expr::And(exprs) => {
                Expr::Or(exprs.into_iter().map(|e| self.reduce_masked(!e)).collect())
            }

            // De Morgan's laws
            Expr::Or(exprs) => {
                Expr::And(exprs.into_iter().map(|e| self.reduce_masked(!e)).collect())
            }

            _ => !self.reduce_masked(expr),
        }
    }

    /// Reduces a scale node
    fn reduce_scale(&self, scale: u64, expr: Expr) -> Expr {
        let scale = scale & self.mask;

        match scale {
            0 => Expr::zero(),

            1 => self.reduce_masked(expr),

            _ => match self.reduce_masked(expr) {
                Expr::Const(c2) => Expr::Const(scale.wrapping_mul(c2) & self.mask),

                Expr::Scale(c2, e) => {
                    let c = scale.wrapping_mul(c2) & self.mask;
                    match c {
                        0 => Expr::zero(),
                        1 => *e,
                        c => c * *e,
                    }
                }

                Expr::Add(sum) => {
                    // TODO: the arith reduce is unnecessary here
                    self.reduce_masked(Expr::Add(sum.into_iter().map(|e| scale * e).collect()))
                }

                other => scale * other,
            },
        }
    }

    /// Reduces a and node
    fn reduce_and(&self, mut exprs: Vec<Expr>) -> Expr {
        if self.allow_low_prefix {
            let mut prefix = self.mask;
            let mut seen = false;
            for expr in &exprs {
                if let Expr::Const(c) = expr {
                    prefix &= *c & self.mask;
                    seen = true;
                }
            }

            if seen && prefix != self.mask && is_low_mask(prefix) {
                let payloads = exprs
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Const(_)))
                    .cloned()
                    .collect::<Vec<_>>();
                if !payloads.is_empty() {
                    let payload = if payloads.len() == 1 {
                        payloads[0].clone()
                    } else {
                        Expr::And(payloads)
                    };
                    let projected = low_prefix::project_low(&payload, prefix.count_ones() as u8);
                    if matches!(
                        low_prefix::upper_bound(&projected, self.mask),
                        Some(bound) if bound <= prefix
                    ) {
                        return self.reduce_masked_plain(projected);
                    }
                    if projected != payload {
                        exprs = vec![projected, Expr::Const(prefix)];
                    }
                }
            }
        }

        let mut c: u64 = self.mask;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::And(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c &= v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c &= self.mask;

        if c == 0 {
            return Expr::zero();
        }

        // The result of `payload & (2^k - 1)` only depends on the low k bits
        // of payload. Reduce those operands in the smaller ring before the
        // historical flattening/distribution logic. This preserves the
        // explicit mask while allowing identities such as
        // `(x & 5) * (x & 5) & 1 = x & 1` to become visible.
        if c != self.mask && is_low_mask(c) {
            let local = Self {
                mask: c,
                allow_low_prefix: false,
            };
            flat = flat.into_iter().map(|e| local.reduce_masked(e)).collect();
        }

        if c != self.mask {
            flat.push(Expr::Const(c));
        }

        if let Some(distributed) = distribute!(And, Xor, &mut flat) {
            return self.reduce_masked(distributed);
        }

        flat = dedupe(flat);

        match flat.len() {
            0 => Expr::Const(u64::MAX),
            1 => flat.remove(0),
            _ => Expr::And(flat),
        }
    }

    fn reduce_or(&self, exprs: Vec<Expr>) -> Expr {
        let mut c: u64 = 0;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Or(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c |= v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c &= self.mask;

        if c != 0 {
            flat.push(Expr::Const(c));
        }

        if let Some(distributed) = distribute!(Or, And, &mut flat) {
            return self.reduce_masked(distributed);
        }

        flat = dedupe(flat);

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.remove(0),
            _ => Expr::Or(flat),
        }
    }

    fn reduce_xor(&self, exprs: Vec<Expr>) -> Expr {
        let mut c: u64 = 0;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Xor(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c ^= v;
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c &= self.mask;

        if c != 0 {
            flat.push(Expr::Const(c));
        }

        flat = remove_pairs(flat);

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.remove(0),
            _ => Expr::Xor(flat),
        }
    }

    fn reduce_add(&self, exprs: Vec<Expr>) -> Expr {
        // Used with dynamic masking
        if self.mask == 1 {
            return self.reduce_xor(exprs);
        }

        let mut c: u64 = 0;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Add(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c.wrapping_add(v);
                FlattenResult::None
            }
            _ => FlattenResult::Expr(e),
        });

        c &= self.mask;

        if c != 0 {
            flat.push(Expr::Const(c));
        }

        match flat.len() {
            0 => Expr::zero(),
            1 => flat.remove(0),
            _ => self.group_terms(flat),
        }
    }

    fn reduce_mul(&self, exprs: Vec<Expr>) -> Expr {
        // Used with dynamic masking
        if self.mask == 1 {
            return self.reduce_and(exprs);
        }

        let mut c: u64 = 1;

        let mut flat = self.flatten(exprs, |e| match e {
            Expr::Mul(v) => FlattenResult::Vec(v),
            Expr::Const(v) => {
                c = c.wrapping_mul(v);
                FlattenResult::None
            }
            Expr::Scale(s, v) => {
                c = c.wrapping_mul(s);
                FlattenResult::Expr(*v)
            }
            _ => FlattenResult::Expr(e),
        });

        c &= self.mask;

        if c == 0 {
            return Expr::zero();
        }

        if let Some(distributed) = distribute!(Mul, Add, &mut flat) {
            return self.reduce_masked(Expr::scale(c, distributed));
        }

        // A bit-valued factor is idempotent under multiplication. This is a
        // local Boolean law and is valid at every surrounding word width.
        if flat.len() >= 2
            && flat.windows(2).all(|pair| pair[0] == pair[1])
            && is_bit_valued(&flat[0])
        {
            return flat.remove(0);
        }

        flat.sort();

        match flat.len() {
            0 => Expr::Const(c),
            1 => Expr::scale(c, flat.remove(0)),
            _ => Expr::scale(c, Expr::Mul(flat)),
        }
    }

    fn reduce_masked(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Var(_) => expr,

            Expr::Const(c) => Expr::Const(c & self.mask),

            Expr::Not(expr) => self.reduce_not(*expr),

            Expr::Scale(c, e) => self.reduce_scale(c, *e),

            Expr::And(exprs) => self.reduce_and(exprs),

            Expr::Or(exprs) => self.reduce_or(exprs),

            Expr::Xor(exprs) => self.reduce_xor(exprs),

            Expr::Add(exprs) => self.reduce_add(exprs),

            Expr::Mul(exprs) => self.reduce_mul(exprs),
        }
    }

    fn reduce_masked_plain(&self, expr: Expr) -> Expr {
        Self {
            mask: self.mask,
            allow_low_prefix: false,
        }
        .reduce_masked(expr)
    }

    // fn reduce_(&self, expr: Expr) -> Expr {
    //     let old = expr.clone();
    //     let res = self.reduce_(expr);

    //     if let Err((v, v1, v2)) = old.sem_equal_masked(&res, self.mask, 500) {
    //         println!("Semantic error {} vs {}", old, res);
    //         println!("Semantic error {} vs {}", v1, v2);
    //         println!("{}\n\n", old.symbol(false));
    //     }
    //     res
    // }
}

impl Expr {
    pub(crate) fn reduce_masked(self, mask: u64) -> Self {
        Reducer {
            mask,
            allow_low_prefix: true,
        }
        .reduce_masked(self)
    }

    /// Canonicalizes the expression on `n` bits (constant folding, flattening
    /// and normalization), without attempting MBA simplification.
    pub fn reduce(self, n: u8) -> Self {
        self.reduce_masked(make_mask(n))
    }
}

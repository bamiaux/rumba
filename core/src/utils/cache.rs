//! Memoization of solved linear MBAs.
//!
//! The solver reaches its cache at exactly one point: a [`get`](LinearCache::get),
//! and on a miss an [`insert`](LinearCache::insert), with the expensive solve
//! running *between* them. Two implementations are provided: [`LocalCache`] for a
//! single-threaded caller, and [`MbaCache`] for one shared across threads.

use std::{cell::RefCell, collections::HashMap, sync::Mutex};

use crate::expr::Expr;

/// A memo of solved linear MBAs.
///
/// The solver reaches its cache at exactly one point (`MBASolver::solve_linear`):
/// a [`get`](LinearCache::get), and on a miss an [`insert`](LinearCache::insert),
/// with the expensive solve running *between* them. Two implementations are
/// provided: [`LocalCache`] for a single-threaded caller, and [`MbaCache`] for one
/// shared across threads.
///
/// [`get`](LinearCache::get) hands back an owned `Expr` rather than a guard or a
/// borrow. That is deliberate: the solver recurses into itself (`hide_in_var`
/// re-enters `simplify_mba_inner` with this same cache), and neither [`RefCell`]
/// nor [`Mutex`] tolerates a live guard across such a call — one panics, the
/// other deadlocks. Returning owned values makes that unrepresentable.
pub trait LinearCache {
    /// The memoized solution for `e`, tallying the lookup as a hit or a miss.
    fn get(&self, e: &Expr) -> Option<Expr>;

    /// Memoize `solved` as the solution for `e`.
    fn insert(&self, e: Expr, solved: Expr);
}

/// A [`LinearCache`] for one thread: no locking, no atomics.
///
/// This is what [`simplify_mba`](crate::simplify::simplify_mba) uses. Accessing it
/// costs a borrow-flag check, so a single-threaded caller pays nothing for a
/// sharing capability it does not use. Not [`Sync`] — use [`MbaCache`] to share one
/// across threads.
///
/// Entries are stored contiguously and searched by structural equality. The
/// short-lived cache of one simplification normally has few entries, making
/// this cheaper than recursively hashing every query. Lookup is linear in the
/// number of entries; [`MbaCache`] retains hash lookup for reuse across calls.
#[derive(Debug, Default)]
pub struct LocalCache {
    entries: RefCell<Vec<(Expr, Expr)>>,
}

impl LocalCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LinearCache for LocalCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        self.entries
            .borrow()
            .iter()
            .find(|(key, _)| key == e)
            .map(|(_, value)| value.clone())
    }

    fn insert(&self, e: Expr, solved: Expr) {
        let mut entries = self.entries.borrow_mut();
        if let Some((_, value)) = entries.iter_mut().find(|(key, _)| key == &e) {
            *value = solved;
        } else {
            entries.push((e, solved));
        }
    }
}

/// A [`LinearCache`] shareable across threads, and across calls to
/// [`simplify_mba_cached`](crate::simplify::simplify_mba_cached). This is what
/// the public [`SimplifyCache`](crate::simplify::SimplifyCache) wraps.
///
/// Critical sections are one hash-map operation each — the solve itself runs
/// outside the lock — so contention stays low even with many workers.
#[derive(Debug, Default)]
pub struct MbaCache {
    entries: Mutex<HashMap<Expr, Expr>>,
}

impl LinearCache for MbaCache {
    fn get(&self, e: &Expr) -> Option<Expr> {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .get(e)
            .cloned()
    }

    fn insert(&self, e: Expr, solved: Expr) {
        self.entries
            .lock()
            .expect("MBA cache mutex poisoned")
            .insert(e, solved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_cache(cache: &impl LinearCache) {
        let key = |i| Expr::Add(vec![Expr::Var(0.into()), Expr::Const(i)]);
        assert_eq!(cache.get(&key(0)), None);
        for i in 0..256 {
            cache.insert(key(i), Expr::Const(i + 1));
        }
        // Equal keys replace their value; nearby structural keys stay distinct.
        for i in (0..256).step_by(7) {
            cache.insert(key(i), Expr::Var((i as usize + 1).into()));
        }
        for i in 0..256 {
            let expected = if i % 7 == 0 {
                Expr::Var((i as usize + 1).into())
            } else {
                Expr::Const(i + 1)
            };
            assert_eq!(cache.get(&key(i)), Some(expected));
        }
        // A returned tree is owned, so it can outlive subsequent cache writes.
        let previous = cache.get(&key(1)).unwrap();
        cache.insert(key(1), Expr::zero());
        assert_eq!(previous, Expr::Const(2));
        assert_eq!(cache.get(&key(1)), Some(Expr::zero()));
        assert_eq!(cache.get(&key(256)), None);
    }

    #[test]
    fn cache_implementations_preserve_structural_keys_and_replacement() {
        check_cache(&LocalCache::new());
        check_cache(&MbaCache::default());
    }
}

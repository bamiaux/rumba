from pathlib import Path
import sys
root = Path(sys.argv[1])
labels = ['total','reduce','make_polynomial','solve_linear','truth_table_masked','make_conjunction_sum','make_bitwise','is_linear_bitwise','group_terms','prettify','projector_defect','hide_in_var','merge_hidden','restore_hidden','solve','reduce_vars','is_linear','is_bitwise','eval_nodes']
module = '''use std::{cell::RefCell, time::Instant};
const N: usize = 19;
struct State { calls: [u64; N], inclusive: [u128; N], exclusive: [u128; N], stack: Vec<u128>, hist: [[u64; 65]; 4] }
thread_local! { static STATE: RefCell<State> = RefCell::new(State { calls: [0; N], inclusive: [0; N], exclusive: [0; N], stack: Vec::with_capacity(256), hist: [[0; 65]; 4] }); }
pub struct Scope { id: usize, start: Instant }
pub fn enter(id: usize) -> Scope {
    STATE.with(|s| { let mut s = s.borrow_mut(); s.calls[id] += 1; s.stack.push(0); });
    Scope { id, start: Instant::now() }
}
impl Drop for Scope { fn drop(&mut self) {
    let elapsed = self.start.elapsed().as_nanos();
    STATE.with(|s| { let mut s = s.borrow_mut(); let children = s.stack.pop().unwrap(); s.inclusive[self.id] += elapsed; s.exclusive[self.id] += elapsed.saturating_sub(children); if let Some(parent) = s.stack.last_mut() { *parent += elapsed; } });
} }
pub fn count(id: usize) { STATE.with(|s| s.borrow_mut().calls[id] += 1); }
pub fn hist(id: usize, value: usize) { STATE.with(|s| s.borrow_mut().hist[id][value.min(64)] += 1); }
pub fn dump() { STATE.with(|s| { let s = s.borrow();
    for (i, name) in LABELS.iter().enumerate() { println!("{name} calls={} inclusive_ms={:.3} exclusive_ms={:.3}", s.calls[i], s.inclusive[i] as f64 / 1e6, s.exclusive[i] as f64 / 1e6); }
    for (i, h) in s.hist.iter().enumerate() { println!("hist{i}={h:?}"); }
}); }
'''.replace('LABELS', repr(labels).replace("'",'"'))
(root/'core/src/phase3_profile.rs').write_text(module)
p=root/'core/src/lib.rs'; p.write_text(p.read_text()+'\n#[doc(hidden)]\npub mod phase3_profile;\n')
def inject(file, marker, text):
 p=root/file; s=p.read_text()
 if 'fn solve(&mut self' in marker and 'fn solve_reduced(&mut self' in s: marker=marker.replace('fn solve(', 'fn solve_reduced(')
 if 'pub fn group_terms' in marker and s.count(marker)==0: marker=marker.replace('exprs: Vec', 'mut exprs: Vec')
 assert s.count(marker)==1, (marker,s.count(marker))
 p.write_text(s.replace(marker,marker+'\n'+text,1))
for file, marker, label in [
 ('simplify.rs','fn simplify_mba_with_cache<C: LinearCache>(cache: &C, e: Expr, n: u8) -> Result<Expr, SolveError> {','total'),
 ('reduce.rs','fn reduce_masked(&self, expr: Expr) -> Expr {','reduce'),
 ('simplify.rs','fn make_polynomial(&mut self, e: Expr) -> Result<Expr, SolveError> {','make_polynomial'),
 ('simplify.rs','fn solve_linear(&mut self, e: Expr, from_poly: bool) -> Result<Expr, SolveError> {','solve_linear'),
 ('expr.rs','pub(crate) fn truth_table_masked(&self, t: usize, mask: u64) -> Vec<u64> {','truth_table_masked'),
 ('simplify.rs','fn make_conjunction_sum(&self, mut signature: Vec<u64>, t: usize) -> Expr {','make_conjunction_sum'),
 ('simplify.rs','fn make_bitwise(&mut self, e: Expr, mut mask: u64) -> Result<Expr, SolveError> {','make_bitwise'),
 ('simplify.rs','fn is_linear_bitwise(&self, l: Expr, mask: u64) -> Option<Expr> {','is_linear_bitwise'),
 ('reduce.rs','pub fn group_terms(&self, exprs: Vec<Expr>) -> Expr {','group_terms'),
 ('prettify.rs','pub(crate) fn prettify(e: Expr, n: u8) -> Expr {','prettify'),
 ('simplify/projector_defect.rs',"pub(super) fn close<C: LinearCache>(s: &mut MBASolver<'_, C>, root: Expr) -> Expr {",'projector_defect'),
 ('simplify.rs','fn hide_in_var(&mut self, e: Expr, mask: u64) -> Result<Expr, SolveError> {','hide_in_var'),
 ('simplify/merge_hidden.rs','pub(super) fn merge_equal_hidden_components(&mut self, e: Expr) -> HiddenMergeResult {','merge_hidden'),
 ('simplify.rs','fn poly_to_nonpoly(&self, e: Expr) -> Expr {','restore_hidden'),
 ('simplify.rs','fn solve(&mut self, e: Expr) -> Result<Expr, SolveError> {','solve'),
]: inject('core/src/'+file, marker, f'    let _profile = crate::phase3_profile::enter({labels.index(label)});')
for file,marker,label in [('simplify.rs','fn reduce_vars(e: Expr, var_map: &mut BiMap<VarId, VarId>, t: &mut usize) -> Expr {','reduce_vars'),('simplify.rs','fn is_linear(&self, e: &Expr) -> bool {','is_linear'),('simplify.rs','fn is_bitwise(e: &Expr, mask: u64) -> bool {','is_bitwise'),('expr.rs','pub(crate) fn eval_bits(&self, vars: &[u64]) -> VarInt {','eval_nodes')]:
 inject('core/src/'+file,marker,f'crate::phase3_profile::count({labels.index(label)});')
inject('core/src/simplify.rs','let e = reduce_vars(e, &mut var_map, &mut t);','crate::phase3_profile::hist(0, t); crate::phase3_profile::hist(1, e.size());')
inject('core/src/reduce.rs','let initial_len = exprs.len();','crate::phase3_profile::hist(2, initial_len); crate::phase3_profile::hist(3, usize::from(exprs.is_sorted()));')
p=root/'core/examples/phase3_probe.rs'; s=p.read_text(); s=s.replace('for (_, e) in cases { let _ = black_box(simplify_mba(e, 64)); }','for (_, e) in cases { let _ = black_box(simplify_mba(e, 64)); }\nrumba_core::phase3_profile::dump();'); p.write_text(s)
# Counters are kept exclusively in this temporary profiling build.
p=root/'core/src/phase3_profile.rs'; s=p.read_text()
s=s.replace('const N: usize = 19;', 'const N: usize = 21;').replace('[[u64; 65]; 4]', '[[u64; 65]; 8]').replace('[[0; 65]; 4]', '[[0; 65]; 8]')
s=s.replace('"eval_nodes"].iter()', '"eval_nodes", "signature_input_nodes", "direct_linear"].iter()')
s += '''
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); unsafe { System.alloc(l) } }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); unsafe { System.alloc_zeroed(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p,l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { REALLOCS.fetch_add(1, Relaxed); BYTES.fetch_add(n as u64, Relaxed); unsafe { System.realloc(p,l,n) } }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
pub fn reset_allocs() { STATE.with(|_| {}); ALLOCS.store(0, Relaxed); REALLOCS.store(0, Relaxed); BYTES.store(0, Relaxed); }
pub fn allocations() { let a = ALLOCS.load(Relaxed); let r = REALLOCS.load(Relaxed); let b = BYTES.load(Relaxed); println!("allocations={a} reallocations={r} allocated_bytes={b}"); }
pub fn add_nodes(nodes: usize) { STATE.with(|s| s.borrow_mut().calls[19] += nodes as u64); }
'''
p.write_text(s)
inject('core/src/simplify.rs','fn is_linear_bitwise(&self, l: Expr, mask: u64) -> Option<Expr> {','crate::phase3_profile::add_nodes(l.size());')
inject('core/src/simplify.rs','let e = reduce_vars(e, &mut var_map, &mut t);','crate::phase3_profile::hist(4, match &e { Expr::Add(es) => es.len(), _ => 1 });')
p=root/'core/src/simplify.rs'; s=p.read_text().replace('for _ in 0..MAX_SIMPLIFICATION_PASSES {', 'for pass in 0..MAX_SIMPLIFICATION_PASSES {\ncrate::phase3_profile::hist(5, pass);')
s=s.replace('let next_size = next.size();', 'let next_size = next.size();\nif pass == 1 { crate::phase3_profile::hist(6, usize::from(next == e)); }')
s=s.replace('if self.is_linear(&e) {\n            return self.solve_linear', 'if self.is_linear(&e) {\n            crate::phase3_profile::count(20);\n            return self.solve_linear')
p.write_text(s)
inject('core/src/reduce.rs','if initial_len > out.len() {','crate::phase3_profile::hist(7, initial_len);')
p=root/'core/examples/phase3_probe.rs'; s=p.read_text().replace('"once" => {','"once" => {\nrumba_core::phase3_profile::reset_allocs();').replace('rumba_core::phase3_profile::dump();', 'rumba_core::phase3_profile::allocations();\nrumba_core::phase3_profile::dump();'); p.write_text(s)
p=root/'core/src/simplify.rs'
if 'fn packed_signature' in p.read_text():
 m=root/'core/src/phase3_profile.rs'; s=m.read_text().replace('const N: usize = 21;', 'const N: usize = 24;').replace('[[u64; 65]; 8]', '[[u64; 65]; 10]').replace('[[0; 65]; 8]', '[[0; 65]; 10]').replace('"direct_linear"].iter()', '"direct_linear", "packed_success", "cube_nodes", "packed_term_nodes"].iter()'); m.write_text(s)
 inject('core/src/simplify.rs','fn packed_signature(&self, e: &Expr, t: usize) -> Option<Vec<u64>> {','crate::phase3_profile::hist(9, t);')
 inject('core/src/simplify.rs','fn cube(e: &Expr, mask: u64) -> Option<u64> {','crate::phase3_profile::count(22);')
 inject('core/src/simplify.rs','fn add(e: &Expr, coeff: u64, mask: u64, signature: &mut [u64]) -> Option<()> {','crate::phase3_profile::count(23);')
 s=p.read_text().replace('        Some(signature)\n', '        crate::phase3_profile::count(21); crate::phase3_profile::hist(8, t);\n        Some(signature)\n'); p.write_text(s)
# Attribute scale handling separately when assessing residual reducer work.
import re
m=root/'core/src/phase3_profile.rs'; s=m.read_text(); n=int(re.search(r'const N: usize = (\d+);',s)[1]); h=int(re.search(r'hist: \[\[u64; 65\]; (\d+)\]',s)[1]); s=s.replace(f'const N: usize = {n};',f'const N: usize = {n+2};').replace('].iter().enumerate() { println!', ', "reduce_scale", "group_fastpath"].iter().enumerate() { println!'); s=s.replace(f'[[u64; 65]; {h}]',f'[[u64; 65]; {h+1}]').replace(f'[[0; 65]; {h}]',f'[[0; 65]; {h+1}]'); m.write_text(s)
inject('core/src/reduce.rs','fn reduce_scale(&self, scale: u64, expr: Expr) -> Expr {',f'let _profile = crate::phase3_profile::enter({n});')
inject('core/src/reduce.rs','Expr::Add(sum) => {',f'crate::phase3_profile::hist({h}, sum.len());')
p=root/'core/src/reduce.rs'; s=p.read_text().replace('            exprs.sort();\n            return Expr::Add(exprs);',f'            crate::phase3_profile::count({n+1});\n            exprs.sort();\n            return Expr::Add(exprs);'); p.write_text(s)

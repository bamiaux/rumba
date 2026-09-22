from pathlib import Path
import re,sys
root=Path(sys.argv[1]); p=root/'core/src/phase3_profile.rs'; s=p.read_text()
n=int(re.search(r'const N: usize = (\d+);',s)[1]); s=s.replace(f'const N: usize = {n};',f'const N: usize = {n+2};').replace('].iter().enumerate() { println!', ', "get_vars", "variable_nodes"].iter().enumerate() { println!')
s+='''
thread_local! { static VAR_ALLOCS: std::cell::Cell<(u64,u64)> = const { std::cell::Cell::new((0,0)) }; }
pub struct VarsAlloc(u64,u64);
pub fn vars_alloc() -> VarsAlloc { VarsAlloc(ALLOCS.load(Relaxed),BYTES.load(Relaxed)) }
impl Drop for VarsAlloc { fn drop(&mut self) { let a=ALLOCS.load(Relaxed)-self.0; let b=BYTES.load(Relaxed)-self.1; VAR_ALLOCS.with(|c| {let (aa,bb)=c.get(); c.set((aa+a,bb+b)); }); } }
pub fn dump_vars() { VAR_ALLOCS.with(|c|println!("get_vars_allocations={:?}",c.get())); }
'''; p.write_text(s)
p=root/'core/src/expr.rs'; s=p.read_text().replace('pub fn get_vars(&self) -> HashSet<VarId> {',f'pub fn get_vars(&self) -> HashSet<VarId> {{\nlet _vars_profile=crate::phase3_profile::enter({n});\nlet _vars_alloc=crate::phase3_profile::vars_alloc();')
s=s.replace('F: FnMut(&Expr, Vec<T>) -> T + Clone,\n    {',f'F: FnMut(&Expr, Vec<T>) -> T + Clone,\n    {{\ncrate::phase3_profile::count({n+1});')
s=s.replace('fn collect(e: &Expr, vars: &mut HashSet<VarId>) {',f'fn collect(e: &Expr, vars: &mut HashSet<VarId>) {{\ncrate::phase3_profile::count({n+1});')
p.write_text(s)
p=root/'core/examples/phase3_probe.rs';p.write_text(p.read_text().replace('rumba_core::phase3_profile::dump();','rumba_core::phase3_profile::dump();\nrumba_core::phase3_profile::dump_vars();'))

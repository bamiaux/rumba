from pathlib import Path
import sys
root=Path(sys.argv[3])
base=(Path(sys.argv[2])/'core/src/reduce.rs').read_text().split('\nimpl Expr {')[0]
base+='\npub fn reduce(e: Expr, n: u8) -> Expr { Reducer { mask: make_mask(n) }.reduce_masked(e) }\n'
(root/'core/src/phase3_reference_reduce.rs').write_text(base)
p=root/'core/src/lib.rs'; s=p.read_text(); marker='\n#[cfg(test)]\nmod phase3_reference_reduce;\n';
if marker not in s: p.write_text(s+marker)
(root/'core/src/reduce.rs').write_text(Path(sys.argv[1]).read_text()+Path(__file__).with_name('reduce-tests.rs').read_text())

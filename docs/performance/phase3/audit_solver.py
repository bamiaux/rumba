"""Inject a reference solver and deterministic differential tests into a copy."""
from pathlib import Path
import shutil
import sys

source, baseline, target = map(Path, sys.argv[1:])
source_text = source.read_text()
shutil.copyfile(baseline / 'core/src/simplify.rs', target / 'core/src/reference_simplify.rs')
shutil.copytree(baseline / 'core/src/simplify', target / 'core/src/reference_simplify', dirs_exist_ok=True)
p = target / 'core/src/lib.rs'
p.write_text(p.read_text() + '\n#[cfg(test)]\nmod reference_simplify;\n')
(target / 'core/src/simplify.rs').write_text(source_text + Path(__file__).with_name('solver-tests.rs').read_text())

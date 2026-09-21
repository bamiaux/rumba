from pathlib import Path
import sys
source=Path(sys.argv[1]).read_text()
base=(Path(sys.argv[2])/'core/src/simplify.rs').read_text()
reference=base[base.index('    /// Creates a conjuction'):base.index('    /// Solves a linear MBA')].replace('fn make_conjunction_sum(', 'fn reference_conjunction_sum(')
source=source.replace('    /// Creates a conjuction', reference+'    /// Creates a conjuction', 1)
source=source.replace('        Some(signature)\n', '''        let scalar = self.calc_signature(e, t);
        assert_eq!(signature, scalar, "packed table q={t}, n={}, expr={e:?}", self.n);
        assert_eq!(self.make_conjunction_sum(signature.clone(), t), self.reference_conjunction_sum(scalar, t));
        Some(signature)
''')
source += Path(__file__).with_name('packed-tests.rs').read_text()
(Path(sys.argv[3])/'core/src/simplify.rs').write_text(source)

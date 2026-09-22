# Solver performance

The September 2026 optimization preserves the solver's transformations, proof
checks, candidate ordering and stopping rules. There are no corpus-specific
cases, new dependencies, cross-expression caches or target-specific intrinsics.

Changes are confined to existing kernels:

- Collect variables into one set; compute the highest identifier without a set.
- Reuse unary-node boxes when mapping children.
- Group sum coefficients and cancel XOR pairs in the existing contiguous vector.
- Enumerate the assignments containing a conjunction directly, without testing
  each variable of each candidate assignment.
- Infer bitwise dependencies using 64 parallel bit lanes in ordinary `u64`
  operations (software SIMD). Two small masks record observed zero/one outputs;
  an iterator enumerates all valid completions in the original order.

Packed truth tables are an established technique in logic tools such as
[ABC](https://github.com/berkeley-abc/abc/blob/master/src/misc/util/utilTruth.h).
Buffer reuse uses standard Rust
[`Vec` operations](https://doc.rust-lang.org/std/vec/struct.Vec.html).

## Measurements

Intel i7-8650U, Linux x86-64, Rust 1.95.0, release build, `parse` feature,
64-bit expressions, pinned to CPU 2. No native CPU flags or PGO. Two complete
reports per executable in baseline/candidate/candidate/baseline order; values
below average the two reports' five-pass medians. Baseline totals were 2.712 and
2.795 s; candidate totals were 2.147 and 2.145 s. CPU frequency was not fixed.

| Metric | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Corpus time | 2.754 s | 2.146 s | -22.1% |
| p50 | 32.39 µs | 26.69 µs | -17.6% |
| p95 | 182.46 µs | 134.69 µs | -26.2% |
| p99 | 487.58 µs | 365.53 µs | -25.0% |
| Allocations + reallocations | 40,440,417 | 25,703,231 | -36.4% |
| Cumulative requested bytes | 2,474,629,954 | 1,548,415,108 | -37.4% |

Allocation counts come from a separate single pass. Each of the seven datasets
improved in total time in both reports. Individual tail measurements remain
sensitive to system noise; these are measurements on this machine, not bounds.

Baseline sampling identified allocation/freeing, recursive reduction, expression
evaluation and bitwise dependency inference as hotspots. In sampled stacks
reaching `simplify_mba`, `malloc` accounted for 8.56% of cycles, `free` 7.52%,
`reduce_masked` 7.59%, `eval_bits` 7.14% and `infer_bitwise_truth_tables` 3.53%
(self time). This motivated buffer reuse and bit-parallel observation checks.

Hardware counters, averaged over two three-pass runs per executable in the same
alternating order, also decreased: instructions 75.49 → 59.95 billion (-20.6%),
branches 13.33 → 10.80 billion (-19.0%), branch misses 280.19 → 248.51 million
(-11.3%), cycles 37.28 → 31.06 billion (-16.7%). These counters include the
runner's parsing and cloning, unlike the simplification-only allocation counts.

## Reproduce

Baseline source: `e1f9f53`. Build baseline and candidate with the same compiler,
features and flags, copying their executables before rebuilding the other one.
For the new profiler, copy `core/examples/profile.rs` into the baseline checkout
as well; it uses the unchanged public API and existing corpus support module.

```sh
cargo build --release -p rumba-core --examples --features parse
cp target/release/examples/corpus /tmp/candidate-corpus
cp target/release/examples/profile /tmp/candidate-profile

# Repeat in baseline/candidate/candidate/baseline order on an otherwise idle CPU.
taskset -c 2 /tmp/baseline-corpus --save /tmp/baseline.snapshot
taskset -c 2 /tmp/candidate-corpus --baseline /tmp/baseline.snapshot

# Count only allocations made during simplification; compare every output AST.
/tmp/baseline-profile --allocations --outputs /tmp/baseline.outputs
/tmp/candidate-profile --allocations --outputs /tmp/candidate.outputs
cmp /tmp/baseline.outputs /tmp/candidate.outputs

# Sample CPU hotspots; repeat for baseline and candidate.
perf record -e cycles:u -F 997 --call-graph dwarf,8192 -o /tmp/candidate.perf \
  /tmp/candidate-profile --repeat 3
perf report --stdio -i /tmp/candidate.perf --no-children -g none
perf stat -e cycles:u,instructions:u,branches:u,branch-misses:u \
  taskset -c 2 /tmp/candidate-profile --repeat 3
```

The corpus runner uses five measured passes per dataset after a quality pass.
Parsing and input cloning precede timing. Its total includes output destruction
and sample bookkeeping; individual latency samples cover `simplify_mba` only.
The profiler excludes parsing, cloning, formatting and result destruction from
its timers and allocation counters. Allocation bytes are cumulative requested
bytes (including reallocation sizes), **not peak memory**. Instrumented timings
are diagnostic; use the uninstrumented corpus executable for latency comparisons.
External `perf` counters cover the whole process, including parsing and cloning;
filter sampled call stacks containing `simplify_mba` to isolate solver hotspots.

## Quality and generality

All 41,000 output ASTs match the baseline byte for byte. Corpus classification
remains 41,000 OK, zero OKZ/NG, with 218,018 output AST nodes. The quality suite
also checks semantics against ground truth with 200 random assignments per case.

Independent generated tests cover scalar versus packed observations on widths
0–64, every binary Boolean function, missing observations, contradictory outputs,
all conjunction subsets up to nine variables, coefficient overflow/cancellation,
sum/XOR permutations and sparse variable identifiers. These tests do not use
corpus expressions. The exact hidden-component proof remains mandatory after
the packed observation filter.

Validation: `cargo fmt --all --check`, workspace Clippy with all features/targets
and `-D warnings`, release tests for core/CLI/C bindings with all features, and
the corpus report's unit tests. Production code shrank by 18 lines; the new
profiler and generated tests are kept outside production paths.

## Follow-up from `afa05d9`

A second profile still put recursive reduction, evaluation and allocation at the
top (7.53%, 7.04% and 6.03% self time respectively for `reduce_masked`, `eval_bits`
and `malloc`, including the runner in the denominator). The next changes are:

- Evaluate four truth-table assignments per traversal, with one interpreter
  shared by scalar and batch evaluation. Variable values are computed directly;
  the interpreted truth table no longer allocates a variable buffer. Ordinary
  fixed-size arrays keep this portable; x86-64 release assembly uses `pand`,
  `por`, `pxor` and `paddq`, without architecture-specific intrinsics. Processing
  values in batches also underlies established vector engines such as
  [DuckDB](https://github.com/duckdb/duckdb/blob/main/src/include/duckdb/common/vector_operations/binary_executor.hpp).
- Reuse the reducer's input vector until an actual expansion requires separating
  pending and accepted operands. Preserve stack order and linear flattening cost;
  use unstable sorting where structurally equal values are interchangeable.
- Borrow linear-solve inputs instead of cloning them, remove the redundant
  `from_poly` flag and use the existing `rustc-hash` dependency for the solve cache.

The optional JIT remains available and disabled by default. Performance work
targets the native evaluator with `--features parse`, without `--all-features`.
The JIT compiler and its benchmarks are unchanged.

Same machine, compiler, features and alternating order as above; measurements
are relative to `afa05d9`, not to the original `e1f9f53` baseline. Both binaries
use only the `parse` feature: none of these measurements use JIT compilation.

| Metric | `afa05d9` | Follow-up | Change |
| --- | ---: | ---: | ---: |
| Corpus wall time | 2.406 s | 2.303 s | -4.3% |
| p50 | 28.27 µs | 25.15 µs | -11.0% |
| p95 | 150.72 µs | 151.60 µs | +0.6% |
| p99 | 412.73 µs | 399.68 µs | -3.2% |
| Allocations + reallocations | 25,703,231 | 18,607,451 | -27.6% |
| Cumulative requested bytes | 1,548,415,108 | 1,108,163,892 | -28.4% |
| Instructions, entire runner × 3 | 59.97 billion | 53.18 billion | -11.3% |
| Branches, entire runner × 3 | 10.80 billion | 9.39 billion | -13.1% |
| Branch misses, entire runner × 3 | 250.61 million | 226.91 million | -9.5% |
| Cycles, entire runner × 3 | 31.05 billion | 28.52 billion | -8.1% |

Wall time was noisy: baseline report totals were 2.303/2.508 s and candidate
totals 2.333/2.272 s. The first pair alone did not show an improvement; no tail
latency improvement is claimed. In the separate counter runs, simplification-only
timers averaged 2.058 → 1.837 s per corpus (-10.8%); these remain diagnostic
because the profiler replaces the allocator. The instruction and allocation
reductions are more stable evidence than the small corpus wall-time difference.
In the final native profile, scalar and batched `eval_lanes` together account
for 4.01% of sampled cycles, `malloc` for 4.54% and `reduce_masked` for 7.94%.
These are whole-runner self-time shares, not isolated kernel speedup ratios.

All 41,000 output ASTs remain byte-identical, with unchanged quality counts.
New seeded tests compare scalar and batched evaluation against an independent
wrapping-integer interpreter: 0–12 variables, widths 0–64, arbitrary constants,
mixed operations and empty operands. They also check reduction semantics and
neutral values of empty operators in large tables. The generated evaluation
suite targets builds without `jit`, so large tables exercise the native path.
Release tests pass with `parse` and with all features; workspace Clippy and
formatting pass. No new production dependencies or compilation layers are added.

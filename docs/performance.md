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

# Algebraic prettifier qualification

## Scope and verdict

**Verdict: `HOLD` for performance qualification of pass 2.** The final candidate processes all 41,000 corpus rows with `NG = 0`, reduces total output AST from 218,018 to 213,542 nodes (−4,476; −2.05%), and never produces a larger AST than the `prettify` baseline on any row. Pass 2 reproduces all 41,000 outputs and AST sizes of the previous candidate exactly. Its two clean paired measurements against `prettify` show p50 increases of +3.59% and +4.81%; p95 increases are +3.52% and +7.25%. The preferred +3% p50 and +5% p95 ceilings are therefore not consistently demonstrated. The branch is deliberately unmerged.

- Repository base branch: `prettify`.
- Exact base commit: `e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9`.
- Research branch: `research/algebraic-prettify`.
- Exact case-census and original implementation commit: `317ffe545cae4d82b6dfe9b013a57d3e61a2c6df`.
- Original quality-authority commit: `7d99f56a5c0b86707eaf944d3d1a5b40881810c6`.
- Shared-index implementation commit: `e25ae94c1c6ef75854e6583ac17fada173b44185`.
- Parsimony pass 2 implementation commit: `a213cb35255a8c57455aa7a3a52c7e54d5aeba64`.
- The initial algebraic `.patch` was not a valid unified diff (`git apply --check` reported “patch with only garbage” at line 5); its algebraic behavior was implemented directly. The solver was not changed.

## Algorithm

The pass runs after solving, in `core/src/prettify.rs`. Each term is interpreted as a coefficient modulo `2^w` and a sorted conjunction support in the existing `FactorSet` representation. One sparse `AddIndex` per sum state stores the factor sets, support-to-(term-index, coefficient) map, and constant-coefficient-to-term-index map. Binary OR/XOR scans pairs of terms and looks up their union support. Difference scans pairs. The higher-order probe uses the same index and checks only the degree-two and degree-three *resident faces* relevant to the laws below. Other terms, including high-degree terms, remain in the sum. No subset of the input polynomial is enumerated.

The existing binary contractions run to completion along one deterministic path. A second path may begin with one higher-order contraction, then use the same binary normalization. Each path retains its smallest AST, since one binary step can temporarily grow before a later complement step shrinks. The smaller path wins; ties retain the binary result. Every higher-order candidate itself must strictly reduce `Expr::size`. At least four resident terms are mathematically required by the smallest new identity; this lower-bound check is not a maximum-size gate. Only coefficient/support relations decide which law applies.

## Algebraic laws and derivation

Write `ab` for `a & b` and let all additions and coefficient operations be modulo `2^w`. The bitwise identities `a|b = a+b-ab`, `a^b = a+b-2ab`, and `~a = -1-a` hold as integer identities at each bit position.

1. **Three-atom OR:** Inclusion–exclusion gives `a|b|d = a+b+d-ab-ad-bd+abd`. Multiplying by any coefficient `c` gives the matched seven-term face.
2. **Filtered OR:** `a&~b = a-ab`, and `((a&~b)&d) = ad-abd`. Substitution in `u|d = u+d-ud` gives `(a&~b)|d = a+d-ab-ad+abd`.
3. **Complement XOR:** `(~b)&d = d-bd` and `(~a)&(~b)&d = d-ad-bd+abd`. Apply `u^v = u+v-2uv` with `u=~a=-1-a` and `v=(~b)&d`. The result is `~a ^ ((~b)&d) = -1-a-d+2ad+bd-2abd`.
4. **Affine XNOR:** `~(x^y) = -1-x-y+2xy` and `~y=-1-y`. Expanding `(-cx)*~(x^y) + (cx-cy)*~y` gives `cx*x + cy*y - 2cx*xy + cy`.
5. **General difference:** `A&~B = A-AB` follows bitwise from `~B=-1-B` as a complement of a Boolean mask, or directly from the partition of the bits of `A` into those in and outside `B`. Thus `c*A-c*(A&B) = c*(A&~B)` for any canonical conjunction `A`.

These identities hold for composite Boolean operands as well as variables. The unit tests exercise arbitrary non-unit scalars at 8, 32, and 64 bits, composite operands, unrelated terms, a five-factor unrelated monomial, more than ten terms, and variable IDs above 20. They check semantic equivalence, strict AST improvement for selected contractions, and exact preservation of unrelated terms. The XNOR test also checks that an equal-size higher-order candidate is rejected.

## Verification

| Check | Result |
|---|---|
| Baseline `cargo test --all-features` | Passed |
| Final candidate `cargo fmt --check` | Passed |
| Final candidate `cargo test --all-features` | Passed |
| Final candidate `cargo clippy --all-targets --all-features -- -D warnings` | Passed |
| Corpus semantic check and solver status | 41,000 rows; `OK = 41,000`, `OKZ = 0`, `NG = 0`; no mismatch or panic |
| Rowwise AST comparison against base | 0 rows larger |
| Output comparison against original candidate | All 41,000 output strings and AST sizes identical after pass 2 |

The baseline was frozen with `just corpus --save /tmp/prettify-baseline.snapshot`. Every ablation ran the full 41,000-row corpus and a rowwise comparison with the original candidate's output. Timing used paired runs of the corpus example in release mode, with five measurements per dataset. The command was `cargo run --release -p rumba-core --example corpus --features parse -- --save <snapshot>`; candidate runs additionally used `--baseline <base-snapshot>`. `qemu-kvm` was absent before, between, and after the pass-2 benchmark pairs. Earlier timing under its CPU load was discarded.

### Corpus quality by dataset

The columns `W/T/L` compare each actual output AST with the supplied target AST. Every row in both runs was `OK`; `OKZ` and `NG` were zero. `Raw AST` is identical between runs.

| Dataset | Rows | Baseline W/T/L | Candidate W/T/L | Baseline AST | Candidate AST | Raw AST |
|---|---:|---:|---:|---:|---:|---:|
| loki_tiny | 25,000 | 0 / 25,000 / 0 | 0 / 25,000 / 0 | 80,000 | 80,000 | 80,000 |
| mba_flatten | 3,000 | 2,338 / 383 / 279 | 2,540 / 331 / 129 | 29,535 | 27,523 | 38,329 |
| mba_obf_linear | 1,000 | 821 / 37 / 142 | 875 / 38 / 87 | 9,892 | 9,387 | 12,389 |
| mba_obf_nonlinear | 1,000 | 1,000 / 0 / 0 | 1,000 / 0 / 0 | 5,878 | 5,846 | 9,582 |
| neureduce | 10,000 | 9,083 / 816 / 101 | 9,135 / 829 / 36 | 79,931 | 79,288 | 115,827 |
| qsynth_ea | 500 | 261 / 20 / 219 | 269 / 18 / 213 | 10,682 | 9,411 | 7,145 |
| syntia | 500 | 104 / 342 / 54 | 108 / 339 / 53 | 2,100 | 2,087 | 2,076 |
| **Total** | **41,000** | **13,607 / 26,598 / 795** | **13,927 / 26,555 / 518** | **218,018** | **213,542** | **265,348** |

### Casewise census

The committed [case file](ALGEBRAIC_PRETTIFY_CASES.tsv) records dataset/row, baseline output, candidate output, target, all three AST sizes, and W/T/L transition for **every one of the 2,211 changed outputs**. Of these, 1,042 are strictly smaller and 1,169 have equal AST size; 0 are larger. The remaining 38,789 outputs are unchanged.

| Transition | Rows |
|---|---:|
| L→T | 55 |
| L→W | 222 |
| T→W | 98 |
| L→L | 518 |
| T→T | 26,500 |
| W→W | 13,607 |
| W→T, W→L, T→L | **0** |

### Ablations

Each ablation ran all 41,000 rows with `OK = 41,000`, `OKZ = 0`, and `NG = 0`. “Larger/smaller” compares each output AST to the original candidate at `7d99f56`; the strict gate also required the original aggregate W/T/L and AST totals or better, with no larger output on any row.

| Step | Change | W / T / L | AST | Larger / smaller | Gate |
|---|---|---:|---:|---:|---|
| A | Remove explicit three-atom OR | 13,927 / 26,555 / 518 | 213,580 | 5 / 0 | Fail |
| B | Remove explicit filtered OR | 13,902 / 26,563 / 535 | 213,706 | 52 / 7 | Fail |
| C | Remove `higher_first` path | 13,858 / 26,544 / 598 | 214,252 | 201 / 0 | Fail |
| D | Share `AddIndex`; binary pair scan and support lookup | 13,927 / 26,555 / 518 | 213,542 | 0 / 0 | Pass |
| E | Remove complement XOR from D | 13,902 / 26,535 / 563 | 213,810 | 52 / 10 | Fail |
| F | Remove affine XNOR from D | 13,914 / 26,557 / 529 | 213,641 | 75 / 0 | Fail |

A and B were also repeated on top of D: A still had 5 larger rows and AST 213,580; B still had 52 larger rows and AST 213,706. The three-atom OR's five counterexamples are `mba_flatten` rows 926, 978, 989 and `qsynth_ea` rows 259, 411. The proposed binary decompositions are algebraically valid, but this deterministic contraction order does not reach the same compact result in every case. A different rewrite selection or coefficient splitting would need separate qualification before either explicit law could be removed.

### Five-run performance before pass 2

The table below records the earlier shared-index candidate at `e25ae94`, before the parsimony pass. Times and percentiles below are medians of the runner's five measurements; throughput is rows divided by median total time. Values are rounded for display; comparison percentages use snapshot nanoseconds.

| Global metric | `prettify` base | Shared-index D | Change |
|---|---:|---:|---:|
| Total time | 2.316 s | 2.433 s | +5.06% |
| Throughput | 17,703 expr/s | 16,851 expr/s | −4.81% |
| p50 | 27.360 µs | 27.990 µs | +2.30% |
| p95 | 156.049 µs | 163.060 µs | +4.49% |
| p99 | 410.649 µs | 438.679 µs | +6.83% |
| Maximum | 31.145 ms | 32.837 ms | +5.43% |

| Dataset | Total base → D | p95 base → D | p99 base → D |
|---|---:|---:|---:|
| loki_tiny | 1.160 → 1.211 s (+4.38%) | 145.070 → 150.809 µs (+3.96%) | 332.919 → 360.889 µs (+8.40%) |
| mba_flatten | 195.930 → 202.701 ms (+3.46%) | 172.510 → 179.979 µs (+4.33%) | 215.390 → 226.749 µs (+5.27%) |
| mba_obf_linear | 129.699 → 135.163 ms (+4.21%) | 400.289 → 417.559 µs (+4.31%) | 447.098 → 468.799 µs (+4.85%) |
| mba_obf_nonlinear | 63.265 → 64.029 ms (+1.21%) | 143.819 → 142.410 µs (−0.98%) | 167.709 → 168.120 µs (+0.25%) |
| neureduce | 272.551 → 280.955 ms (+3.08%) | 46.990 → 48.420 µs (+3.04%) | 54.699 → 57.090 µs (+4.37%) |
| qsynth_ea | 478.094 → 542.535 ms (+13.48%) | 3.902 → 4.715 ms (+20.85%) | 9.424 → 10.658 ms (+13.09%) |
| syntia | 12.679 → 13.336 ms (+5.18%) | 126.890 → 131.899 µs (+3.95%) | 195.889 → 205.799 µs (+5.06%) |

At the shared-index commit, the global p50 and p95 met the preferred ceilings of +3% and +5%. The `qsynth_ea` tail was less stable. In a separate clean paired run against the original `7d99f56` implementation, D changed total time from 2.443 s to 2.423 s (−0.83%), p50 from 28.880 to 28.960 µs (+0.28%), and p95 from 161.260 to 162.420 µs (+0.72%). This supports no material performance regression from the index refactor itself; the base comparison remains the relevant end-to-end cost.

### Parsimony pass 2 verification

The pass-2 patch omitted unified-diff hunk ranges, so `git apply --check` rejected it. Its hunks were matched uniquely against the source and applied. It also left a reference to the removed `first_coefficient` binding; that compilation error was corrected by removing the stale assignment. `cargo fmt --check`, `cargo test --all-features`, and `cargo clippy --all-targets --all-features -- -D warnings` passed. A separate release-mode comparison found **0 changed output strings and 0 changed AST sizes across all 41,000 rows**. The full corpus runner again recorded `OK = 41,000`, `OKZ = 0`, `NG = 0`, and `(W,T,L,AST) = (13927,26555,518,213542)`.

The table shows paired five-run medians from clean runs without `qemu-kvm`; both base-to-pass-2 orders used a separately rebuilt binary for each revision. Percentages compare snapshot nanoseconds. The reverse pair ran pass 2 before the base.

| Pair | Total time | p50 | p95 | p99 |
|---|---:|---:|---:|---:|
| Shared-index D → pass 2 | 2.461 → 2.420 s (−1.66%) | 28.990 → 28.540 µs (−1.55%) | 162.479 → 167.299 µs (+2.97%) | 423.719 → 418.089 µs (−1.33%) |
| `prettify` → pass 2 | 2.418 → 2.471 s (+2.20%) | 27.830 → 28.830 µs (+3.59%) | 161.810 → 167.500 µs (+3.52%) | 434.409 → 437.429 µs (+0.70%) |
| `prettify` → pass 2, reverse order | 2.387 → 2.491 s (+4.35%) | 27.470 → 28.790 µs (+4.81%) | 160.490 → 172.130 µs (+7.25%) | 410.699 → 440.489 µs (+7.25%) |

The two direct base comparisons do not establish the preferred latency ceilings, although the pass-2-to-D pair shows no total-time regression from this refactor. Individual dataset times vary between runs; for example, the base `neureduce` median total changed from 270 ms to 309 ms between the direct pairs. These timings do not justify a stable per-dataset performance claim.

## Complexity and architecture audit

Let `t` be resident top-level terms, `d` the largest support degree, and `M = |P|` the total AST size of the sparse polynomial including its operands. The shared index is built once per sum state, in expected `O(M)` time and at worst `O(tM)` under hash collisions. The binary OR/XOR scan considers `O(t²)` pairs, computes each union support and looks it up in the index; a conservative per-state bound including factor comparisons and sorting is expected `O(t² M log(d+1))`. Difference is another pair scan. The higher-order probe checks a constant number of support relations per resident degree-two or degree-three term, using the same index; it has expected `O(tM)` cost and worst-case `O(t²M)` under collisions. It examines only resident supports; there is no subset enumeration. Every contraction removes at least one top-level term, so each of the two deterministic paths has at most `t` steps. A conservative whole-Add expected bound is `O(t³ M log(d+1))`. All bounds are polynomial in the stated input parameters.

1. Every match uses canonical coefficient, constant, and support relations. `FxHashMap` is used only for key lookup; term order and tie-breaking determine output, so hash iteration order does not decide a rewrite.
2. No result depends on how the input expression was written before solving. Composite Boolean atoms are accepted.
3. TARGET is never read by the prettifier. The target is used only by the corpus runner and the report comparison.
4. There is no hidden exponential search: at most two deterministic contraction paths are evaluated for a sum.
5. There is no maximum term count, variable count, or global degree rejection. The degree-two/three probe and four-term minimum follow from the laws' support faces. Unrelated high-degree monomials and sums with more than ten terms are tested. The solver's existing `MAX_VARS = 20` was untouched.
6. All four explicit higher-order identities remain because each independent ablation fails the current strict quality gate. The index is shared by binary and higher-order matching. Algebraic decomposition alone did not demonstrate that a smaller deterministic rewrite implementation can preserve all selected outputs.

No SOURCE fingerprint, TARGET comparison, corpus ID, dataset string, row ID, Forest, automaton, Mealy transducer, or arbitrary maximum-size gate appears in the new production reifier. The only changed production file is `core/src/prettify.rs`.

## Diffstat

Against the `prettify` base, the final implementation changes `core/src/prettify.rs` by **550 insertions and 160 deletions** (710 changed lines). The shared-index refactor was **141 insertions and 158 deletions** relative to the original candidate, a net reduction of 17 lines. Parsimony pass 2 is **17 insertions and 69 deletions** relative to that refactor, a further net reduction of 52 lines. The exhaustive case census adds 2,212 TSV lines including its header. No solver file changes. The report is committed separately on the same research branch.

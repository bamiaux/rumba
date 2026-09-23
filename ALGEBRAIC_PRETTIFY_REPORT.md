# Algebraic prettifier qualification

## Scope and verdict

**Verdict: `PROMOTE`.** The candidate processes all 41,000 corpus rows with `NG = 0`, reduces total output AST from 218,018 to 213,542 nodes (−4,476; −2.05%), and never produces a larger AST than the `prettify` baseline on any row. In the final five-run measurement without `qemu-kvm`, total time changes from 2.456 s to 2.492 s (+1.47%), p50 from 29.190 µs to 29.220 µs (+0.10%), and p95 from 166.379 µs to 169.809 µs (+2.06%). The branch is deliberately unmerged.

- Repository base branch: `prettify`.
- Exact base commit: `e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9`.
- Research branch: `research/algebraic-prettify`.
- Exact candidate implementation and case-census commit: `317ffe545cae4d82b6dfe9b013a57d3e61a2c6df`.
- The supplied `.patch` was not a valid unified diff (`git apply --check` reported “patch with only garbage” at line 5); its algebraic behavior was implemented directly. The solver was not changed.

## Algorithm

The pass runs after solving, in `core/src/prettify.rs`. Each term is interpreted as a coefficient modulo `2^w` and a sorted conjunction support in the existing `FactorSet` representation. An `FxHashMap` indexes resident supports and coefficients. A higher-order probe checks only the degree-two and degree-three *resident faces* relevant to the laws below. Other terms, including high-degree terms, remain in the sum. No subset of the input polynomial is enumerated.

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
| Candidate `cargo fmt --check` | Passed |
| Candidate `cargo test --all-features` | Passed |
| Candidate `cargo clippy --all-targets --all-features -- -D warnings` | Passed |
| Corpus semantic check and solver status | 41,000 rows; `OK = 41,000`, `OKZ = 0`, `NG = 0`; no mismatch or panic |
| Rowwise AST comparison | 0 rows larger than baseline |

The baseline was frozen with `just corpus --save /tmp/prettify-baseline.snapshot`. Final timing used `cargo run --release -p rumba-core --example corpus --features parse -- --save /tmp/prettify-baseline-clean.snapshot` on the base worktree, then the same command with `--baseline /tmp/prettify-baseline-clean.snapshot --save /tmp/algebraic-prettify-hash.snapshot` on the candidate worktree. The direct `cargo run` form was used because this repository's `just corpus` recipe hardcodes a worktree-local `target/release/examples/corpus`, while both worktrees shared a compilation cache. The runner measures five runs per dataset after its quality pass. `qemu-kvm` was absent before, between, and after the final pair. Earlier timing under its CPU load was discarded.

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

### Five-run performance

Times and percentiles below are medians of the runner's five measurements; throughput is rows divided by median total time. Values are rounded for display; the comparison percentages use snapshot nanoseconds.

| Global metric | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Total time | 2.456 s | 2.492 s | +1.47% |
| Throughput | 16,693 expr/s | 16,451 expr/s | −1.45% |
| p50 | 29.190 µs | 29.220 µs | +0.10% |
| p95 | 166.379 µs | 169.809 µs | +2.06% |
| p99 | 423.959 µs | 437.159 µs | +3.11% |
| Maximum | 31.339 ms | 32.987 ms | +5.26% |

| Dataset | Total base → candidate | p95 base → candidate | p99 base → candidate |
|---|---:|---:|---:|
| loki_tiny | 1.235 → 1.206 s (−2.41%) | 156.689 → 152.090 µs (−2.94%) | 354.110 → 349.399 µs (−1.33%) |
| mba_flatten | 207.425 → 214.843 ms (+3.58%) | 178.599 → 189.330 µs (+6.01%) | 223.190 → 239.959 µs (+7.51%) |
| mba_obf_linear | 130.822 → 142.557 ms (+8.97%) | 401.699 → 425.669 µs (+5.97%) | 440.379 → 605.709 µs (+37.54%) |
| mba_obf_nonlinear | 62.915 → 67.424 ms (+7.17%) | 143.739 → 152.860 µs (+6.35%) | 164.560 → 247.299 µs (+50.28%) |
| neureduce | 312.683 → 331.609 ms (+6.05%) | 56.250 → 58.960 µs (+4.82%) | 67.040 → 131.890 µs (+96.73%) |
| qsynth_ea | 488.494 → 508.061 ms (+4.01%) | 3.946 → 4.078 ms (+3.34%) | 9.409 → 9.800 ms (+4.16%) |
| syntia | 12.565 → 12.934 ms (+2.93%) | 123.319 → 127.060 µs (+3.03%) | 193.540 → 197.549 µs (+2.07%) |

The global p50 and p95 meet the stated preferred ceilings of +3% and +5%. Some dataset p99 values are higher; the runner's per-case wall-clock timings vary with host scheduling, and the maximum is one case. Those tails merit monitoring in a later promotion benchmark, but the five-run global result does not show a material throughput regression.

## Complexity and architecture audit

Let `t` be resident top-level terms, `d` the largest support degree, and `M = |P|` the total AST size of the sparse polynomial including its operands. The new hash index costs expected `O(M)` to build per candidate pass and at worst `O(tM)` under collisions. Each resident degree-two or degree-three term triggers only a constant number of lookups and at most a constant number of candidate constructions, so the higher-order probe is expected `O(tM)` and worst-case `O(t²M)` per pass. It examines only resident supports; there is no subset enumeration. The pre-existing binary union scan has three nested term loops and dominates at `O(t³ M log(d+1))` conservatively per pass, including factor comparisons and sorting. Every path removes at least one top-level term at each step, so there are at most `t` steps; the two-path choice adds only a factor of two. A conservative whole-Add bound is `O(t⁴ M log(d+1))`. All bounds are polynomial in the stated input parameters.

1. Every new match uses canonical coefficient, constant, and support relations. `FxHashMap` is used only for key lookup; term order and tie-breaking determine output, so hash iteration order does not decide a rewrite.
2. No result depends on how the input expression was written before solving. Composite Boolean atoms are accepted.
3. TARGET is never read by the prettifier. The target is used only by the corpus runner and the report comparison.
4. There is no hidden exponential search: at most two deterministic contraction paths are evaluated for a sum.
5. There is no maximum term count, variable count, or global degree rejection. The degree-two/three probe and four-term minimum follow from the laws' support faces. Unrelated high-degree monomials and sums with more than ten terms are tested. The solver's existing `MAX_VARS = 20` was untouched.
6. The three cube laws share support indexing and replacement. A general incidence-algebra section engine would require enumerating or representing more coefficient faces; no smaller implementation with the same selected outputs was demonstrated. The affine XNOR law uses a distinct constant/singleton/pair relation. The focused laws remain simpler than a general engine.

No SOURCE fingerprint, TARGET comparison, corpus ID, dataset string, row ID, Forest, automaton, Mealy transducer, or arbitrary maximum-size gate appears in the new production reifier. The only changed production file is `core/src/prettify.rs`.

## Diffstat

The candidate implementation commit changes `core/src/prettify.rs` by **482 insertions and 23 deletions** (505 changed lines). The exhaustive case census adds 2,212 TSV lines including its header. No solver file changes. The report is committed separately on the same research branch.

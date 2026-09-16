# RUMBA `prune` — Simplicity Census and Ablations

## Scope and checkout

- Branch: `prune`
- HEAD: `4f6a3214637afa6c38a5525ddb31fb80061077a2`
- Initial worktree: clean
- Final worktree: contains only the removable diagnostic/test plumbing and this report; no commit was created.
- MaskSpark input: `/home/bamiaux/Downloads/maskspark_v8(2).csv`
- MaskSpark checksum: `b52c791ef9cb7f0c17f595a3a5d3a7cb35371d4cdbd7745584e69e603f24b9c6`
- MaskSpark rows: 49 data rows, header `index,theme,source,expected`.

The CSV supplied with this run is v8; no v6 data was substituted.

The instrumentation is controlled by `RUMBA_DIAGNOSTICS=1` and the private
`RUMBA_ABLATION` switch. It carries a local `SolverStats` through the existing
solver; it does not read TARGET/EXPECTED during simplification, add patterns,
or add a second algebra. The corpus example gained `--maskspark PATH` and
`--maskspark-width N` for repeatable qualification.

## Executive result

The 41K baseline remains qualified: **41,000 direct, 0 OKZ, 0 NG, 0 ERR**.

MaskSpark v8 was not forgotten. At WORD64, the current pipeline produced 41
direct cases, 6 semantic-only cases, and 2 semantic failures: indexes **40
(`prefix-borrow-8`)** and **41 (`prefix-borrow-affine-8`)**. It produced no
errors. The `target+1` negative had **0/49 structural collisions** and
**0/49 semantic collisions**. At width 8, all 49 cases were semantically
correct, although only 31 were structurally direct.

The smallest understandable pipeline supported by the measured gates is:

- keep simple demanded-width `R_k` reduction;
- keep the width-typed hidden complement gauge and recursive restoration;
- keep `variable_substitution`;
- keep both filtered-cut certificate producers and the one improving quotient;
- keep scalar precision until a broader width oracle exists, despite no current gate delta;
- keep full `project_low` for now because it supplies unique MaskSpark v8 coverage, but isolate or replace it with a smaller targeted prefix/carry/borrow capability before deleting it;
- remove or replace binary hidden-relation synthesis after a wider oracle confirms the measured result; it has no unique coverage in either 41K or MaskSpark v8.

This is a recommendation, not the final architectural rewrite requested by the
mission. No production mechanism was permanently deleted.

## Phase 0: baseline qualification

Commands run before the diagnostic changes:

```text
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
```

All passed. The release corpus runner reported:

| Dataset | Total | Direct | OKZ | NG | ERR | Actual AST | Raw AST |
|---|---:|---:|---:|---:|---:|---:|---:|
| `loki_tiny` | 25,000 | 25,000 | 0 | 0 | 0 | 80,000 | 80,000 |
| `qsynth_ea` | 500 | 500 | 0 | 0 | 0 | 13,288 | 7,145 |
| `syntia` | 500 | 500 | 0 | 0 | 0 | 2,445 | 2,076 |
| **Total** | **41,000** | **41,000** | **0** | **0** | **0** | **298,710** | **265,348** |

The runner classifies simplifier failures as `ERR`; an error is never counted
as a successful negative rejection.

## MaskSpark v8 qualification

The runner simplifies SOURCE and EXPECTED independently, then checks the
simplified SOURCE against the original EXPECTED with `Expr::sem_equal` (200
random samples per width). `target+1` uses an independently built
`EXPECTED + 1`; its output is checked both structurally and semantically.

### Per-case result

`target+1` is `rejected` for every row at both widths. The baseline per-case
result is:

| Index | Theme | Width 64 | Width 8 | target+1 |
|---:|---|---|---|---|
| 1 | v7-legacy | direct | semantic-only | rejected |
| 2 | v7-legacy | direct | semantic-only | rejected |
| 3 | v7-legacy | direct | semantic-only | rejected |
| 4 | v7-legacy | direct | semantic-only | rejected |
| 5 | v7-legacy | direct | semantic-only | rejected |
| 6 | v7-legacy | direct | direct | rejected |
| 7 | v7-legacy | direct | direct | rejected |
| 8 | v7-legacy | direct | semantic-only | rejected |
| 9 | v7-legacy | direct | semantic-only | rejected |
| 10 | v7-legacy | direct | semantic-only | rejected |
| 11 | v7-legacy | direct | semantic-only | rejected |
| 12 | v7-legacy | direct | direct | rejected |
| 13 | v7-legacy | direct | direct | rejected |
| 14 | v7-legacy | direct | direct | rejected |
| 15 | v7-legacy | direct | direct | rejected |
| 16 | v7-legacy | direct | direct | rejected |
| 17 | v7-legacy | direct | direct | rejected |
| 18 | v7-legacy | direct | direct | rejected |
| 19 | v7-legacy | direct | direct | rejected |
| 20 | v7-legacy | direct | direct | rejected |
| 21 | v7-legacy | direct | direct | rejected |
| 22 | v7-legacy | direct | direct | rejected |
| 23 | v7-legacy | direct | direct | rejected |
| 24 | v7-legacy | semantic-only | semantic-only | rejected |
| 25 | v7-legacy | direct | direct | rejected |
| 26 | v7-legacy | direct | direct | rejected |
| 27 | v7-legacy | direct | direct | rejected |
| 28 | v7-legacy | direct | direct | rejected |
| 29 | dyadic-unit-not | direct | direct | rejected |
| 30 | dyadic-unit | direct | direct | rejected |
| 31 | demand-contraction | semantic-only | direct | rejected |
| 32 | low-not-affine | semantic-only | semantic-only | rejected |
| 33 | quotient-coefficients | direct | direct | rejected |
| 34 | carry-affine-frontier | direct | direct | rejected |
| 35 | borrow-affine-frontier | direct | direct | rejected |
| 36 | nested-carry-frontier | direct | direct | rejected |
| 37 | nested-borrow-frontier | direct | direct | rejected |
| 38 | prefix-carry-8 | direct | semantic-only | rejected |
| 39 | prefix-carry-affine-8 | direct | semantic-only | rejected |
| 40 | prefix-borrow-8 | NG | semantic-only | rejected |
| 41 | prefix-borrow-affine-8 | NG | semantic-only | rejected |
| 42 | ddq-2-cancellation | direct | direct | rejected |
| 43 | ddq-2-higher | direct | direct | rejected |
| 44 | ddq-3-valuation | direct | direct | rejected |
| 45 | ddq-4-valuation | semantic-only | semantic-only | rejected |
| 46 | ddq-dyadic-composition | direct | direct | rejected |
| 47 | prefix-carry-demand-contraction | semantic-only | semantic-only | rejected |
| 48 | safety-overlap-multiplicity | direct | direct | rejected |
| 49 | graded-disjoint-partition | semantic-only | semantic-only | rejected |

| Width | Direct | Semantic-only | NG | ERR | Structural `target+1` collisions | Semantic `target+1` collisions | Negative errors |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 64 | 41/49 | 6/49 | 2/49 | 0/49 | 0/49 | 0/49 | 0/49 |
| 8 | 31/49 | 18/49 | 0/49 | 0/49 | 0/49 | 0/49 | 0/49 |

The two WORD64 NG cases are not introduced by an ablation; they are already
present in the qualified current pipeline. The mismatch is in the existing
prefix-borrow family and needs a separate correctness investigation.

## NG54

The exact historical NG54 oracle was not present in the checkout: no
`ng54_candidates.json`, NG54 file, or existing project harness was found by
the repository search.

**REPORT: MISSING.** No substitute or reconstructed 54-case set was used.

## Small-width soundness

The dynamic-prefix test now exercises widths **1, 2, 3, 4, 5, and 64** over
six cases covering dynamic masks, nested low-prefix behavior, nested hidden
definitions, complement-gauge-shaped definitions, even coefficients, and
sub-width-style operands. The existing narrow-width prettification test also
passes. Both targeted tests pass with scalar precision enabled and with
`RUMBA_ABLATION=scalar_precision_off`.

The MaskSpark run additionally gives 49/49 semantic success at width 8. No
mismatch was observed in these checks. The tests use the existing sampled
semantic checker rather than a newly introduced evaluator.

## Census totals

The baseline census used `RUMBA_DIAGNOSTICS=1` with the 41K source expressions
once each. Counters below are cumulative; solver and reducer counters include
nested solves invoked while simplifying a source expression, so they should
not be interpreted as one event per top-level row.

### Fixed point

| Measure | Result |
|---|---:|
| Top-level expressions | 41,000 |
| Pass histogram | 1 pass: 94; 2 passes: 40,891; 3 passes: 15 |
| Maximum pass count | 3 |
| Pass-limit hits | 0 |

The outer fixed-point rule was not changed.

### LOW

| Measure | Count |
|---|---:|
| Low-prefix AND opportunities | 2,117 |
| `project_low` calls | 1,997 |
| `project_low` unchanged | 1,997 |
| `project_low` changed | 0 |
| Immediately proven bounded | 2 |
| Fallback/reject path | 1,995 |
| `project_low` time | 12,177,226 ns |
| Simple `R_k` re-reductions | 3,067 |
| Simple `R_k` changed operands | 128 |

On the 41K corpus, the large engine did not return a changed projection in
the measured opportunities. MaskSpark nevertheless shows unique behavior
outside that corpus; see the ablation matrix below.

### Hidden gauge

| Measure | Count |
|---|---:|
| `hide_in_var` calls | 97,265 |
| Full-width hidden | 97,265 |
| Sub-width hidden | 0 |
| Exact definition reuse | 45,355 |
| Exact complement reuse | 11,913 |
| Structural orbit reuse | 0 |
| New full-width hidden | 40,141 |
| New plain sub-width hidden | 0 |
| Hidden width histogram | `64:97,409` |
| Allocation width histogram | `64:40,141` |

The zero structural-orbit reuse count does not make complement canonicalization
free: disabling the orbit changes the orientation of newly allocated hidden
definitions and produced 143 OKZ results. It is therefore an information-flow
and canonicality boundary, not merely a cache hit rate.

### `merge_hidden`

| Measure | Count |
|---|---:|
| Calls | 179,404 |
| `changed=false` | 179,206 |
| `changed=true` | 198 |
| Targets examined | 38,755 |
| Constant successes | 0 |
| Unary successes | 62 |
| Binary successes | 148 |
| Candidate proofs attempted | 218 |
| Candidate proofs successful | 210 |
| Aliases emitted | 210 |
| Candidate proof time | 27,532,463 ns |
| Synthesis time | 112,116,431 ns |

### Variable substitution / lambda

| Measure | Count |
|---|---:|
| Calls | 289,187 |
| Zero hidden candidates | 263,215 |
| One-hidden attempts / successes | 23,404 / 8,506 |
| Two-hidden attempts / successes | 2,568 / 1,262 |
| `find_lambda_int` successes | 8,506 |
| `find_two_lambdas_int` successes | 1,262 |
| One-hidden `[0,1]` / `[-1,-2]` | 4,634 / 3,872 |
| Two-hidden `[0,1]` / `[-1,-2]` | 701 / 561 |

### Filtered cut

| Producer / measure | Count |
|---|---:|
| Predecessor candidates | 1,032 |
| Predecessor containment successes | 1,032 |
| Predecessor certified relations | 1,032 |
| Predecessor improving quotients | 186 |
| Predecessor winning candidates | 112 |
| Order candidate comparisons | 99,991 |
| Order subset successes | 40,445 |
| Order certified relations | 161,780 |
| Order improving quotients | 9 |
| Order winning candidates | 6 |
| Relation-proof time | 429,787,045 ns |
| `close` calls | 26,731 |
| `close` with no improvement | 24,691 |
| `close` changing result | 118 |
| No root term map | 1,922 |
| Term measure before / after | 56,456 / 56,236 |
| Hidden measure before / after | 36,319 / 36,319 |

### Scalar precision

| Measure | Count |
|---|---:|
| Calls | 41,000 |
| Expressions changed | 0 |
| Full-width changes | 0 |
| Sub-width changes | 0 |

## Critical ablations

The 41K columns are `direct / OKZ / NG / ERR`. MaskSpark columns are
`direct / semantic-only / NG / ERR` at WORD64. Negative columns are
`structural collisions / semantic collisions / errors`.

| Mode | 41K | MaskSpark v8 | `target+1` | Result |
|---|---:|---:|---:|---|
| Baseline | 41000 / 0 / 0 / 0 | 41 / 6 / 2 / 0 | 0 / 0 / 0 | Qualified 41K; two existing MS NGs |
| A: `project_low` off | 41000 / 0 / 0 / 0 | 24 / 17 / 6 / 2 | 0 / 0 / 2 | Unique MS coverage is lost; 4 NG + 2 ERR beyond baseline |
| B: all LOW off | 41000 / 0 / 0 / 0 | 21 / 20 / 6 / 2 | 0 / 0 / 2 | Simple `R_k` still recovers direct forms |
| C: `merge_hidden` off | 40994 / 0 / 6 / 0 | 41 / 6 / 2 / 0 | 0 / 0 / 0 | Six 41K NGs |
| D: `merge_hidden` unary-only | 40994 / 0 / 6 / 0 | 41 / 6 / 2 / 0 | 0 / 0 / 0 | Same measured result as full merge |
| E: `variable_substitution` off | 40970 / 0 / 30 / 0 | 41 / 6 / 2 / 0 | 0 / 0 / 0 | Thirty 41K NGs |
| F: predecessor off | 40947 / 0 / 53 / 0 | 38 / 9 / 2 / 0 | 0 / 0 / 0 | Broad unique contribution |
| G: order off | 40999 / 0 / 1 / 0 | 38 / 9 / 2 / 0 | 0 / 0 / 0 | Narrow but independent contribution |
| H: filtered cut off | 40946 / 0 / 54 / 0 | 35 / 12 / 2 / 0 | 0 / 0 / 0 | Union of F and G regressions |
| I: scalar precision off | 41000 / 0 / 0 / 0 | 41 / 6 / 2 / 0 | 0 / 0 / 0 | No measured gate delta |

The exact WORD64 MaskSpark changes from baseline were:

- A: direct became semantic-only at 1, 2, 3, 4, 8, 9, 10, 42, 43, 44,
  46; direct became NG at 5, 11, 38, 39; direct became ERR at 7, 14.
- B: A's changes plus direct became semantic-only at 20, 27, 28, 29; the
  semantic-only case 49 became direct.
- C and D: no MaskSpark status changes.
- E: direct became semantic-only at 19; semantic-only case 49 became direct.
- F: direct became semantic-only at 6, 12, 13.
- G: direct became semantic-only at 16, 17, 18.
- H: the union of F and G's direct-status changes.
- I: no MaskSpark status changes.

The project-low failures in A are exactly:

- NG: `5/v7-legacy`, `11/v7-legacy`, `38/prefix-carry-8`,
  `39/prefix-carry-affine-8`.
- ERR: `7/v7-legacy`, `14/v7-legacy` (`TooManyVariables`, found 31, max 20).
- `target+1` errors: `7/v7-legacy`, `14/v7-legacy`.

### Exact newly failing 41K IDs

`project_low` off and all LOW off have no 41K regressions. The other exact
sets are:

- `merge_hidden` off: `loki_tiny:14545`, `loki_tiny:23816`,
  `qsynth_ea:53`, `qsynth_ea:165`, `qsynth_ea:249`, `qsynth_ea:260`.
- `merge_hidden` unary-only: the same six IDs as `merge_hidden` off.
- `variable_substitution` off: `loki_tiny:2852`, `loki_tiny:4629`,
  `loki_tiny:6987`, `loki_tiny:7618`, `loki_tiny:8008`,
  `loki_tiny:8060`, `loki_tiny:8635`, `loki_tiny:8694`,
  `loki_tiny:9064`, `loki_tiny:9144`, `loki_tiny:11363`,
  `loki_tiny:12023`, `loki_tiny:13240`, `loki_tiny:13616`,
  `loki_tiny:13721`, `loki_tiny:13814`, `loki_tiny:14574`,
  `loki_tiny:14589`, `loki_tiny:17380`, `loki_tiny:19587`,
  `loki_tiny:19704`, `loki_tiny:21003`, `loki_tiny:22582`,
  `loki_tiny:22946`, `loki_tiny:23335`, `loki_tiny:23452`,
  `qsynth_ea:88`, `qsynth_ea:120`, `qsynth_ea:132`, `qsynth_ea:160`,
  `qsynth_ea:343`.
- Predecessor off: `loki_tiny:2319`, `loki_tiny:2499`, `loki_tiny:2631`,
  `loki_tiny:2729`, `loki_tiny:3312`, `loki_tiny:3424`, `loki_tiny:3919`,
  `loki_tiny:4081`, `loki_tiny:4220`, `loki_tiny:4310`, `loki_tiny:4549`,
  `loki_tiny:7246`, `loki_tiny:7492`, `loki_tiny:8173`, `loki_tiny:8341`,
  `loki_tiny:8584`, `loki_tiny:9594`, `loki_tiny:9810`, `loki_tiny:12529`,
  `loki_tiny:12689`, `loki_tiny:13078`, `loki_tiny:13174`, `loki_tiny:14143`,
  `loki_tiny:14221`, `loki_tiny:14369`, `loki_tiny:14497`, `loki_tiny:14545`,
  `loki_tiny:16073`, `loki_tiny:17633`, `loki_tiny:17818`, `loki_tiny:17825`,
  `loki_tiny:17972`, `loki_tiny:17980`, `loki_tiny:17981`, `loki_tiny:18104`,
  `loki_tiny:18200`, `loki_tiny:18631`, `loki_tiny:18685`, `loki_tiny:19597`,
  `loki_tiny:19704`, `loki_tiny:19966`, `loki_tiny:22103`, `loki_tiny:23004`,
  `loki_tiny:23488`, `loki_tiny:23810`, `loki_tiny:23816`, `loki_tiny:24044`,
  `loki_tiny:24286`, `loki_tiny:24338`, `loki_tiny:24490`, `loki_tiny:24796`,
  `loki_tiny:24869`, `loki_tiny:24932`.
- Order off: `loki_tiny:17380`.
- Filtered cut off: the predecessor set plus `loki_tiny:17380`.
- Scalar precision off: none.

### Supplementary complement-orbit ablation

An additional diagnostic ablation, not part of A–I, disabled complement-orbit
canonicalization. It produced `40857 / 143 / 0 / 0` on 41K. The exact 143
OKZ IDs were:

```text
qsynth_ea:3, qsynth_ea:9, qsynth_ea:19, qsynth_ea:20, qsynth_ea:23, qsynth_ea:24, qsynth_ea:25, qsynth_ea:29, qsynth_ea:31, qsynth_ea:33, qsynth_ea:38, qsynth_ea:41, qsynth_ea:48, qsynth_ea:58, qsynth_ea:59, qsynth_ea:61, qsynth_ea:63, qsynth_ea:77, qsynth_ea:78, qsynth_ea:87, qsynth_ea:91, qsynth_ea:92, qsynth_ea:95, qsynth_ea:98, qsynth_ea:100, qsynth_ea:108, qsynth_ea:111, qsynth_ea:113, qsynth_ea:114, qsynth_ea:118, qsynth_ea:119, qsynth_ea:120, qsynth_ea:125, qsynth_ea:130, qsynth_ea:132, qsynth_ea:133, qsynth_ea:134, qsynth_ea:135, qsynth_ea:137, qsynth_ea:139, qsynth_ea:143, qsynth_ea:147, qsynth_ea:151, qsynth_ea:154, qsynth_ea:156, qsynth_ea:157, qsynth_ea:158, qsynth_ea:160, qsynth_ea:165, qsynth_ea:174, qsynth_ea:180, qsynth_ea:183, qsynth_ea:185, qsynth_ea:186, qsynth_ea:198, qsynth_ea:199, qsynth_ea:200, qsynth_ea:203, qsynth_ea:213, qsynth_ea:217, qsynth_ea:220, qsynth_ea:223, qsynth_ea:231, qsynth_ea:232, qsynth_ea:234, qsynth_ea:241, qsynth_ea:243, qsynth_ea:244, qsynth_ea:248, qsynth_ea:252, qsynth_ea:256, qsynth_ea:260, qsynth_ea:270, qsynth_ea:271, qsynth_ea:277, qsynth_ea:289, qsynth_ea:294, qsynth_ea:298, qsynth_ea:306, qsynth_ea:307, qsynth_ea:309, qsynth_ea:311, qsynth_ea:315, qsynth_ea:320, qsynth_ea:326, qsynth_ea:327, qsynth_ea:330, qsynth_ea:337, qsynth_ea:340, qsynth_ea:343, qsynth_ea:345, qsynth_ea:349, qsynth_ea:350, qsynth_ea:351, qsynth_ea:360, qsynth_ea:364, qsynth_ea:366, qsynth_ea:367, qsynth_ea:368, qsynth_ea:369, qsynth_ea:370, qsynth_ea:371, qsynth_ea:374, qsynth_ea:383, qsynth_ea:389, qsynth_ea:390, qsynth_ea:391, qsynth_ea:399, qsynth_ea:414, qsynth_ea:418, qsynth_ea:423, qsynth_ea:431, qsynth_ea:433, qsynth_ea:443, qsynth_ea:444, qsynth_ea:451, qsynth_ea:458, qsynth_ea:462, qsynth_ea:466, qsynth_ea:470, qsynth_ea:481, qsynth_ea:486, qsynth_ea:497, syntia:47, syntia:75, syntia:121, syntia:134, syntia:201, syntia:213, syntia:222, syntia:234, syntia:302, syntia:307, syntia:310, syntia:344, syntia:347, syntia:353, syntia:371, syntia:379, syntia:431, syntia:432, syntia:472, syntia:483
```

## Performance: fresh release processes

The performance batch used release binaries and fresh processes in an
AB/BA-style sequence. Each process ran five measured passes; the table shows
the two fresh-process totals and their median, followed by the median of the
reported latency columns. The runs are visibly noisy, so these values are
directional rather than a claim of a stable percentage improvement.

| Mode | Fresh totals | Median total | Median expr/s from median total | Median p50 | Median p95 | Median p99 | Median max |
|---|---|---:|---:|---:|---:|---:|---:|
| Baseline | 12.09 s, 6.68 s | 9.39 s | 4,369 | 85 µs | 741 µs | 2.03 ms | 81.1 ms |
| A: project LOW off | 10.91 s, 6.53 s | 8.72 s | 4,702 | 77 µs | 688 µs | 1.88 ms | 51.9 ms |
| B: all LOW off | 7.57 s, 8.03 s | 7.80 s | 5,256 | 74 µs | 577 µs | 1.56 ms | 344 ms |
| C: merge hidden off | 7.94 s, 6.78 s | 7.36 s | 5,571 | 62 µs | 589 µs | 1.56 ms | 47.7 ms |
| D: variable substitution off | 8.67 s, 11.23 s | 9.95 s | 4,121 | 75 µs | 921 µs | 2.52 ms | 50.1 ms |
| E: filtered cut off | 5.00 s, 6.46 s | 5.73 s | 7,154 | 64 µs | 429 µs | 1.08 ms | 63.4 ms |

No max-RSS measurement was available from the corpus runner. The timing
instrumentation itself records only mechanism-local nanoseconds for
`project_low`, merge proof/synthesis, and filtered-cut relation proofs.

## Production LOC by subsystem

Counts below use the clean HEAD as the production baseline. The current column
includes the removable census plumbing and therefore is not a proposed final
production size.

| Subsystem | HEAD LOC | Current LOC | Diagnostic delta |
|---|---:|---:|---:|
| Reducer core (`core/src/reduce.rs`) | 515 | 578 | +63 |
| Full LOW (`core/src/reduce/low_prefix.rs`) | 1,947 | 1,947 | 0 |
| MBA solver (`core/src/simplify.rs`) | 882 | 1,263 | +381 |
| Hidden gauge | 292 | 368 | +76 |
| `merge_hidden` | 404 | 433 | +29 |
| Filtered cut | 567 | 639 | +72 |
| Scalar precision | 95 | 116 | +21 |
| Lambda / variable substitution | 150 | 150 | 0 |

The full LOW implementation is the largest single removable concept at 1,947
lines. Its measured unique MaskSpark value means it should be replaced by a
smaller proven capability before deletion, not deleted solely because it is
large.

## Scorecard and answers to the key questions

| Mechanism | Unique measured coverage | Runtime evidence | Conceptual cost | Recommendation |
|---|---|---|---|---|
| Simple `R_k` LOW | No 41K count delta; recovers direct forms in MaskSpark rows 20, 27, 28, 29 versus all LOW off | 3,067 re-reductions; 128 changed operands | Low: a local smaller-ring reduction under an explicit prefix mask | **KEEP** |
| Full `project_low` | Six MaskSpark rows (5, 7, 11, 14, 38, 39) regress to NG/ERR without it; no 41K delta | 1,997 calls, 12.18 ms measured on 41K; 1,995 fallback paths | High: large Shadow/BoolAnf/Cylinder-style subsystem | **KEEP for now; replace/isolate later** |
| Hidden complement gauge | Disabling canonicalization caused 143 41K OKZ results | 97,265 hide calls; exact reuse is frequent | Low/medium: one solver-local width-aware canonical representation | **KEEP** |
| `merge_hidden` unary | Six 41K regressions when all merge is removed | 198 changed calls; 210 successful aliases | Medium: hidden relation proof and alias bookkeeping | **KEEP** |
| `merge_hidden` binary | No unique 41K or MaskSpark coverage versus unary-only | 148 successful binary candidates; included in 112 ms synthesis time | High relative to its measured marginal value | **DELETE/REPLACE after wider-oracle confirmation** |
| Variable substitution | 30 unique 41K NGs when disabled | 8,506 one-hidden and 1,262 two-hidden successes | Medium: two bounded lambda families and signature repair | **KEEP** |
| Filtered predecessor | 53 unique 41K NGs; 3 MaskSpark direct losses | 1,032 certified relations; 186 improving; 112 winners | Low: one ray/principal-containment producer | **KEEP** |
| Filtered order | 1 unique 41K NG; 3 MaskSpark direct losses | 99,991 comparisons; 40,445 subset proofs; 6 winners | Low: one Boolean-order producer | **KEEP** |
| Scalar precision | No 41K, MaskSpark, or tested width 1–5 delta | 41,000 calls, 0 changes | Low/medium: explicit width/valuation normalization | **KEEP pending NG54/wider widths** |

Answers in direct terms:

1. Full LOW supplies coverage not recovered by simple `R_k` on six v8 rows,
   while simple LOW still improves several direct canonical forms.
2. The six rows are concentrated in the legacy wide Boolean/carry family and
   two high-variable rows; the data supports a narrower replacement hypothesis,
   but does not identify a safe replacement yet.
3. `merge_hidden` still provides unique corpus coverage.
4. Binary synthesis is not necessary for any measured 41K or MaskSpark gate;
   the unary-only ablation is identical to full merge on those gates.
5. Variable substitution is independently necessary on 30 41K cases.
6. Predecessor has the broad responsibility; order is a narrow independent
   chart, not dead code.
7. The filtered cut remains explainable as two certificate producers feeding
   one finite improving quotient.
8. Scalar precision is not required by the measured gates, but the available
   width tests and missing NG54 oracle are insufficient evidence for deletion.

## Final recommendation

### KEEP

- simple demanded-width `R_k` reduction;
- width-typed hidden definitions, complement gauge, and recursive restoration;
- unary/constant hidden relation merging;
- variable substitution;
- filtered predecessor and Boolean-order certificate producers;
- one finite improving quotient;
- scalar precision until NG54 and broader narrow-width coverage are restored.

### DELETE / REPLACE, but not in this mission

- binary hidden-relation synthesis, subject to confirmation against NG54 or a
  larger exact oracle;
- the full `project_low` subsystem only after a smaller targeted implementation
  reproduces the six unique MaskSpark cases and the negative gates.

### Requires further test

- fix or separately characterize MaskSpark v8 rows 40–41;
- locate the exact NG54 oracle;
- rerun the scorecard after any narrower LOW replacement;
- obtain stable multi-process timing and RSS measurements without diagnostic
  counters included in the hot path.

The measured architecture is therefore closest to:

```text
Reducer
  -> simple demanded precision

MBASolver
  -> width-typed hidden variables + complement gauge
  -> unary hidden relations / variable substitution
  -> polynomial solve
  -> filtered predecessor/order certificates
  -> one improving quotient
  -> restore hidden definitions

outer fixed point
```

with full `project_low` retained temporarily as a proven coverage dependency,
not as an invitation to add another parallel algebraic architecture.

# Rumba corpus results

## Production pipeline

The public `simplify_mba` entry point now runs:

```text
ordinary RUMBA simplification
-> P7e dependency closure
-> guarded P9-L projection
-> least-cost exact AST
```

P9-L is attempted after P7e and its candidate is retained only when it is
strictly smaller and the sequential carry automaton proves exact equivalence.
P8 is disabled in default builds and is available only through the explicit
`p8-experiments` feature. P9-Poly, P9-P, and P10 remain experiments and are not
production stages.
No SAT or Z3 backend is used.

## Official 41,000-expression corpus

Reproduced in release mode with all features enabled:

```console
just test
```

The seven dataset tests completed in **119.46 seconds**, excluding compilation.

| Dataset | Cases | OK | OKZ | NG | Success rate | Median time |
|---|---:|---:|---:|---:|---:|---:|
| `syntia.csv` | 500 | 480 | 20 | 0 | 100.00% | 330.85 us |
| `mba_obf_nonlinear.csv` | 1,000 | 1,000 | 0 | 0 | 100.00% | 1.71 ms |
| `mba_obf_linear.csv` | 1,000 | 1,000 | 0 | 0 | 100.00% | 2.26 ms |
| `qsynth_ea.csv` | 500 | 369 | 131 | 0 | 100.00% | 1.64 ms |
| `mba_flatten.csv` | 3,000 | 3,000 | 0 | 0 | 100.00% | 166.22 us |
| `neureduce.csv` | 10,000 | 10,000 | 0 | 0 | 100.00% | 909.17 us |
| `loki_tiny.csv` | 25,000 | 25,000 | 0 | 0 | 100.00% | 401.97 us |
| **Total** | **41,000** | **40,849** | **151** | **0** | **100.00%** | — |

Compared with the previous ordinary-simplifier baseline:

| Dataset | Previous NG | Production NG | Status movement |
|---|---:|---:|---:|
| `qsynth_ea.csv` | 19 | 0 | 19 NG -> OKZ |
| `loki_tiny.csv` | 82 | 0 | 82 NG -> OK |
| All other datasets | 0 | 0 | unchanged |
| **Total** | **101** | **0** | **101 resolved** |

## What the zero-NG result means

- **OK** means the simplified source structurally matches the simplified
  expected expression.
- **OKZ** means the two forms differ structurally, but production simplification
  proves their residual to be zero.
- **NG** means the corpus harness could not establish either result.

The official result is therefore exactly **41,000/41,000 accepted, 0 NG**.
It must not be misread as autonomous source synthesis of all 101 historical NG
cases: the no-oracle ablation of `P7e -> P9-L` directly produces a smaller
candidate for **85/101**. The remaining 16 are accepted by the official corpus
because the exact pipeline proves the source/expected residual equal to zero.

Those 16 autonomous-source blockers for the production pipeline are:

- 12 expressions with true word multiplication:
  `qsynth_ea.csv:{13,53,77,125,134,210,234,260,294,369,481,486}`;
- 4 supported but non-linearizable expressions:
  `qsynth_ea.csv:{25,114,139,423}`.

Their full post-P7e expressions are recorded in `p10_results.md` under
“Expressions with word multiplication” and “Expressions without word
multiplication”.

The opt-in P11 experiments now reduce all 16 sources without using `expected`
for candidate generation. P11a reaches or beats the corpus target cost on 6 of
the 12 multiplication cases; P11b reaches or beats it on all 4 supported cases.
Thus the experimental aggregate is 10/16 at or below target cost, while the
production pipeline and its 85/101 autonomous-generation figure remain
unchanged. See `p11a_results.md` and `p11b_results.md`.

## Performance delta

Coverage improved from 40,899/41,000 to 41,000/41,000, but the current complete
pipeline is materially slower:

| Measure | Previous baseline | Production | Ratio |
|---|---:|---:|---:|
| Complete corpus test time | 3.11 s | 119.46 s | 38.4x |
| Syntia median | 20.93 us | 330.85 us | 15.8x |
| Nonlinear MBA median | 96.52 us | 1.71 ms | 17.7x |
| Linear MBA median | 58.43 us | 2.26 ms | 38.7x |
| QSynth median | 301.41 us | 1.64 ms | 5.4x |
| Flatten median | 20.84 us | 166.22 us | 8.0x |
| NeuReduce median | 56.24 us | 909.17 us | 16.2x |
| Loki median | 39.33 us | 401.97 us | 10.2x |

The next production optimization should preserve the exact stages while adding
a cheap trigger and/or memoization so that P7e and P9-L are not paid on inputs
already settled by ordinary simplification.

## Validation

- 60 core unit tests passed.
- 16 active bitwise-frontier integration tests passed; 5 known historical
  tests remain ignored.
- The seven release corpus tests passed.
- P8's historical integration tests are pinned to the ordinary diagnostic
  baseline, so they continue to test the P8 ablation independently of the
  public production pipeline.

Timing values are machine-dependent and are intended for comparisons on the
same environment.

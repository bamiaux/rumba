# P9-lite carry-state diagnostic

## Method

P9 compiles the input and candidate into one structurally interned DAG. `Add`
and `Scale` nodes own stable carry slots; bitwise nodes own no state. Starting
with zero carries, the verifier explores every current-variable-bit assignment
LSB-first for all 64 levels. It compares the two output bits directly and uses
`rem_euclid(2)` / `div_euclid(2)` for signed carries. Final carries are ignored,
matching arithmetic modulo `2^64`. A first mismatch reconstructs a replayable
word-level counterexample. State, variable, DAG-node, and candidate limits return
`Unknown` without changing the input.

Candidates are generated without consulting `expected`:

- a bitwise candidate whose uniformity is checked on `{0, mask}`, followed by
  conjunction-basis interpolation on `{0, 1}`;
- an affine candidate from evaluations at zero and at each unit vector.

At most two strictly smaller candidates are certified, in deterministic cost
order. The normal `simplify_mba` path is unchanged.

## Decisive 41-case reconciliation

The diagnostic matrix is in `p9_41_matrix.csv`. Each row reconstructs the exact
residual in memory through the same sequence that produced
`remaining_unresolved.csv`:

1. simplify `source` and `expected`;
2. form `expected - source`;
3. diagnose hidden atoms;
4. run the P7e/P8 experimental pipeline;
5. retain its nonzero `pipeline.result` AST.

The diagnostic asserts that the rendered expression still matches the CSV, but
does not parse that rendering back as the P9 input.

| Check | Result |
|---|---:|
| Exact residual ASTs reconstructed | 41/41 |
| Distinct exact structural hashes | 41/41 |
| Exact residuals supported by Rust P9 | 41/41 |
| Unsupported `Mul` nodes in exact residuals | 0/41 |
| Test A: `verify(diff-produced, 0)` | 41/41 `ProvedEquivalent` |
| Test B: `verify(source, expected)` | 41/41 `ProvedEquivalent` |
| Rust bitwise candidate | 41/41 generated as `0x0` |
| Rust affine candidate | 41/41 generated as `0x0` |
| Rust bitwise candidate certified | 41/41 `ProvedEquivalent` |
| Rust affine candidate certified | 41/41 `ProvedEquivalent` |
| Counterexamples or failure bits | 0/41 |

| Verification | Median states | p95 states | Max states |
|---|---:|---:|---:|
| Test A, exact residual against zero | 14 | 62 | 99 |
| Test B, source against expected | 6 | 14 | 26 |

The Python prototype and its per-line candidates are not present in the
repository or supplied attachments, so the cross-verification columns remain
`unavailable`. The subsequently supplied aggregate family counts are reproduced
exactly by Rust on the 41 source expressions: 23 affine and 18 bitwise.

## Two distinct experiments

The 41-case diagnostic and the autonomous benchmark answer different
questions:

- `verify(diff-produced, 0)` proves that the exact verifier recognizes the
  nullity of residuals constructed with the measurement oracle;
- `simplify_mba(source) -> candidate generation -> P9` measures whether P9 can
  autonomously synthesize a smaller source expression without using
  `expected`.

The first result cannot be used as a 41/41 autonomous-synthesis result. Its two
Rust generators both return zero because their input is already a semantically
zero residual.

Running the generators on the 41 corresponding source expressions exposed a
separate Rust bug. The bitwise generator correctly used `{0, mask}` to test
uniform bitwise behavior, but incorrectly passed the resulting mask-valued
outputs into an arithmetic conjunction basis that expects `{0, 1}` samples.
This produced false candidates for functions including NOT, OR, and XOR. After
separating the uniformity cube from the interpolation cube, Rust reproduces the
prototype result:

| Source candidate family | Resolved |
|---|---:|
| Affine (`0` and units) | 23 |
| Bitwise (`0/mask` gate, `0/1` interpolation) | 18 |
| Total | 41/41 |

## Scalar boundary normalization

`Expr::Display` renders `Scale(constant, expression)` in multiplication syntax.
Parsing that text produces `Mul(Const(constant), expression)`. P9 now performs
a targeted boundary normalization before fragment checking and compilation:

```text
Mul(Const(c), e)          -> Scale(c, e)
Mul(e, Const(c))          -> Scale(c, e)
Mul(Const(a), Scale(b,e)) -> Scale(a*b, e)
```

Only scalar factors are absorbed, modulo the target width. A product containing
two or more nonconstant factors remains `Mul` and remains unsupported. Failed
or budget-limited analysis still returns the exact original input.

| Serialization control | Before | After normalization |
|---|---:|---:|
| CSV rendering reparses to the identical raw AST | 0/41 | 0/41 |
| Reparsed CSV residual is accepted by P9 | 0/41 | 41/41 |

The text format is therefore still not a structurally faithful serialization.
The matrix's structural hashes use explicit variant tags, so `Scale` and `Mul`
cannot collide. A reversible typed AST payload remains preferable for durable
cross-implementation fixtures.

## Autonomous exact-AST benchmark

`p9_lite_corpus.rs` was rerun directly on the 101 simplified source ASTs. It
does not serialize an intermediate expression and does not use `expected` for
candidate generation.

| Variant | Resolved | Remaining | P9 inputs | Candidate generated | Certification attempted | Candidate certified and reduced | Unsupported exact AST |
|---|---:|---:|---:|---:|---:|---:|---:|
| P9 alone | 82 | 19 | 101 | 88 | 88 | 82 | 13 |
| P7e then P9 | 85 | 16 | 85 | 73 | 73 | 69 | 12 |
| P7e then P8 then P9 | 85 | 16 | 58 | 46 | 46 | 42 | 12 |

The P7e/P8 totals include respectively 16 and 43 expressions resolved by the
prepasses before P9.

| Variant | Candidate generation ms | Verification ms | Total ms | Median states | p95 states | Max states |
|---|---:|---:|---:|---:|---:|---:|
| P9 alone | 1.993 | 137.290 | 139.283 | 11 | 53 | 83 |
| P7e then P9 | 1.635 | 116.692 | 154.530 | 11 | 62 | 83 |
| P7e then P8 then P9 | 1.666 | 119.441 | 192.883 | 15 | 78 | 83 |

All 13 unsupported P9-alone inputs are QSynth expressions whose simplified,
in-memory AST contains genuine word-by-word multiplication such as `v1*v1`,
`v0*v1`, or `v1*(v0&v2)`. Every reported `Mul` has at least two nonconstant
factors. They are not CSV-induced scalar products and cannot soundly be
rewritten as `Scale`.

## Parallel coverage matrix

`p9_parallel_coverage.csv` runs both branches from the same ordinarily
simplified base AST. `expected` is used only to score the results.

| Coverage class | Lines |
|---|---:|
| P9 from base | 82 |
| P7e/P8 from base | 43 |
| Both | 40 |
| P9 only | 42 |
| P7e/P8 only | 3 |
| Neither | 16 |
| Union | 85 |

The expected `41/52/8/0` split does not occur. The earlier count of 60 for
P7e/P8 was obtained by applying that diagnostic pipeline to the oracle-built
residual `expected - source`; the autonomous branch `P7e/P8(base)` resolves 43.

All 41 lines from `remaining_unresolved.csv` are now resolved by `P9(base)`.
Their accepted candidates split exactly into 23 affine and 18 bitwise cases.
Of the 13 true nonlinear P9 inputs, P7e/P8 resolves one and leaves twelve
unresolved. The remaining four union failures are `CandidateRejected`, giving
16 unresolved lines in total.

A cost-only choice among `base`, `P7e/P8(base)`, and `P9(base)` selects P9 on
78 lines, P7e/P8 on 7, and the base on 16. It matches the measurement target on
83 lines rather than the full union's 85 because, on two lines, a smaller
P7e/P8 form is selected over a P9 form that normalizes to `expected`. Both are
certified transformations of the same base; this difference concerns the
benchmark's target-normal-form metric, not semantic correctness.

## P9-L linear projection

P9-L replaces the affine and bitwise proposals with one canonical projection.
It evaluates the normalized input on the numeric cube `{0,1}^t`, then calls the
same `conjunction_sum_from_signature` decomposition used by RUMBA's
`MBASolver`. P9 certifies that unique projection once. The `{0,mask}` cube is
not needed for membership testing.

| Metric | P9-L(base) | P7e then P9-L |
|---|---:|---:|
| Sources | 101 | 101 |
| Prepass resolved | 0 | 16 |
| P9-L inputs | 101 | 85 |
| Total pipeline resolved | 82 | 85 |
| Linear projections proved and reduced | 82 | 69 |
| Linear projection counterexamples | 6 | 4 |
| Unsupported true `Mul` | 13 | 12 |
| Budget exceeded | 0 | 0 |
| Old resolved lines lost | 0 | 0 |
| Old `CandidateRejected` newly resolved | 0 | 0 |
| Median states | 11 | 11 |
| p95 states | 53 | 62 |
| Max states | 83 | 83 |

The resolved line sets are exactly identical to those of the corrected two-
generator implementation: 82 lines from base and 85 after P7e. The four
post-P7e counterexamples are `qsynth_ea.csv:25`, `:114`, `:139`, and `:423`.
They are proven not equivalent to their unique linear projection; this is a
membership result rather than a heuristic candidate miss.

One release run measured:

| Variant | Candidate generation ms | Verification ms | Total ms |
|---|---:|---:|---:|
| Two generators from base | 1.993 | 137.290 | 139.283 |
| P9-L from base | 0.976 | 112.747 | 113.723 |
| P7e then two generators | 1.635 | 116.692 | 154.530 |
| P7e then P9-L | 0.855 | 97.704 | 130.841 |

P9-L therefore passes the minimum and architectural gates: no regression, one
canonical candidate, explicit non-membership counterexamples, and lower time in
this run. It does not pass the functional-gain gate because none of the four
old post-P7e rejections becomes proved.

## P9-Poly bounded experiment

P9-Poly is implemented as a separate layer; general word multiplication was
not added to the P9 carry automaton. On every `Mul` node it identifies the
maximal multiplication-free factors, computes their canonical P9-L projection,
and substitutes a factor only after an exact P9 certificate. It then collects
the resulting arithmetic polynomial in a bounded sparse map modulo the target
width. The existing PCT simplifier is invoked only if direct collection did not
produce a strictly smaller candidate.

The implementation has explicit limits for multiplication nodes, factor
occurrences, polynomial degree, sparse monomials, candidate size, and the P9
factor certificates. Any limit or inconclusive certificate preserves the exact
input expression.

| Gate on the 12 post-P7e `Mul` cases | Result |
|---|---:|
| All factors P9-L certified | 9/12 |
| Factor occurrences certified | 126/133 |
| Factor counterexamples | 7 |
| Factor unknown / unsupported | 0 |
| Resolved by direct sparse collection | 0 |
| Resolved by PCT fallback | 0 |
| Candidate changed | 0 |
| Four P9-L counterexamples resolved by P9-Poly | 0/4 |
| Semantic regressions (256 deterministic samples/candidate) | 0 |
| Maximum factor-certificate states | 4 |

The three partially linearizable inputs are `qsynth_ea.csv:234`, `:294`, and
`:369`, with respectively 4, 2, and 1 factor counterexamples. The other nine
have every factor certified. No failure comes from a budget.

One release run measured 0.040 ms for projection generation, 0.452 ms for P9
factor certification, 0.038 ms for sparse collection, and 1.669 ms for the PCT
fallback over all twelve cases.

The zero gain is informative. The products in these targets are generally
nested inside bitwise contexts; arithmetic sparse collection must treat an
enclosing `And`, `Or`, `Xor`, or `Not` as an opaque atom. Consequently it cannot
perform cancellations across those contexts. The current PCT sees essentially
the same boundary. Extending this result to 97/101 would require a new
composition rule or bounded semantic synthesis for polynomial-bitwise
contexts, not merely a larger sparse budget and not general multiplication in
the P9 automaton.

The four post-P7e P9-L counterexamples are also emitted by the benchmark:

| Line | Variables | Explicit `Mul` degree | Source states | Expected states | Counterexample states | Nodes source/expected |
|---|---:|---:|---:|---:|---:|---:|
| `qsynth_ea.csv:25` | 2 | 1 | 22 | 29 | 4 | 50/47 |
| `qsynth_ea.csv:114` | 2 | 1 | 12 | 10 | 4 | 16/16 |
| `qsynth_ea.csv:139` | 3 | 1 | 90 | 99 | 48 | 84/68 |
| `qsynth_ea.csv:423` | 2 | 1 | 29 | 59 | 4 | 25/23 |

Here degree 1 is the structural degree induced by explicit word products; it
does not claim that the enclosing bitwise/arithmetic composition is a linear
MBA. P9-L proves precisely the opposite for these four sources. Running the
bounded P9-Poly path on them also changes and resolves zero cases.

## Safety validation

- exhaustive widths 1 through 6: zero errors for addition, subtraction,
  negative constants, positive and negative scales, AND, OR, XOR, NOT, and
  mixed carry/bitwise expressions;
- replay of a generated counterexample through the normal word evaluator;
- deterministic pseudo-random checks at widths 32 and 64 for every accepted
  candidate in the test set;
- exhaustive checks that the P9 bitwise generator reconstructs AND, OR, XOR,
  and NOT at widths 1 through 6, plus checks at widths 32 and 64;
- scalar-boundary tests for negative, positive, nested, and bitwise-wrapped
  scales, plus a check that a true word-by-word product remains unsupported;
- display/parse support-preservation tests for `Scale`, `Add`, `Not`, `And`,
  `Or`, `Xor`, and a negative constant;
- explicit checks that `Unsupported` and budget `Unknown` preserve the exact
  input expression.
- sparse polynomial cancellation checks across widths 2 through 6;
- exhaustive small-width checks that a P9-L-rejected polynomial factor is not
  substituted and that the result remains exact;
- an explicit sparse-budget failure check that preserves an exact candidate.
- a variable-budget check that prevents exponential factor projection and
  reports the factor as unknown.

The no-oracle 41,000-expression benchmark was not run because none of the
specified 101-case coverage gates is positive.

## Recommendation

The corrected verdict has three parts:

- P9's exact certificate succeeds on all 41 oracle-built residuals;
- P9's autonomous generators also resolve all 41 corresponding sources;
- across all 101 NG, `P9(base)` resolves 82 and its union with autonomous
  P7e/P8 resolves 85, not 101.

For this corpus, prefer **P7e + P9** over **P7e + P8 + P9**: both resolve the
same 85 lines, while P8 adds time and no coverage. Implement that choice as
**P7e + P9-L**: it preserves the exact 85-line set with a single canonical
candidate and lower measured cost. A parallel P9/P7e-P8 branch
also reaches only the same 85-line set. Reaching 101/101 still requires handling
twelve genuine word products and four expressions proven outside the linear
MBA class. The bounded P9-Poly layer safely classifies and collects the twelve
products but does not reduce them, so the measured pipeline remains 85/101.
The current evidence does not justify pretending those products are scalar or
expanding the carry automaton to general multiplication.

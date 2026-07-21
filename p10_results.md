# P10 bounded experiments

All expressions below are exact in-memory AST renderings after the ordinary
RUMBA simplifier and P7e. `expected` is used only to score a result, never to
generate a candidate.

## P10a — alternating PolyNF and BitwiseNF

P10a was run on the nine `Mul` cases for which every factor is P9-L certified,
using exactly the two fixed sequences:

```text
PolyNF -> BitwiseNF -> PolyNF
BitwiseNF -> PolyNF -> BitwiseNF
```

| Line | PolyNF monomials | Max bitwise atoms | Bitwise rewrites | Result |
|---|---:|---:|---:|---|
| 13 | 8 | 2 | 0 | `BitwiseNoChange` |
| 53 | 31 | 3 | 0 | `BitwiseNoChange` |
| 77 | 12 | 2 | 0 | `BitwiseNoChange` |
| 125 | 19 | 3 | 0 | `BitwiseNoChange` |
| 134 | 16 | 3 | 0 | `BitwiseNoChange` |
| 210 | 8 | 3 | 0 | `BitwiseNoChange` |
| 260 | 15 | 2 | 0 | `BitwiseNoChange` |
| 481 | 35 | 3 | 0 | `BitwiseNoChange` |
| 486 | 21 | 3 | 0 | `BitwiseNoChange` |

There are 19 certified semantic bindings. Every binding is already structurally
identical to its canonical P9-L projection. There are no atom-budget or
size-budget aborts. Consequently every exact trace is:

```text
SemanticKey(source) -> PolyNF(source) -> BitwiseNF(source) -> PolyNF(source)
```

The exact expanded trace, including width, ordered variables, modular
coefficients and P9 proof metrics for every key, is emitted by
`core/examples/p10a_corpus.rs`.

The first missing contextual fact is therefore precise: every maximal bitwise
frontier is already its conjunction-basis normal form over the current opaque
arithmetic words. Simplification would require proving relations between those
opaque words; alternating two free normal forms cannot discover them.

## P10c — binary bitwise composition of linear parents

P10c collected every P9-L-certified subexpression as a possible parent and
tested every pair against all sixteen binary bitwise functions. Word-level
observations generated candidates; P9 was the required exact certificate.

| Line | Parents | Pairs | Observation-compatible candidates | Proved |
|---|---:|---:|---:|---:|
| 25 | 6 | 21 | 0 | 0 |
| 114 | 7 | 28 | 0 | 0 |
| 139 | 13 | 91 | 0 | 0 |
| 423 | 8 | 36 | 0 | 0 |

No budget was reached. The four sources are outside the tested
`F(L1,L2)` class for the certified-parent set.

## P10b — contextual factor equality

P10b inspected the seven factors rejected by P9-L in the three partially
linearizable product cases. Each occurrence was used only in its immediate
single-hole multiplicative context `B*z`. A substitution required either:

```text
PCT(B * (e - projection)) == 0
```

or an exact reduced-width P9 proof justified by a guaranteed scalar factor
`2^k` in `B`.

```text
nonlinear_factor_projections=7
pct_attempts=7
reduced_width_attempts=0
contextual_rewrites=0
budget_exceeded=0
```

All seven local coefficients have guaranteed two-adic valuation zero, so no
width reduction is justified. None of the seven complete contextual residuals
reduces to zero by PCT.

## Expressions with word multiplication

### `qsynth_ea.csv:13`

```text
0x2 * v1 + (-0x2) * (0x2 * v1 & v1 * v1) + v1 * v1 & (-0x1) * v1 + (-0x1) * (v0 * v1) + (-0x1) * (v1 * v1 * v1)
```

### `qsynth_ea.csv:53`

```text
(-0x1) + (-0x1) * (v0 & (-0x1) + (-0x2) * (v1 * v1) + (-0x1) * v2 + (-0x1) * (v1 * v2) + (v2 & 0x2 * (v1 * v1) + (-0x1) * (v1 * (v1 & v2)) + v1 * v2) + v1 * (v1 & v2)) + (-0x1) * (v1 & (-0x1) + (-0x2) * (v1 * v1) + (-0x1) * v2 + (-0x1) * (v1 * v2) + (v2 & 0x2 * (v1 * v1) + (-0x1) * (v1 * (v1 & v2)) + v1 * v2) + v1 * (v1 & v2)) + (v0 & v1 & (-0x1) + (-0x2) * (v1 * v1) + (-0x1) * v2 + (-0x1) * (v1 * v2) + (v2 & 0x2 * (v1 * v1) + (-0x1) * (v1 * (v1 & v2)) + v1 * v2) + v1 * (v1 & v2))
```

### `qsynth_ea.csv:77`

```text
(-0x1) * v0 + (-0x1) * v2 + (v0 & v2) + (0x2 * v2 & v0 + v2 + ((-0x1) + (-0x1) * v0 + (-0x1) * v2 & v2 * v2))
```

### `qsynth_ea.csv:125`

```text
v2 + (-0x1) + (-0x1) * (v2 & v3 + (-0x1)) + (v2 & v3 & v3 + (-0x1)) + (v2 + (-0x1) * v4 & (-0x1) * v2 + (-0x1) * (v2 & v3 & v3 + (-0x1)) + (v2 & v3 + (-0x1)) & v3 * v3)
```

### `qsynth_ea.csv:134`

```text
(-0x1) + (-0x1) * (v2 + v4 + (-0x1) + (v2 & v4 & (-0x1) + (-0x1) * v2 + (-0x1) * v4) & v2 + (-0x1) + 0x2 * v4 + (-0x2) * (v4 & v2 + v4) + (-0x1) * (v4 * v4))
```

### `qsynth_ea.csv:210`

```text
v2 & 0x3 * v3 + (-0x2) * (0x2 * v3 & v3 + (-0x1) * v2) + (-0x1) * v2 & (-0x1) * (v3 * v3) + v2 * v3
```

### `qsynth_ea.csv:234`

```text
0x2 * (v2 * v3 & v2 * v3 * (v3 & v2 + (-0x1)) & v3 * v3) + (-0x2) * (v2 * v3 & v3 * v3) + (-0x1) * (v2 * v3 & v2 * v3 * (v3 & v2 + (-0x1))) + (-0x1) * (v2 * v3 * (v3 & v2 + (-0x1)) & v3 * v3) + v2 * v3 + v2 * v3 * (v3 & v2 + (-0x1)) + v3 * v3
```

### `qsynth_ea.csv:260`

```text
(-0x1) + (-0x1) * (v3 + (-0x1) + (-0x1) * (v4 & v0 + (-0x1)) + (v0 & (-0x1) * v0 + (-0x1) * v3 + (v4 & v0 + (-0x1))) & (-0x1) + (-0x2) * (v0 * v4))
```

### `qsynth_ea.csv:294`

```text
0x2 * (v1 * (v2 & v2 + (-0x1))) + (-0x2) * (v1 * v2) + (-0x1) * (v1 + v2 * v2 & 0x2 * (v1 * (v2 & v2 + (-0x1))) + (-0x2) * (v1 * v2))
```

### `qsynth_ea.csv:369`

```text
v2 + (-0x1) + (v3 & (-0x1) + (-0x1) * (v2 * v3) + (-0x1) * (v3 * v3) + (-0x1) * (v3 * (v2 & (-0x1) + (-0x1) * v2 + (-0x1) * v3)))
```

### `qsynth_ea.csv:481`

```text
(-0x2) * v1 + (-0x2) * ((-0x2) * v1 & (-0x1) * v0 + (-0x1) * (v0 & v1) + (-0x1) * (v0 & (-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1)) + (-0x1) * ((-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1) & v0 * v2) + (v0 & (-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1) & v0 * v2)) + (-0x1) * v0 + (-0x1) * (v0 & v1) + (-0x1) * (v0 & (-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1)) + (-0x1) * ((-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1) & v0 * v2) + (v0 & (-0x1) + (-0x1) * v0 + (-0x1) * (v0 & v1) & v0 * v2)
```

### `qsynth_ea.csv:486`

```text
(-0x2) * ((-0x1) * v0 + (v0 & v2 + (-0x1)) & (-0x1) * (v1 * (v0 & v2)) + (-0x1) * (v1 * (v1 & v2)) + (-0x1) * (v2 * (v0 & v2)) + (-0x1) * (v2 * (v1 & v2)) + v1 * (v0 & v1 & v2) + v2 * (v0 & v1 & v2)) + (-0x1) * v0 + (-0x1) * (v1 * (v0 & v2)) + (-0x1) * (v1 * (v1 & v2)) + (-0x1) * (v2 * (v0 & v2)) + (-0x1) * (v2 * (v1 & v2)) + (v0 & v2 + (-0x1)) + v1 * (v0 & v1 & v2) + v2 * (v0 & v1 & v2)
```

## Expressions without word multiplication

### `qsynth_ea.csv:25`

```text
(-0x1) + (-0x1) * (v2 & (-0x1) + (-0x2) * v2 + (-0x1) * v3 + (-0x1) * (v3 & (-0x1) + (-0x2) * v2)) + (-0x1) * (v3 & (-0x1) + (-0x2) * v2 + (-0x1) * v3 + (-0x1) * (v3 & (-0x1) + (-0x2) * v2)) + (v2 & v3 & (-0x1) + (-0x2) * v2 + (-0x1) * v3 + (-0x1) * (v3 & (-0x1) + (-0x2) * v2))
```

### `qsynth_ea.csv:114`

```text
v1 + v4 + (-0x2) * (v4 & v1 + (-0x1)) & (-0x1) * v4 + (v1 & v4)
```

### `qsynth_ea.csv:139`

```text
v1 + v2 + 0x1 + 0x2 * (v0 & (-0x1) + (-0x1) * v1 + (-0x1) * v2) + 0x4 * (v0 & v1 & v2 & v1 + v2 + 0x1 + 0x2 * (v0 & (-0x1) + (-0x1) * v1 + (-0x1) * v2) + (-0x1) * v0) + (-0x2) * (v0 & v1 & v2) + (-0x2) * (v0 & v2 & v1 + v2 + 0x1 + 0x2 * (v0 & (-0x1) + (-0x1) * v1 + (-0x1) * v2) + (-0x1) * v0) + (-0x2) * (v1 & v2 & v1 + v2 + 0x1 + 0x2 * (v0 & (-0x1) + (-0x1) * v1 + (-0x1) * v2) + (-0x1) * v0) + (-0x1) * v0 + (v0 & v2) + (v1 & v2)
```

### `qsynth_ea.csv:423`

```text
v2 + 0x3 * v0 + (v0 & (-0x1) + (-0x3) * v0 + (-0x1) * v2 + (-0x1) * (v2 & (-0x1) + (-0x2) * v0)) + (v2 & (-0x1) + (-0x2) * v0)
```

## Verdict

```text
P10a resolved = 0/9
P10c resolved = 0/4
P10b resolved = 0/12
pipeline coverage remains = 85/101
```

The bounded abstractions fail for a common reason: the missing equalities are
between correlated opaque words and are not global P9-L equalities, binary
bitwise functions of two certified linear subexpressions, or equalities exposed
by an immediate affine multiplication context. A broader contextual analysis
or bounded grammatical synthesis is required for further coverage.

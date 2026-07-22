# M2 external Z3 artifacts

These files validate the 101 historical autonomous rewrites without linking Z3
into Rust or the production pipeline. Cut experiments belong to the separate K
roadmap and are not exported here.

- `required_rewrites.smt2`: 101 mandatory `source != retained output` queries.
- `diagnostic_best.smt2`: 101 non-blocking comparisons from ordinary RUMBA to
  the lowest-cost autonomous candidate observed during export.
- `local_edges.smt2`: three ordered proof edges for each historical line:
  `source -> ordinary -> P7e -> retained output`.
- `*_bits.smt2`: four exact 16-bit slice queries per obligation, used only
  after a direct timeout.
- `cases.tsv` and `local_edges.tsv`: stable typed-AST hashes, pair hashes,
  residual hashes and chain hashes.
- `rewrite_comparison.tsv` and `comparison_status.tsv`: retained-versus-best
  dumps with ASTs, hashes and independent Z3 statuses.
- `gate_cases.tsv` and `m2_gate_summary.txt`: the mandatory M2 gate.
- `diagnostic_summary.txt`: the explicitly non-blocking best-candidate suite.
- `required_unknown_dedup.tsv` and `local_unknown_dedup.tsv`: M2b groups.
- `structured_lemmas.smt2` and `structured_bridges.smt2`: M2d bottom-up local
  rewrites and final congruence bridges for the 17 direct mandatory timeouts.
- `structured_case_results.tsv`: effective direct-or-sliced result for every
  structured chain.

The current completed run used Z3 4.16.0. Direct queries had a 5-second timeout
and slice fallbacks had a 30-second timeout. Wall-clock timing is intentionally
not reported because the host was under concurrent load.

Current mandatory result:

```text
required_total=101
validated=87
direct_unsat=84
complete_local_chains=52
complete_structured_chains=3
sat=0
incomplete_chains=14
gate=FAIL
```

The separate diagnostic suite is `81 UNSAT / 20 UNKNOWN / 0 SAT` and cannot
block M2. The 17 direct mandatory `UNKNOWN` results are unique source/candidate
pairs, residuals and chains. Structured congruence closes lines 25, 77 and 125;
14 mandatory chains remain incomplete. Per-case caches are resumable, but are
working data and are not part of the committed artifacts. No long brute-force
retry is part of the default protocol.

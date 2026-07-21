# P11a-2 — compression hiérarchique exacte

## Architecture

P11a reste expérimental et n'est pas dans le pipeline de production
`RUMBA -> P7e -> P9-L`.

```text
compression exacte d'un sous-terme
-> archivage de 8 surfaces au plus par ProofKey
-> conservation Pareto (coût, racine, facteur commun, support bitwise)
-> promotion des surfaces comme atomes du parent
-> trois tours bottom-up fixes
```

Les candidats viennent uniquement de transformations génériques. `expected`
reste un oracle de diagnostic et n'est jamais injecté dans la génération. Un
candidat strictement moins cher doit avoir la même empreinte observationnelle,
puis la même `ProofKey` ou une preuve exacte du résidu par le prouveur figé
RUMBA/P7e/P9. Ce prouveur ne rappelle ni P11a ni le simplificateur public.

La dernière correction normalise les facteurs additifs uniformément négatifs,
par exemple `-v1 - v2 -> -(v1 + v2)`, et permet deux factorisations locales
enchaînées dans un tour, sous la borne `max_components` existante.

## Micro-gate

| Ligne | Entrée P7e | P11a précédent | P11a-2 final | Cible diagnostique |
|---:|---:|---:|---:|---:|
| 13 | 26 | 15 | **15** | 24 |
| 53 | 110 | 82 | **41** | 27 |
| 486 | 91 | 39 | **19** | 23 |

La ligne 486 passe de 36 à 19 avec la dernière factorisation, sans preuve
secondaire. La ligne 53 reste à 41 ; sa cible passe le certificat P7e lorsqu'on
la propose, mais la génération autonome ne la reconstruit pas.

Les 7 tests unitaires P11a passent après la correction.

## Benchmark final des douze cas `Mul`

Lignes : `13, 53, 77, 125, 134, 210, 234, 260, 294, 369, 481, 486`.

```text
mul_cases=12
resolved=6
cost_reduced=12
reduction_ge_25_percent=8
reduction_ge_50_percent=5
pareto_wins_over_best_only=0
surfaces_generated=770
secondary_proofs=27
budgets_reached=2
best_only_time_ms=median:2480.072,p95:71730.382,max:71730.382
pareto_time_ms=median:2452.375,p95:72593.437,max:72593.437
expected_used_for_candidate_generation=false
```

| Ligne | Entrée | Best-only | Pareto | Cible | Résolue | Temps Pareto |
|---:|---:|---:|---:|---:|---:|---:|
| 13 | 26 | 15 | 15 | 24 | oui | 0,73 s |
| 53 | 110 | 41 | 41 | 27 | non | 43,49 s |
| 77 | 24 | 16 | 16 | 15 | non | 2,45 s |
| 125 | 38 | 37 | 37 | 40 | oui | 1,09 s |
| 134 | 32 | 28 | 28 | 23 | non | 1,70 s |
| 210 | 23 | 15 | 15 | 27 | oui | 0,29 s |
| 234 | 65 | 16 | 16 | 16 | oui | 32,37 s |
| 260 | 31 | 28 | 28 | 13 | non | 2,30 s |
| 294 | 33 | 29 | 29 | 13 | non | 0,74 s |
| 369 | 26 | 13 | 13 | 14 | oui | 3,84 s |
| 481 | 94 | 41 | 41 | 14 | non | 72,59 s |
| 486 | 91 | 19 | 19 | 23 | oui | 3,94 s |

## Verdict et garde

P11a réduit les douze sources et en amène six au coût cible ou en dessous.
L'archive Pareto ne gagne toutefois aucun cas. Les lignes 53 et 481 dominent
le temps, et les bornes déterministes actuelles ne bornent pas le coût interne
d'une normalisation ou d'une preuve.

La garde retenue est donc architecturale : P11a reste un outil expérimental
explicitement invoqué. Il ne doit pas être activé largement ni ajouté au
pipeline de production dans cet état.

# État de travail — snapshot P11

Date : 2026-07-21  
Branche : `bax`  
Parent avant snapshot : `54a7e48`

Ce commit est volontairement un snapshot brut demandé avant une pause. Il ne
doit pas être poussé automatiquement.

## État architectural

- Z3/SAT a été retiré et ne doit pas être réintroduit.
- Le pipeline de production est `RUMBA -> P7e -> P9-L`.
- P8 est désactivé par défaut et reste derrière `p8-experiments`.
- P9-L, P9-Poly/P9-P, P10a/P10b/P10c et P11a sont présents avec leurs exemples
  et rapports expérimentaux.
- P11a reste expérimental : il n'est pas branché dans le pipeline de production.
- P11b n'est pas encore implémenté.

## P11a-2 avant la dernière correction

P11a-2 utilise trois tours hiérarchiques fixes, une archive de huit surfaces au
plus par `ProofKey`, la promotion des sous-termes certifiés et un fallback exact
du résidu sans récursion vers P11a.

Micro-gate mesuré :

| Ligne | Entrée | Résultat P11a-2 | Cible diagnostique |
|---:|---:|---:|---:|
| 13 | 26 | 15 | 24 |
| 53 | 110 | 41 | 27 |
| 486 | 91 | 36 | 23 |

L'ablation montrait que le gain venait du troisième tour. L'archive Pareto ne
gagnait encore aucun cas face à `best-only`.

## Benchmark des douze cas `Mul`

Lignes : `13, 53, 77, 125, 134, 210, 234, 260, 294, 369, 481, 486`.

Résultats avant la dernière correction de factorisation :

```text
mul_cases=12
resolved_at_or_below_target_cost=5
cost_reduced=12
reduction_ge_25_percent=8
reduction_ge_50_percent=5
pareto_wins_over_best_only=0
surfaces_generated=709
secondary_proofs=27
budgets_reached=2
pareto_time_ms=median:2912.173,p95:95518.750,max:95518.750
```

P11a-2 généralise donc fonctionnellement, mais n'est pas assez rapide pour une
activation large. Les pires cas mesurés sont 481 (~95,5 s) et 53 (~52 s) en
release pour la variante Pareto.

## Diagnostic des sous-termes cibles

L'exemple `p11a_target_diagnostics` utilise `expected` uniquement après et à
côté de la génération autonome.

- Ligne 53 : premier sous-terme cible absent signalé `~v0`; `v1|v2` existe,
  mais la composition parent nécessaire n'est pas reconstruite jusqu'à la cible.
- Ligne 486 : premier sous-terme cible absent `v0|v1`; les enfants existent,
  mais la forme n'était pas disponible comme facteur composable.
- Dans les deux cas, le diagnostic confirme une lacune de composition/génération,
  pas une panne du certificat.

## Dernière correction appliquée juste avant la pause

Une correction générique a été ajoutée dans P11a :

1. normaliser un facteur additif uniformément négatif, par exemple
   `-v1 - v2 -> -(v1 + v2)` ;
2. autoriser deux factorisations locales enchaînées dans le même tour, avec la
   borne `max_components` existante.

Dernière mesure effectuée sur la ligne 486 :

```text
input_nodes=91
result_nodes=19
expected_nodes=23
proof_key_preserved=true
residual_proofs_attempted=0
budget_exceeded=false
result=(-v0 + (v0 & v2 - 1)) ^ (v1 + v2) * (-(v2 & (v1 | v0)))
```

La ligne 486 est donc désormais résolue à un coût meilleur que la cible
(`36 -> 19`, cible 23), sans oracle et sans preuve secondaire.

## Point exact de reprise

Le snapshot intervient immédiatement après ce succès sur 486.

- La ligne 53 n'a pas encore été remesurée après la correction.
- Le benchmark des douze n'a pas encore été régénéré après la correction.
- La seconde étape de factorisation a été compilée par le run release de 486,
  mais la suite complète de tests n'a volontairement pas été relancée avant ce
  snapshot brut.
- Le dernier test antérieur à cette seconde étape était
  `cargo test -p rumba-core --lib p11a::tests` : 7/7 réussis.
- P11b sur `25, 114, 139, 423` reste à faire.

Ordre conseillé à la reprise :

```text
1. tester la ligne 53 avec la correction actuelle
2. exécuter les tests P11a
3. régénérer le benchmark des 12 Mul
4. décider d'une garde de temps/candidats pour 53 et 481
5. implémenter P11b pour 25, 114, 139, 423
6. benchmark final des 16 sans oracle de génération
7. mettre à jour p11a_results.md et results.md
```

Commandes utiles :

```console
cargo run --release -q -p rumba-core --features parse --example p11a_micro_gate -- 53
cargo run --release -q -p rumba-core --features parse --example p11a_mul_corpus
cargo run --release -q -p rumba-core --features parse --example p11a_target_diagnostics
cargo test -p rumba-core --lib p11a::tests
```

## Fichiers de résultats importants

- `results.md` : état du corpus de production.
- `p9_lite_results.md` : P9-L.
- `p9_poly_results.txt` et `p9_p_results.txt` : expériences polynomiales.
- `p10_results.md` : P10a/P10b/P10c.
- `p11a_results.md` : P11a-2 avant la dernière correction 486.

Les fichiers expérimentaux et rapports non suivis visibles au moment de la
pause font intentionnellement partie de ce snapshot.

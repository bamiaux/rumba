# P11a-2 — compression hiérarchique exacte

## Expressions recherchées

`expected` reste un oracle de diagnostic : ces formes ne sont jamais injectées
dans la génération.

| Ligne | Expression cible du corpus | Coût |
|---:|---|---:|
| 13 | `(~(v0 + v1*v1) * (v1|v1)) & ((v1*v1) ^ (v1+v1))` | 24 |
| 53 | `v2 | ((v1 + (v2|v1))*v1) | ((~v0) & (~(v1&v1)))` | 27 |
| 486 | `(-(v0 & (-v2))) ^ (-(v2 & (v0|v1)) * (v1+v2))` | 23 |

## Architecture

P11a reste expérimental et n'est pas dans le pipeline de production
`RUMBA -> P7e -> P9-L`. P11b et la logique de retenue ne sont pas modifiés.

P11a-2 ajoute à la compression existante :

```text
compression exacte d'un sous-terme
-> archivage de 8 surfaces au plus par ProofKey
-> conservation Pareto (coût, racine, facteur commun, support bitwise)
-> promotion des surfaces comme atomes du parent
-> trois tours bottom-up fixes
```

Les remplacements hiérarchiques sont exacts par congruence. Les nouveaux
candidats locaux suivent toujours la garde :

```text
candidat strictement moins cher
-> empreinte observationnelle identique
-> même ProofKey : accepter
-> sinon prove_zero_without_p11(source - candidat)
```

Le prouveur figé ne rappelle ni P11a ni le simplificateur public : RUMBA/PCT,
P7e, puis P9 direct. Le fallback est borné à 16 preuves par source.

## Micro-gate

Commande :

```console
cargo run --release -p rumba-core --features parse --example p11a_micro_gate
```

| Ligne | Entrée P7e | P11a précédent (2 tours) | P11a-2 (3 tours) | Cible diagnostique | Gate |
|---:|---:|---:|---:|---:|---:|
| 13 | 26 | 15 | **15** | 24 | aucune régression |
| 53 | 110 | 82 | **41** | 27 | **41 < 82** |
| 486 | 91 | 39 | **36** | 23 | **36 < 39** |

Les trois résultats concordent avec `expected` sur les observations et restent
exactement prouvés. La ligne 13 produit même une surface plus petite que le
rendu du corpus :

```text
(2*v1 ^ v1*v1) & v1 * ~(v0 + v1*v1)
```

## Diagnostics hiérarchiques

| Ligne | Sous-termes compacts | Promus | Réutilisés par parent | Facteurs communs | Candidats bitwise parent | Prunés même clé | Prunés budget | Coût minimal atteint |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 13 | 15 | 15 | 0 | 0 | 9 | 10 | 0 | 15 |
| 53 | 351 | 137 | 48 | 15 | 95 | 294 | 12 | 41 |
| 486 | 42 | 33 | 0 | 0 | 12 | 38 | 0 | 36 |

Sur la ligne 53, 38 fusions utilisent la relation exacte plutôt qu'une égalité
directe de ProofKey ; 5 preuves de résidu sont tentées, dont 4 réussissent. La
ligne 486 atteint 36 sans preuve de résidu supplémentaire.

## Ablation et verdict

| Variante | Ligne 53 | Ligne 486 |
|---|---:|---:|
| 2 tours, une meilleure surface | 82 | 39 |
| 3 tours, une meilleure surface | **41** | **36** |
| 3 tours, archive Pareto | **41** | **36** |

Le résultat causal est net : le troisième tour hiérarchique débloque les deux
gates. L'archive Pareto est effectivement alimentée et réutilisée, mais ne
réduit pas encore davantage ces trois lignes. Elle évite cependant de jeter
les surfaces arithmétiques, factorielles et bitwise utiles aux parents futurs.

Les cibles de 53 et 486 ne sont toujours pas générées, et leurs coûts 27/23 ne
sont pas atteints. Le certificat n'est donc plus le verrou ; la compression de
surface reste incomplète. `ProofKey` demeure figée et sûre, sans prétention de
complétude.

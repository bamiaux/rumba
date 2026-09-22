# Profilage et suppression d'allocations depuis `prettify`

> Ce rapport décrit le premier palier, avec deux optimisations. La
> [deuxième passe](continuation/README.md) ajoute la suppression d'une copie
> d'AST. La [campagne QSynth](../qsynth/README.md) ajoute ensuite deux
> optimisations bit-parallèles présentes dans les sources de travail actuelles.

**Deux optimisations retenues : environ 14–15 % de temps en moins, 25,2 %
d'allocations en moins et seulement +6 lignes nettes de production.**

Campagne du 21 septembre 2026. Base réelle :
`e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9` (`prettify`), conformément à la
correction demandée. Modifications laissées sans commit.

## Correction de la première campagne

Les exécutables `base-probe` et `a2-probe` de la campagne précédente avaient le
même SHA-256 : le répertoire Cargo `target` partagé entre checkouts a contaminé
le témoin. Les anciennes comparaisons de vitesse et leurs verdicts DROP ne
prouvaient donc pas l'absence de gain. Cette campagne les remplace. Les
contre-exemples structurels de l'ancienne campagne restent valables ; notamment,
la construction directe de `Scale(Add)` reste exclue.

Chaque variante est maintenant compilée dans son propre répertoire de build.
Le témoin contient bien `Expr::visit` dans ses symboles, le candidat la nouvelle
collecte. Les snapshots sont comparés au témoin original, indépendamment des
exécutables employés pour chronométrer.

## Protocole

- AMD Ryzen 9 3900X, machine virtualisée, affinité CPU 2, Rust 1.95.0.
- `cargo build --release -p rumba-core --example phase3_probe --features parse`.
- Même corpus de 41 000 expressions, mots de 64 bits, pas de JIT pour les mesures.
- Parsing et clonage des entrées hors chronomètre ; simplification et destruction
  des résultats incluses. Un warm-up complet puis cinq runs, médiane.
- Mesures séquentielles, sans compilation ni autre benchmark en parallèle.
- `perf` activé par FIFO **après parsing et warm-up**, désactivé après une passe
  complète. Les compteurs matériels portent uniquement sur cette passe.
- Allocations et appels comptés dans une compilation instrumentée séparée.
  Ses durées ne servent pas aux décisions de performance.
- Les temps sont locaux à cette machine ; pas de comparaison avec la base
  `eb281e2…` indisponible ni avec son chiffre historique de 1,594 s.

## Résultats

Chaque case temporelle est une médiane de cinq runs après warm-up. Les séries
finales sont exécutées dans l'ordre témoin/candidat/candidat/témoin :

| Série finale | `prettify` | Deux optimisations | Delta |
|---|---:|---:|---:|
| 1 | 2,249605 s | 1,916207 s | −14,82 % |
| 2, ordre inversé | 2,297243 s | 1,959574 s | −14,70 % |
| Médiane des dix mesures regroupées | 2,255360 s | 1,943767 s | −13,82 % |

La dispersion interdit de promettre le même pourcentage sur toute machine.
Les données brutes, warm-ups, temps CPU et empreintes des exécutables sont dans
[measurements.json](measurements.json). `vars` désigne la collecte seule, `d1`
les petites sommes, `combo` collecte + réutilisation des allocations,
`vars-d1` les deux optimisations retenues, `best` leur essai avec réutilisation,
et `final` les sources finales formatées. Les noms `base` de **ce** fichier
correspondent tous au véritable témoin recompilé isolément.

| Expérience isolée | LOC nettes du prototype | Éligibilité | Temps 41k | Gain observé | Parité AST + texte | Verdict |
|---|---:|---|---:|---|---|---|
| Collecte dans un ensemble unique | −14 | 396 544 appels | 2,045–2,103 s | 9,6–10,7 % | 41 000/41 000 | KEEP |
| Petites sommes distinctes | +20 | 1 208 364 appels | 2,122–2,140 s | 5,3–6,2 % | 41 000/41 000 | KEEP |
| Réutilisation de `Box` et de `Vec` dans `map`/`try_map` | +15 | transformations AST | 2,219–2,239 s | environ 1 % au contrôle rapproché | 41 000/41 000 | DROP |
| Évaluation sur quatre voies, t=2 | +28 | tables à deux variables | 2,254 s | environ 1 %, exploratoire | 41 000/41 000 | DROP |
| Évaluation sur quatre voies, 2≤t≤10 | +37 | blocs de quatre affectations | 2,216–2,220 s | 1,8–2,0 %, sous le seuil | 41 000/41 000 | DROP |
| A2, entrée directe de `solve_linear`, recontrôle | +4 | 18 654 racines éligibles | 2,276 s | 3,3 %, exploratoire, seuil A=5 % | 41 000/41 000 | DROP |

Les comparaisons utilisent les témoins rapprochés de chaque série, pas un unique
témoin ancien. La collecte puis les petites sommes passent de 2,044942 à
1,960473 s sur la série d'intégration, soit 4,13 % supplémentaires pour D1.

La réutilisation des allocations, ajoutée à ces deux changements, donne
1,958220 s contre 1,960473 s : **0,11 %**, insuffisant pour garder ce code.
Le prototype SIMD exécute bien des instructions SSE2 (`paddq`, `pand`, `por`,
`pxor`) mais le gain global ne justifie pas son ajout. Les patches expérimentaux
[réutilisation](reuse-allocations.patch) et [quatre voies](four-lanes.patch)
sont conservés pour inspection, hors production ; ils s'appliquent isolément
à `prettify`.

## Profil matériel et allocations

Médianes de trois passes complètes, `perf stat` activé seulement pendant la
simplification. [Sorties brutes](perf-stat.txt). Le champ `seconds time elapsed`
de perf inclut aussi le chargement et le warm-up ; il n'est pas utilisé.

| Compteur | `prettify` | Final | Delta |
|---|---:|---:|---:|
| Cycles | 9 568 396 672 | 8 122 256 108 | −15,11 % |
| Instructions | 20 072 782 912 | 16 430 495 852 | −18,15 % |
| Branchements | 3 405 833 916 | 2 805 779 845 | −17,62 % |
| Mauvaises prédictions | 68 958 843 | 64 331 754 | −6,71 % |
| Allocations | 39 561 465 | 29 583 855 | −25,22 % |
| Réallocations | 863 281 | 863 280 | quasi identique |
| Octets demandés cumulés | 2 472 672 290 | 1 777 063 642 | −28,13 % |

Les trois dernières lignes proviennent du compteur d'allocations séparé,
**pas de perf**. Les octets cumulés ne mesurent pas le pic de mémoire résidente.
Le gain correspond à 9 977 610 allocations et environ 696 Mo de demandes cumulées
supprimés. Dans `get_vars` seul : 6 956 638 → 419 098 allocations et
363 202 032 → 22 664 512 octets.

Échantillonnage `cycles:u` à 997 Hz, sans parsing ni warm-up, temps **propre** :

| Point chaud | `prettify` | Final |
|---|---:|---:|
| `reduce_masked` | 8,77 % | 11,29 % |
| `eval_bits` | 7,05 % | 9,03 % |
| destruction d'`Expr` | 4,53 % | 5,82 % |
| `group_terms` | 3,28 % | 1,86 % |
| `malloc` | 4,70 % | 5,47 % |
| `_int_malloc` | 3,99 % | 4,03 % |
| `free` | 4,25 % | 3,23 % |

Les pourcentages sont relatifs à un total réduit : leur hausse n'indique pas
à elle seule une régression. L'échantillonnage, d'environ 2 000 points par
version, localise les zones chaudes ; les trois passes de compteurs matériels
et les dix mesures de latence quantifient le gain. [Profil complet](perf-flat.txt).

Les compteurs de toutes les étapes restent **identiques** entre référence et
version finale : 18 041 509 visites de réduction, 176 250 polynomializations,
178 623 `solve_linear`, 408 067 tables de vérité, 105 976 `make_conjunction_sum`,
2 614 565 `make_bitwise`, 148 155 `is_linear_bitwise`, 1 321 924 `group_terms`.
La collecte visite les mêmes 4 467 939 nœuds. Aucun passage hide/poly/fixed-point
n'est supprimé ; ni l'ordre des candidats ni le modèle de coût ne changent.
Les anciens patterns sont absents de `prettify` ; ses 41 000 finitions et
26 218 appels projector-defect sont inchangés. Appels, durées instrumentées et
histogrammes q/t complets : [allocations-and-calls.txt](allocations-and-calls.txt).

## Validation

- Diff exact des AST et du rendu : **41 000/41 000 identiques** au témoin original
  et au témoin recompilé.
- Qualité, sur les deux versions : **41 000 OK / 0 OKZ / 0 NG**, avec 200
  évaluations aléatoires par cas.
- Comparaison de bibliothèques entièrement séparées : **33 280 réductions**
  sur les largeurs 0–64, plus **896 simplifications** sur sept largeurs 1–64,
  zéro différence d'AST ou de rendu.
- Tests ciblés : variables clairsemées et `usize::MAX`, tous les opérateurs,
  normalisation des coefficients, masque nul, largeur un bit, annulation de
  termes de même base.
- `cargo test --workspace --release --all-features`, `cargo fmt --all --check`
  et `git diff --check` : PASS. [Journal de validation](validation.txt).

## Travail supprimé et invariants

### Collecte des variables

`get_vars` remplit un seul ensemble au cours du parcours. L'ancien visiteur
allouait des vecteurs de résultats intermédiaires et construisait/fusionnait
un ensemble par sous-arbre. Le visiteur générique, sans autre utilisateur,
est supprimé. Les identifiants restent arbitraires, y compris les indices
clairsemés ; aucune renumérotation n'est ajoutée.

Les consommateurs du solveur prennent un maximum, testent l'appartenance ou
trient les identifiants avant de choisir des candidats. Ils ne dépendent pas
de l'ordre d'itération de l'ensemble.

### Petites sommes déjà distinctes

Les termes arrivent déjà réduits dans `group_terms`. Pour au plus quatre termes,
si leurs bases hors coefficient sont distinctes, le regroupement ne peut rien
fusionner. On trie directement le vecteur existant, avec le même comparateur.
On évite la table de hachage, le vecteur de sortie et les reconstructions de
`Scale`. Le masque nul conserve le chemin général : ses coefficients doivent
être annulés. Les cas avec doublons conservent le regroupement original.

## Reproduction

Depuis le dépôt, exécuter `bash docs/performance/phase3-followup/reproduce.sh`.
Le script compile `prettify` et les sources de travail actuelles dans deux
répertoires distincts, compare les snapshots du corpus et des arbres générés,
puis mesure dans l'ordre témoin/candidat/candidat/témoin. Les fichiers sont
conservés dans le répertoire temporaire affiché.

La sonde `probe.rs` est copiée dans `core/examples/phase3_probe.rs` pour compiler ;
son module `support/corpus.rs` est celui du dépôt. La sonde `synthetic.rs` génère
33 280 réductions sur toutes les largeurs 0–64 et 896 simplifications sur sept
largeurs 1–64. Les deux bibliothèques complètes sont compilées séparément : le
témoin ne partage donc pas les nouveaux helpers `Expr` avec le candidat.

Pour reproduire les compteurs et les profils, dans l'environnement contenant
`perf` (version utilisée : 7.2.4), pointer `probe` vers l'un des exécutables isolés :

```sh
profile_dir=$(mktemp -d /tmp/rumba-perf.XXXXXX)
mkfifo "$profile_dir/control" "$profile_dir/ack"
perf stat -o "$profile_dir/stat.txt" -D -1 \
  --control "fifo:$profile_dir/control,$profile_dir/ack" \
  -e cycles:u,instructions:u,branches:u,branch-misses:u -- \
  taskset -c 2 "$probe" profile "$profile_dir/control" "$profile_dir/ack"
perf record -o "$profile_dir/perf.data" -D -1 \
  --control "fifo:$profile_dir/control,$profile_dir/ack" \
  -F 997 -e cycles:u --call-graph dwarf -- \
  taskset -c 2 "$probe" profile "$profile_dir/control" "$profile_dir/ack"
perf report --stdio --no-children --call-graph none --percent-limit 0.8 \
  -i "$profile_dir/perf.data"
```

Pour les allocations, utiliser une **nouvelle copie jetable** des sources de
chaque variante, et un autre répertoire Cargo. Copier la sonde originale
`docs/performance/phase3/probe.rs` dans `core/examples/phase3_probe.rs` de cette
copie, puis appliquer `docs/performance/phase3/instrument.py CHECKOUT` et
`docs/performance/phase3-followup/variable_profile.py CHECKOUT` avec Python.
Compiler comme ci-dessus et exécuter `phase3_probe once`. Cette instrumentation
n'entre jamais dans la bibliothèque livrée ni dans les exécutables chronométrés.

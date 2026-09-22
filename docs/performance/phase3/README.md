# RUMBA v1.0.1 — phase 3 depuis `prettify`

> **Correction du protocole, campagne suivante :** l'audit des exécutables a
> établi que `base-probe` et `a2-probe` utilisés dans les répétitions avaient
> le même SHA-256 (`d68634c25aea0e1523f79b2035a5a880bb09338305bd974498206716d956babe`).
> Le partage du répertoire Cargo `target` entre checkouts a contaminé le témoin.
> Les deltas et verdicts de performance ci-dessous ne permettent donc pas de
> conclure à l'absence de gain contre `prettify`. Ils sont conservés comme trace
> de l'expérience, et remplacés par une campagne utilisant un répertoire de
> compilation distinct pour chaque variante. Les snapshots d'origine et les
> contre-exemples structurels restent des preuves séparées des chronométrages.
> Voir la [campagne corrigée et les optimisations retenues](../phase3-followup/README.md).

## Rapport historique — conclusions de performance invalidées

**Aucun patch de production retenu.** Les expérimentations A, B, C, puis D
n'ont pas établi un gain respectant les seuils demandés et la parité structurelle.
La branche finale `perf/rumba-1.0.1-phase3` contient ce dossier de preuves ;
le code de production reste celui de
`e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9` (`prettify`).

Le remplacement de la base initialement demandée par `prettify` a été autorisé
explicitement par l'utilisateur. Le commit `eb281e2…` étant indisponible,
aucune comparaison de performance avec ce commit n'est revendiquée.

## Référence et protocole

- Machine : AMD Ryzen 9 3900X, Linux virtualisé, affinité CPU 2.
- Compilateur : Rust 1.95.0, LLVM 21.1.8.
- Commande de référence :
  `cargo build --release -p rumba-core --example corpus --features parse`,
  identique à la recette `_corpus-report-runner` de `prettify`.
- Sept CSV du dépôt, 41 000 expressions, mots de 64 bits. Aucune dépendance ajoutée.
- Runner officiel : passe qualité de warm-up, puis cinq mesures par dataset.
  Référence initiale : **2,496617 s ; 41 000 OK / 0 OKZ / 0 NG**.
- Sonde comparative : parsing et clonage des entrées hors chronomètre ;
  un warm-up complet, puis cinq parcours complets, médiane. Simplification et
  destruction des résultats incluses. Chaque processus utilise les caches locaux
  habituels, sans cache partagé entre expressions.
- Parité : comparaison des 41 000 lignes contenant identifiant, `Debug` de l'AST
  retourné et texte de `repr(64, false, false)`. Les SHA-256 sont conservés dans
  [measurements.json](measurements.json).
- CPU de la cohorte : temps d'exécution du thread via `/proc/self/schedstat` ;
  comptabilité du noyau, avec une granularité observable de quelques millisecondes.

Le runner officiel final donne **2,366983 s avec exactement le même code**,
soit une dérive de −5,19 %. Il ne faut pas comparer un candidat tardif uniquement
aux 2,497 s initiales. Les décisions utilisent les témoins rapprochés et, pour les
signaux prometteurs, les répétitions alternées. Les mesures préliminaires de A
restent exploratoires ; la série initiale chevauchant l'instrumentation a été
écartée et le témoin remesuré avant les comparaisons retenues.

## Résultats des expériences isolées

Temps en secondes de la sonde sur les 41k. Delta = temps candidat / temps témoin − 1 ;
un delta négatif indique une accélération. LOC = différence nette de production
du prototype, hors sonde, instrumentation et tests. Les patches archivés sont
**indépendants, tous applicables à `prettify` ; ne pas les empiler**.

| Expérience | LOC | Éligibles | Temps 41k | Delta | Parité AST + texte | Verdict |
|---|---:|---:|---:|---:|---:|---|
| A0 — solve racine unique | +4 | 18 654 racines | 2,167 | non qualifié | 35 256/41 000 | DROP |
| A1 — solve racine direct, fixed-point conservé | +7 | 18 654 racines | 2,390 | −1,93 % exploratoire | 41 000/41 000 | DROP |
| A2 — entrée directe de `solve`, récursion comprise | +4 | 18 654 racines | 2,282 | +1,68 % au dernier contrôle | 41 000/41 000 | DROP |
| B — transfert de l'arbre dans la vérification de signature | 0 | 148 155 appels | 2,316 | +2,42 % | 41 000/41 000 | DROP |
| C1 — cube `u64`, q ≤ 6 | +44 | 89 901 conversions | 2,314 | +2,84 % | 41 000/41 000 | DROP |
| C1b — cube `u64`, 3 ≤ q ≤ 6 | +44 | 23 331 conversions | 2,206 | −0,68 % | 41 000/41 000 | DROP |
| C2 — C1b + Möbius dense sous le même seuil | +58 | 23 331 conversions packées | 2,201 | −0,70 % | 41 000/41 000 | DROP |
| D1 — bases distinctes, arité ≤ 4 | +20 | 1 208 364 appels | 2,196 | −3,66 % initialement | 41 000/41 000 | DROP après répétitions |
| D1b — bases distinctes, arité = 2 | +20 | 866 155 appels | 2,146 | −1,21 % | 41 000/41 000 | DROP |
| D2 — construction directe de `Scale(Add)` | +12 | 124 808 distributions | 2,300 | +1,05 % | 41 000/41 000 ; échec hors corpus | DROP |
| D4 — éviter la réduction répétée de la première passe | +8 | 41 000 racines | 2,168 | −1,87 % | 41 000/41 000 | DROP |

A0 donne **33 850 OK / 7 150 OKZ / 0 NG** : il change aussi certaines sorties de
ground truth, d'où un nombre d'OKZ supérieur aux 5 744 différences des entrées.
Toutes les autres variantes donnent **41 000 OK / 0 OKZ / 0 NG**, vérifiés par
la même classification que le runner officiel et 200 évaluations aléatoires par cas.

Les répétitions D1, chacune avec warm-up et cinq mesures, n'atteignent pas
les 3–5 % reproductibles recherchés pour un petit patch :

| Série | Témoin | D1 | Delta |
|---|---:|---:|---:|
| 1 | 2,217821 | 2,155378 | −2,82 % |
| 2, ordre inversé | 2,231030 | 2,194336 | −1,64 % |
| 3 | 2,241201 | 2,190766 | −2,25 % |

Les deux derniers contrôles A2 passent de −1,71 % à +1,68 % : le seuil A de
5 % n'est pas reproduit. Les témoins exacts, cinq temps individuels, warm-ups,
temps CPU disponibles et médianes figurent dans [runs.txt](runs.txt).

## Profil du meilleur état

`perf` et les profileurs usuels n'étaient pas installés. Le profil ci-dessous
provient d'une **copie instrumentée séparée**, sur les 41k complets : compteurs
de visites, chronométrage imbriqué et comptage des allocations système.
Ce n'est pas un échantillonnage matériel. Les durées instrumentées incluent son
surcoût et ne servent jamais aux décisions de gain. Les durées inclusives
récursives ne s'additionnent pas ; les durées exclusives excluent les autres
portées instrumentées, pas toutes les fonctions auxiliaires.

Le meilleur état étant inchangé, les nombres d'appels du nouveau profil sont
exactement ceux de la référence. Son temps total instrumenté est **4 081,1 ms**.

| Zone | Appels référence | Appels meilleur état | Exclusif final, ms |
|---|---:|---:|---:|
| total | 41 000 | 41 000 | 262,2 |
| reduce | 18 041 509 | 18 041 509 | 1 593,0 |
| make_polynomial | 176 250 | 176 250 | 41,6 |
| solve_linear | 178 623 | 178 623 | 317,8 |
| truth_table_masked | 408 067 | 408 067 | 275,8 |
| make_conjunction_sum | 105 976 | 105 976 | 28,9 |
| make_bitwise | 2 614 565 | 2 614 565 | 242,0 |
| is_linear_bitwise | 148 155 | 148 155 | 148,8 |
| group_terms | 1 321 924 | 1 321 924 | 292,1 |
| reduce_scale | 2 884 664 | 2 884 664 | 216,3 |
| hide_in_var | 94 512 | 94 512 | 200,7 |
| merge_hidden | 176 222 | 176 222 | 155,1 |
| prettify | 41 000 | 41 000 | 64,6 |
| projector_defect | 26 218 | 26 218 | 170,8 |
| anciens patterns | absents | absents | sans objet |

`prettify` a supprimé l'ancien moteur de patterns ; ses finitions et la fermeture
projector-defect sont donc rapportées séparément. Le profil compte aussi
**5 314 248 visites de `reduce_vars`**, **49 524 435 visites d'évaluation** et
**1 278 721 nœuds dans les entrées de `is_linear_bitwise`**.
Allocations : **39 561 465**, plus **863 281 reallocations**, pour
**2 472 672 290 octets demandés cumulés** ; ce n'est pas une mesure de mémoire vive maximale.

Le paramètre `t` du solveur est le nombre de variables distinctes après renumérotation,
appelé `q` pour le cube dans la mission. Distribution des 178 623 appels :
t=0 : 1 474 ; t=1 : 52 988 ; t=2 : 83 694 ; t=3 : 25 129 ; t=4 : 11 948 ;
t=5 : 2 669 ; t=6 : 561 ; t>6 : 160. Maximum observé : 14.
Les histogrammes de taille AST et de nombre de termes sont également archivés.

Dans [profiles.txt](profiles.txt), `hist0` = t, `hist1` = taille AST du problème
renuméroté, `hist2` = arité de `group_terms`, `hist3` = entrée déjà triée (0/1),
`hist4` = nombre de termes du problème, `hist5` = indice de passe,
`hist6` = pass2 == pass1 (0/1), `hist7` = arités conduisant à une nouvelle réduction
après regroupement. Les tailles ≥64 sont regroupées dans la dernière case.
Pour C, `hist8/9` donnent q des conversions réussies/tentées. Lorsque présent,
le dernier histogramme ajouté par l'instrumentation de `reduce_scale` donne
l'arité des sommes distribuées. Les témoins avec et sans cette portée supplémentaire
sont tous deux archivés ; les compteurs restent comparables, les temps exclusifs
dépendent des portées instrumentées.

## Travail supprimé par les prototypes et raisons des rejets

**A.** La cohorte strictement linéaire contient 18 654 expressions, soit 45,50 %.
A0 n'en conserve exactement que 12 910 : la deuxième passe réordonne les termes,
par exemple `v1 + v0` devient `v0 + v1`. Aucun choix du « meilleur des deux »
n'a été ajouté. A1 conserve le fixed-point ; A2 place le même raccourci à l'entrée
de `solve` et l'applique aussi aux sous-problèmes récursifs.

A2 active 130 715 raccourcis : autant d'appels à `make_polynomial` et à
`merge_equal_hidden_components` disparaissent. `make_bitwise` passe de
2 614 565 à 1 449 138 visites, mais la validation `is_bitwise` passe de
40 469 à 1 470 717 visites. Aucun appel à `hide_in_var`, aucune restauration
cachée et aucune passe de fixed-point ne disparaît. La seconde polynomialisation
conditionnelle n'est pas éliminée non plus. La forme des clés de cache change,
avec 13 résolutions effectives supplémentaires. Sur la cohorte, A2 conserve
18 654/18 654 sorties et passe de **0,499923 s à 0,463861 s de CPU médian**
(−7,21 %), insuffisant pour le seuil global.

**B.** Ici, `is_linear_bitwise` vérifie une signature arithmétique et peut effectuer
une substitution ; ce n'est pas un prédicat syntaxique suivi d'une construction
identique. `make_bitwise` valide et construit déjà les sous-arbres purement
bitwise en un seul parcours. Le test local déplace l'arbre dans `reduce_vars`
puis restaure ses identifiants, au lieu de le cloner. Il économise 319 492
allocations, mais remplace la visite de clonage par une visite de restauration :
aucune fusion supplémentaire de ces deux opérations n'est obtenue.

**C.** Le cube utilise six lanes constantes, sans table générique ni multiword.
Le bit d'affectation zéro détermine les bits de poids fort constants d'un
sous-arbre strictement bitwise ; les coefficients arithmétiques sont ensuite
appliqués modulo la largeur du mot. Les formes non reconnues continuent dans
l'évaluateur scalaire existant. C1 réduit les visites d'évaluation de
49 524 435 à 31 744 725, mais ajoute validation, construction du cube et
reconstruction des valeurs. Le coût sur les petits cubes annule le gain.
C1b/C2 n'atteignent pas 2 %. C2 ne remplace jamais globalement la Möbius
incrémentale : sa transformation dense est limitée à 3 ≤ q ≤ 6.

C1 et C2 passent chacun **14 336 tables générées**, sur toutes les largeurs
1–64 bits et q=0–6, avec comparaison des coefficients/conjonctions à la fonction
originale. Des assertions ont aussi comparé chaque conversion packée effectivement
rencontrée pendant un parcours des 41k à la table scalaire et à la reconstruction
originale, sans différence. Voir [audits.txt](audits.txt).

**D1.** 1 232 726 appels à `group_terms` sur 1 321 924 ont une arité ≤4 ;
226 509 entrées sont déjà triées. Le prototype évite la table de hachage et les
reconstructions de coefficients pour les bases distinctes. Il économise
3 440 072 allocations, sans retirer de réduction. Le garde du masque nul
conserve le comportement de `reduce(0)`. **33 280 arbres générés**, sur les
largeurs 0–64, ont un résultat strictement identique au réducteur original.
Le signal chronométrique initial ne se reproduit cependant pas au niveau demandé.

**D2.** Un passage préalable par le réducteur ne garantit pas que chaque terme
soit déjà entièrement normalisé. Le test différentiel trouve dès n=3 un cas
où la construction directe conserve un `Mul` imbriqué que la réduction
générique aplatit. Le log conserve l'entrée et les deux AST exacts. Le passage
des 41k et de la qualité ne suffit donc pas à retenir ce patch.

**D3.** 40 867 expressions exécutent pass2 ; 18 799 restent identiques et
**22 068 changent structurellement**. Dix-sept atteignent pass3. Les observations
historiques « seulement 25 actives » ne s'appliquent pas à `prettify`.
Aucun prédicat trivial permettant de supprimer pass2 n'a été établi ;
aucun patch supprimant globalement cette passe n'a été testé ou retenu.

**D4.** Ce dernier contrôle sépare l'entrée déjà réduite du solveur et saute
uniquement la réduction répétée de la première passe. Il retire 2 303 237 visites
du réducteur et 1 929 573 allocations. La parité passe aussi sur 896 problèmes
générés comparés au solveur original, mais le gain global mesuré reste de 1,87 %.

**KEEP : aucun.** Les patches de production ont tous été retirés avant la création
de la branche finale. Ces résultats n'établissent pas qu'aucun gain futur soit
possible ; ils ne qualifient pas un gain à livrer dans cette session.

## Reproduire les contrôles

Depuis la racine du dépôt, créer des copies jetables, sans modifier le checkout :

```bash
report_dir="$PWD/docs/performance/phase3"
reference_dir=$(mktemp -d /tmp/rumba-reference.XXXXXX)
candidate_dir=$(mktemp -d /tmp/rumba-candidate.XXXXXX)
git archive e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9 | tar -x -C "$reference_dir"
git archive e1f9f531e2c5a73bf1bd746cbb25931fd5d3c4b9 | tar -x -C "$candidate_dir"
cp "$report_dir/probe.rs" "$reference_dir/core/examples/phase3_probe.rs"
cp "$report_dir/probe.rs" "$candidate_dir/core/examples/phase3_probe.rs"
git -C "$candidate_dir" apply "$report_dir/patches/a2.patch"
cargo build --manifest-path "$reference_dir/Cargo.toml" --release -p rumba-core --example phase3_probe --features parse
cargo build --manifest-path "$candidate_dir/Cargo.toml" --release -p rumba-core --example phase3_probe --features parse
taskset -c 2 "$reference_dir/target/release/examples/phase3_probe" snapshot "$reference_dir/outputs"
taskset -c 2 "$candidate_dir/target/release/examples/phase3_probe" snapshot "$candidate_dir/outputs"
cmp "$reference_dir/outputs" "$candidate_dir/outputs"
taskset -c 2 "$reference_dir/target/release/examples/phase3_probe" bench
taskset -c 2 "$candidate_dir/target/release/examples/phase3_probe" bench
taskset -c 2 "$candidate_dir/target/release/examples/phase3_probe" quality
```

Exécuter les mesures successivement, jamais en concurrence avec un profil ou
un autre benchmark. Alterner l'ordre des témoins/candidats. L'option `cohort`
imprime les identifiants éligibles ; `bench unused PATH` restreint une mesure
au fichier d'identifiants. `once` effectue un seul parcours sans chronométrage.

Pour profiler une autre copie fraîche du candidat :
`python3 "$report_dir/instrument.py" "$candidate_dir"`, recompiler puis exécuter
`phase3_probe once`. Ne jamais utiliser ce binaire pour un chiffre de gain.

Les scripts d'audit prennent trois arguments : source candidate, répertoire de
référence, répertoire jetable à instrumenter (initialement une copie de référence).
Par exemple, pour C1/C2 :

```bash
python3 "$report_dir/audit_packed.py" "$candidate_dir/core/src/simplify.rs" "$reference_dir" "$candidate_dir"
cargo test --manifest-path "$candidate_dir/Cargo.toml" --release -p rumba-core --lib --features parse phase3_packed_tests
```

Recompiler également la sonde puis lancer `once` pour les assertions packées sur
le corpus. `audit_reduce.py` injecte le réducteur de référence et
`phase3_differential_tests` ; `audit_solver.py` fait de même pour le solveur.
Chaque audit et chaque profil doit partir d'une nouvelle copie.

Validation finale du livrable : `cargo fmt --all --check`, les sept tests unitaires
de `rumba-core` en release avec `parse`, application à blanc des onze patches sur
une copie fraîche de la référence, puis exécution réussie de l'audit C2 avec les
scripts archivés. Le diff de `core`, `cli`, `bindings`, des manifests, du lockfile
et du justfile contre la référence est vide.

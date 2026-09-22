# QSynth : poids du corpus, profil et deux optimisations ciblées

> Le [profilage suivant de Loki](../loki/README.md) part de cet état et ajoute
> une optimisation du cache local. Les mesures ci-dessous décrivent ce palier QSynth.

**QSynth est le plus cher par expression, mais Loki coûte davantage au total.**
Sur la version de travail à l'entrée de cette campagne, les 500 cas QSynth
représentent environ **22 % du temps pour 1,22 % des expressions**. Leur coût
moyen est 21 fois celui d'un cas Loki.

Les deux changements retenus réduisent ensuite QSynth de **12,7 %** et le corpus
complet de **4,5 %**, avec les mêmes sorties. Aucun commit effectué.

## Comparaison des corpus avant ces changements

Un warm-up puis cinq runs, médiane, CPU 2, parsing et clonage des entrées hors
chronomètre. Les sept cohortes sont mesurées séquentiellement avec le même
exécutable. La part du temps est calculée sur la **somme des médianes des cohortes**
(1,898079 s), et non sur une médiane globale mesurée séparément.

| Corpus | Expressions | Temps | Part du temps | Moyenne par expression |
|---|---:|---:|---:|---:|
| Loki | 25 000 | 971,4 ms | 51,18 % | 38,9 µs |
| **QSynth EA** | **500** | **416,0 ms** | **21,91 %** | **831,9 µs** |
| NeuReduce | 10 000 | 218,9 ms | 11,53 % | 21,9 µs |
| MBA Flatten | 3 000 | 150,2 ms | 7,91 % | 50,1 µs |
| MBA Obf Linear | 1 000 | 82,5 ms | 4,35 % | 82,5 µs |
| MBA Obf Nonlinear | 1 000 | 49,3 ms | 2,60 % | 49,3 µs |
| Syntia | 500 | 9,8 ms | 0,52 % | 19,6 µs |

[Données et runs par corpus](datasets.json). Les fichiers de quelques corpus
n'ont pas de saut de ligne final : `wc -l` sous-estime alors d'une expression.
Les effectifs ci-dessus sont ceux du parseur du corpus.

## Où QSynth dépense son temps

Le profil porte **uniquement sur QSynth**, avec `perf` activé par FIFO après le
parsing et le warm-up. Dix passes sont échantillonnées à 997 Hz ; le clonage
entre passes se fait avec les compteurs désactivés. Environ 4 000 échantillons
avant et 3 000 après ; zéro échantillon perdu.

| Fonction, temps propre | Avant | Après |
|---|---:|---:|
| Évaluation scalaire `eval_bits` | 14,53 % | 5,71 % |
| Évaluation à quatre voies `eval_four` | absente | 3,43 % |
| `reduce_masked` | 14,20 % | 15,61 % |
| `reduce_and` | 5,90 % | 6,72 % |
| Inférence des tables bitwise | 5,71 % | 0,77 % |
| `malloc` | 4,00 % | 4,45 % |
| Destruction d'`Expr` | 3,62 % | 4,07 % |

Les proportions sont relatives à un total réduit. La hausse de la part d'une
fonction ne signifie pas à elle seule qu'elle ralentit. Profils
[avant](profile-before.txt), [après](profile-after.txt) et
[inclusif avant](profile-inclusive-before.txt). Les temps inclusifs des parcours
récursifs se recouvrent et ne s'additionnent pas.

Le coût est concentré : les **10 cas les plus lents représentent 29,2 %** du
temps QSynth, les 50 premiers **61,1 %**, sur les médianes par expression.

| Cas | Nœuds dans l'entrée | Variables dans l'entrée | Avant | Après |
|---|---:|---:|---:|---:|
| `qsynth_ea:34` | 1 581 | 3 | 29,079 ms | 15,101 ms |
| `qsynth_ea:452` | 671 | 3 | 20,241 ms | 15,256 ms |
| `qsynth_ea:490` | 674 | 3 | 17,714 ms | 14,740 ms |
| `qsynth_ea:425` | 2 641 | 2 | 10,017 ms | 8,973 ms |
| `qsynth_ea:186` | 861 | 3 | 8,617 ms | 7,873 ms |

Le cas 34 perd **48 %** de sa latence. Il produit notamment une table à
**14 variables / 16 384 affectations** après les transformations internes.
Ce nombre est distinct des trois variables de son entrée. Classements complets :
[avant](cases-before.csv), [après](cases-after.csv).

## Changements retenus

### Observations bitwise en parallèle dans un mot

L'inférence parcourait chaque bit de chaque observation. Pour chaque affectation
des parents, elle calcule maintenant le masque des positions correspondantes
avec des opérations `u64`. Une intersection avec la sortie ou son complément
indique si un zéro ou un un a été observé. Une position d'entrée ayant les deux
valeurs reste contradictoire ; une position sans observation reste inconnue.

La complétion de la table, les candidats produits, leur ordre et leur preuve
restent inchangés. Les opérations traitent les bits en parallèle, sans nouvelle
algèbre ni approximation. Le vecteur des observations n'est construit qu'après
le rejet des contradictions.

Sur QSynth : **55 026 appels inchangés**, **10 921 183 itérations bit à bit**
remplacées par **223 688 traitements de mots**, et **54 970 allocations évitées**.
Le nombre d'itérations ne prétend pas compter les instructions machine : chaque
traitement de mot examine les affectations possibles des parents.

### Quatre affectations par traversée des grandes tables

Sur les **50 833 tables** calculées pour QSynth, seulement **103** ont au moins
huit variables. Elles représentent pourtant **10 595 840 des 14 358 047 visites
de nœuds** dues aux tables de vérité, soit **73,8 %**.

Pour ces tables interprétées, une traversée évalue quatre affectations avec des
tableaux `[u64; 4]`. Les deux premières variables fournissent les quatre
combinaisons 00, 01, 10, 11 ; les autres ont la même valeur dans les quatre voies.
Les opérations arithmétiques conservent le débordement modulo 2⁶⁴ et le masque
final. L'ordre des entrées de la table et l'extraction des coefficients restent
inchangés. Le compilateur génère notamment `paddq`, `pand`, `por` et `pxor`
SSE2 sur cette machine : [désassemblage](simd-assembly.txt).

Le seuil de huit variables vient de ce profil. Il n'y a aucun test du nom du
corpus dans la bibliothèque. Avec la fonctionnalité `jit`, le basculement
existant au JIT au-delà de dix variables est conservé. Les performances chiffrées
ici utilisent **`--features parse`, sans JIT**, comme la référence.

Les deux changements représentent **+56 lignes nettes de production**,
hors tests. Aucun paquet ajouté, aucun code `unsafe` ajouté.

## Mesures contrôlées et choix des variantes

Les sources de départ contiennent déjà les trois optimisations des
[campagnes précédentes](../phase3-followup/continuation/README.md). Le témoin
`current` est cet état, et **pas `prettify` brut**. Chaque variante dispose de
son propre checkout et de son propre répertoire Cargo. Machine : Ryzen 9 3900X
virtualisé, Rust 1.95.0 ; même compilation release et affinité CPU 2.

| Variante indépendante | Deux médianes QSynth | Verdict |
|---|---:|---|
| Observations dans un mot `u64` | 390,7 / 379,7 ms | KEEP |
| Quatre voies pour les grandes tables | 386,9 / 378,2 ms | KEEP |
| Témoin encadrant ces essais | 406,9 / 409,3 ms | référence |

Les variantes ont chacune conservé les 41 000 sorties du corpus complet.
La combinaison a ensuite été mesurée. Une variante supplémentaire mutualisant
l'évaluateur scalaire et l'évaluateur vectoriel avec un paramètre constant donne
1,822838 s sur les 41k contre 1,779814 s pour les chemins spécialisés, soit
+2,42 % sur cet essai : **DROP**. Son
[patch expérimental](unified-dropped.patch) est archivé hors production.

Validation finale des sources retenues, deux séries de cinq runs chacune après
warm-up, ordre témoin/final/final/témoin :

| Mesure | Avant | Après | Delta |
|---|---:|---:|---:|
| QSynth, série 1 | 422,673 ms | 369,274 ms | −12,63 % |
| QSynth, série 2 | 408,689 ms | 351,143 ms | −14,08 % |
| **QSynth, médiane des dix runs regroupés** | **409,542 ms** | **357,499 ms** | **−12,71 %** |
| 41k, série 1 | 1,841363 s | 1,755539 s | −4,66 % |
| 41k, série 2 | 1,841662 s | 1,760476 s | −4,41 % |
| **41k, médiane des dix runs regroupés** | **1,841513 s** | **1,758008 s** | **−4,53 %** |

La dispersion est visible dans les runs ; les gains ne sont pas une promesse
sur d'autres machines. Les [mesures brutes](measurements.json) comprennent les
warm-ups, les temps CPU et les empreintes des exécutables et des snapshots.

## Compteurs matériels et travail supprimé

Médianes de trois mesures `perf stat`, portant chacune sur **dix passes QSynth**.
Parsing, warm-up et clonage des entrées exclus par les commandes FIFO.

| Compteur, dix passes | Avant | Après | Delta |
|---|---:|---:|---:|
| Cycles | 16 953 447 106 | 14 681 592 465 | −13,40 % |
| Instructions | 38 972 451 940 | 30 996 566 494 | −20,47 % |
| Branchements | 6 499 704 525 | 5 201 572 609 | −19,97 % |
| Mauvaises prédictions | 109 723 789 | 96 433 128 | −12,11 % |

[Sorties brutes](perf-stat.txt). Le temps global affiché par perf inclut aussi
les phases désactivées ; il n'est pas utilisé comme latence du solveur.

Comptage séparé sur **une passe QSynth**, avec instrumentation non chronométrée :

| Compteur | Avant | Après |
|---|---:|---:|
| Visites physiques de nœuds pour l'évaluation | 14 825 519 | 6 878 639 |
| Allocations | 5 298 439 | 5 243 469 |
| Réallocations | 159 523 | 159 523 |
| Octets demandés cumulés | 346 339 988 | 346 199 451 |

Les octets cumulés ne sont pas le pic de mémoire résidente. Les visites de
réduction (**4 976 489**), les tables de vérité (**50 833**), les résolutions
linéaires (**19 583**) et les appels d'inférence (**55 026**) sont inchangés.
[Compteurs, histogrammes et diagnostic du cas 34](counts.txt).

## Validation et reproduction

- **41 000/41 000 AST et rendus identiques** au vrai témoin `prettify`.
- **41 000 OK / 0 OKZ / 0 NG**, avec 200 évaluations aléatoires par expression.
- **33 280 réductions + 896 simplifications générées** : zéro différence entre
  bibliothèques compilées séparément.
- Inférence parallèle comparée à l'ancienne boucle scalaire sur les largeurs
  0–64, les arités 0–2, toutes les fonctions booléennes correspondantes, les
  observations manquantes et des contradictions aléatoires.
- Tables à quatre voies comparées à l'évaluation scalaire : largeurs 0, 1, 7,
  32 et 64, seuils de dispatch, arités vides et débordements arithmétiques.
- `cargo test --workspace --release --all-features`, puis
  `cargo test -p rumba-core --release --lib` sans JIT : PASS.
  Formatage et `git diff --check` : PASS. [Journal](validation.txt).

Exécuter `bash docs/performance/qsynth/reproduce.sh`. Le script crée deux copies
isolées des sources actuelles et inverse uniquement [changes.patch](changes.patch)
dans le témoin. Il vérifie les sorties, mesure QSynth et les 41k, puis conserve
les classements et résultats qualité dans le répertoire temporaire affiché.

La [sonde QSynth](probe.rs) se copie dans `core/examples/qsynth_probe.rs`. Son
mode `profile CONTROL ACK` effectue dix passes en désactivant les compteurs
pendant les clonages. Les commandes `perf --control fifo:...` sont celles du
[protocole précédent](../phase3-followup/README.md#reproduction), avec cet
exécutable. Les compteurs de visites s'obtiennent avec l'instrumentation
séparée de `phase3/instrument.py`, en comptant aussi l'entrée d'`eval_four` ;
les histogrammes des tables accumulent les appels par `t` et `size(AST) × 2ᵗ`.
Les boucles d'inférence sont comptées respectivement à chaque bit avant et
à chaque mot après. Cette instrumentation ne figure pas dans le code livré.

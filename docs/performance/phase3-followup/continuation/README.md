# Deuxième passe : supprimer la copie d'entrée du solveur linéaire

> Ce rapport décrit le palier précédant le
> [profilage ciblé de QSynth](../../qsynth/README.md), qui apporte deux
> optimisations supplémentaires.

**Une optimisation supplémentaire retenue : emprunter l'AST pendant le calcul
de sa signature.** Trois lignes modifiées, aucune ligne nette ajoutée. Elle
supprime 105 976 clonages, portant sur 2 824 707 nœuds, et 1 735 122 allocations
supplémentaires. Les changements restent sans commit.

Le point de départ de cette passe est la version comportant les deux
optimisations du [premier profilage](../README.md), déjà plus rapide que
`prettify`. Les patches de ce dossier s'appliquent à cet état intermédiaire,
**indépendamment les uns des autres**. Seul `borrow.patch` est retenu.

## Pourquoi cette copie était inutile

Sur un échec du cache linéaire, `solve_linear` clonait l'expression pour appeler
`solve_linear_inner`, puis conservait l'original comme clé du cache. Le calcul
interne ne fait que lire l'expression pour produire sa signature. Il accepte
maintenant `&Expr` et lit directement la future clé. La valeur est ensuite
transférée au cache comme avant. Les expressions produites, l'ordre des
candidats, les passes du solveur et le modèle de coût restent inchangés.

Le comptage instrumenté donne exactement les mêmes appels à toutes les étapes
avant/après, y compris 105 976 calculs internes et 178 623 `solve_linear`.
Les arbres lus ont la même taille cumulée ; seule leur copie disparaît.

## Mesures finales

Même protocole que le palier précédent : CPU 2, parsing/clonage des entrées hors
chronomètre, un warm-up puis cinq runs, compilations isolées et aucun autre
benchmark ou build simultané. La deuxième campagne suit l'ordre
`prettify / palier précédent / emprunt / sous-ensembles / emprunt / palier précédent / prettify`.

| Comparaison | Palier précédent | Avec emprunt | Delta |
|---|---:|---:|---:|
| Série 1 | 2,008646 s | 1,920702 s | −4,38 % |
| Série 2, ordre inversé | 1,969728 s | 1,884529 s | −4,33 % |
| Médiane des dix mesures regroupées | 1,993216 s | 1,909585 s | **−4,20 %** |

Sur cette même campagne, les dix mesures de `prettify` ont une médiane de
**2,359338 s**, contre **1,909585 s** pour la version finale : **−19,06 % cumulés**.
Les médianes individuelles de `prettify` sont 2,416231 et 2,338456 s ; cette
variation explique pourquoi on ne multiplie pas les pourcentages issus de
campagnes différentes. Les chiffres restent propres à ce corpus et à cette
machine. [Runs, empreintes des exécutables et des sorties](measurements.json).

La variante de mise à jour des coefficients donne 1,974853 s, contre
1,989187 s pour la moyenne des deux témoins encadrants : **−0,72 %**, exploratoire,
insuffisant pour la retenir.

Trois passes de `perf stat`, activé seulement pendant la simplification,
confirment que le travail exécuté baisse :

| Compteur, médiane | Palier précédent | Avec emprunt | Delta |
|---|---:|---:|---:|
| Cycles | 8 076 251 633 | 7 828 161 354 | −3,07 % |
| Instructions | 16 432 968 643 | 15 870 747 487 | −3,42 % |
| Branchements | 2 806 271 053 | 2 693 951 435 | −4,00 % |
| Mauvaises prédictions | 64 223 687 | 61 082 205 | −4,89 % |

[Compteurs bruts](perf-stat.txt). Comme précédemment, le temps total affiché
par perf inclut aussi le chargement et le warm-up ; il n'est pas utilisé.
Le [profil final](perf-flat.txt) conserve `reduce_masked` à 11,94 % et `eval_bits`
à 10,00 % de temps propre. Les deux symboles de clonage passent de 5,65 % à
4,38 % cumulés. Les proportions sont relatives au total de chaque exécution ;
les échantillons localisent les coûts, les compteurs et chronométrages mesurent
le gain. Aucun changement SIMD supplémentaire n'est livré : les prototypes
à quatre voies de la passe précédente restent écartés.

## Pistes étudiées

Le profil du palier précédent localisait notamment `reduce_masked` à 11,29 %,
`eval_bits` à 9,03 %, la destruction d'AST à 5,82 %, et les deux symboles de
clonage d'AST/vecteurs à environ 5,65 % cumulés de temps propre.

| Variante isolée | Première médiane 41k | Delta contre témoin encadrant | Verdict |
|---|---:|---:|---|
| Un tampon `VecDeque` pour la pile et la sortie de `flatten` | 2,036319 s | +4,73 % | DROP |
| `IndexSet` : identifiants contigus, restauration par indice | 1,937884 s | −0,33 % | DROP |
| Maximum des variables sans construire d'ensemble | 1,954767 s | +0,54 % | DROP |
| Emprunt de l'entrée de `solve_linear_inner` | 1,878672 s | −3,38 % | KEEP, répété ci-dessous |

Le témoin encadrant est la moyenne des médianes avant/après :
(1,927426 + 1,961276) / 2 = 1,944351 s. Chaque médiane porte sur cinq runs
après un warm-up complet. Ces résultats ne justifient pas l'ajout des trois
premières variantes : changer la structure de données ne suffit pas à gagner.

Une cinquième variante énumère directement les blocs de sur-ensembles à modifier
pendant la mise à jour des coefficients. Elle conserve la transformation
incrémentale et l'ordre des mises à jour, en supprimant le test sur chaque indice.
La parité des coefficients a été vérifiée pour toutes les combinaisons jusqu'à
10 variables et quatre coefficients, avec débordement arithmétique inclus.
Elle n'est pas retenue faute de gain global suffisant.

## Allocations

| Mesure instrumentée | Palier précédent | Avec emprunt | Économie supplémentaire |
|---|---:|---:|---:|
| Allocations | 29 583 855 | 27 848 733 | 1 735 122 / 5,87 % |
| Réallocations | 863 280 | 863 280 | 0 |
| Octets demandés cumulés | 1 777 063 642 | 1 690 064 250 | 86 999 392 / 4,90 % |

Depuis `prettify` : **39 561 465 → 27 848 733 allocations, soit −29,61 %**.
Les octets cumulés ne sont pas une mesure de la mémoire résidente maximale.
Les durées des exécutables instrumentés ne servent pas à mesurer le gain.
[Compteurs complets](allocations-and-calls.txt).

## Validation et reproduction

Pour chacune des cinq variantes : **41 000 sorties AST + texte identiques** au
véritable témoin `prettify`, et **34 176 résultats générés identiques**
(33 280 réductions sur toutes les largeurs 0–64 et 896 simplifications sur sept
largeurs 1–64). La version retenue conserve **41 000 OK / 0 OKZ / 0 NG**, avec
200 évaluations aléatoires par expression. Les tests du workspace en release,
toutes fonctionnalités activées, passent ; formatage et diff également.
[Journal](validation.txt).

Le [script de reproduction](../reproduce.sh) compile `prettify` et les sources
actuelles dans des répertoires Cargo distincts, puis compare les résultats et
mesure les deux exécutables. Pour isoler seulement l'effet de cette passe,
préparer deux copies des sources actuelles, et appliquer `borrow.patch` à
l'envers dans l'une des copies. Le protocole de `perf` avec activation par FIFO
est décrit dans le [rapport précédent](../README.md#reproduction).

Les compteurs de copies s'obtiennent dans les copies instrumentées en ajoutant,
à l'entrée de `solve_linear_inner`, un compteur d'appels et l'accumulation de
`e.size()`. Cette instrumentation est identique pour les variantes possédée et
empruntée et ne figure pas dans la bibliothèque livrée.

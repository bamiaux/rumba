# Loki : profil et cache local contigu

**Oui : Loki reste le premier poste en temps total.** Après le palier QSynth,
il représente **51,7 % du temps** sur la somme des médianes des sept corpus.
QSynth reste beaucoup plus cher par expression, mais ne représente plus que
20,0 % du total dans cette comparaison.

Une optimisation retenue : remplacer la table de hachage du cache **local à
une simplification** par un vecteur de couples clé/valeur. Mesure finale Loki :
**905,601 → 884,379 ms, soit −2,34 %**, avec **−4,68 % d'instructions**.
Les premiers essais donnaient 4–5 % ; le résultat final plus prudent est celui
à retenir. Aucun commit effectué.

## Poids des corpus

Même binaire de départ, un warm-up puis cinq runs par corpus, CPU 2. Parsing
et clonage des entrées exclus du chronomètre. La somme des médianes est
**1,861426 s** ; ce n'est pas une médiane du corpus complet mesuré séparément.

| Corpus | Expressions | Temps | Part | Temps moyen par expression |
|---|---:|---:|---:|---:|
| **Loki** | **25 000** | **962,555 ms** | **51,71 %** | **38,50 µs** |
| QSynth EA | 500 | 371,470 ms | 19,96 % | 742,94 µs |
| NeuReduce | 10 000 | 231,034 ms | 12,41 % | 23,10 µs |
| MBA Flatten | 3 000 | 151,799 ms | 8,15 % | 50,60 µs |
| MBA Obf Linear | 1 000 | 84,630 ms | 4,55 % | 84,63 µs |
| MBA Obf Nonlinear | 1 000 | 50,476 ms | 2,71 % | 50,48 µs |
| Syntia | 500 | 9,462 ms | 0,51 % | 18,92 µs |

Les [mesures brutes](measurements.json) contiennent les effectifs et les runs.
Les corpus ont été mesurés séquentiellement, avec filtrage avant le chronomètre.
Le contrôle après optimisation confirme le classement : Loki **909,552 ms /
51,31 %**, QSynth **363,716 ms / 20,52 %**. Cette seconde comparaison des poids
ne remplace pas les mesures ABBA de gain ci-dessous.

## Points chauds de Loki

Profil `perf` réservé aux 25 000 expressions Loki, dix passes après warm-up.
Les compteurs sont activés par FIFO uniquement pendant la simplification,
jamais pendant le parsing ou le clonage des entrées entre passes.
9 626 échantillons avant, 9 175 après, à 997 Hz ; aucun échantillon perdu.

| Fonction, temps propre avant | Part |
|---|---:|
| `reduce_masked` | 12,55 % |
| `eval_bits` | 6,44 % |
| `malloc` | 6,13 % |
| Destruction d'AST | 4,98 % |
| `free` | 4,33 % |
| `reduce_and` | 3,10 % |
| Collecte des vecteurs sur place | 3,05 % |
| Clonage de vecteurs | 2,71 % |
| Clonage d'AST | 2,25 % |
| `DefaultHasher::write` | 1,75 % |

Profils [avant](profile-before.txt), [inclusif avant](profile-inclusive-before.txt)
et [après](profile-after.txt). Les temps inclusifs des parcours récursifs se
recouvrent : ils ne s'additionnent pas. Les pourcentages après sont rapportés
à un total différent et ne mesurent pas individuellement une accélération.

Une passe instrumentée compte **9 087 230 visites de réduction**,
**15 167 365 allocations**, **124 450 appels à `solve_linear`**,
**310 685 tables de vérité** et **16 052 431 visites d'évaluation scalaire**.
Ces compteurs orientent l'enquête vers les nombreux petits arbres et la
gestion mémoire. Le temps instrumenté n'est jamais utilisé comme benchmark.

## Changement retenu

Le cache local était une `HashMap<Expr, Expr>`. Chaque recherche calculait
le hachage récursif de la clé, alors que la grande majorité des recherches
portent sur très peu d'entrées :

- **80,0 %** des lectures Loki voient au plus deux entrées ; **95,5 %**, au plus quatre.
- La plus grande taille observée à la lecture est **15** pour Loki et **31** pour QSynth.
- Le vecteur stocke les clés et valeurs côte à côte et teste leur égalité
  structurelle. Une inégalité peut s'arrêter avant d'avoir parcouru toute la clé.

[Histogrammes complets](cache-sizes.txt). Ce comptage est distinct du comptage
des allocations : l'histogramme utilise lui-même une structure allouée.

La recherche rend toujours un AST possédé. L'insertion remplace une clé égale
ou ajoute une nouvelle paire. La récursion du solveur ne conserve donc aucun
emprunt du cache. Aucun candidat, ordre de recherche, coefficient, règle ou
critère d'arrêt n'est modifié.

La recherche est **linéaire dans le nombre d'entrées** : le choix vise les petits
caches par appel mesurés ici, pas une garantie de vitesse pour des caches
locaux arbitrairement grands. Le cache partagé et réutilisable `MbaCache`,
employé par `simplify_mba_cached`, conserve sa table de hachage. Les gains de
ce rapport portent sur `simplify_mba`.

[Patch retenu](changes.patch) : **+9 lignes nettes de production**, plus tests.
Aucune dépendance et aucun `unsafe` ajoutés. Le SIMD du palier QSynth reste
présent ; aucun nouveau chemin SIMD n'est livré dans cette passe Loki.

## Variantes testées

Chaque variante a son propre checkout et son propre répertoire Cargo. Les
patches expérimentaux ci-dessous s'appliquent **indépendamment à l'état avant
cette passe**, qui contient déjà les cinq optimisations précédentes.

| Variante | Deux médianes Loki exploratoires | Décision |
|---|---:|---|
| [Aplatissement dans un seul buffer](flat.patch) | 935,005 / 916,291 ms | DROP |
| [Annulation des doublons XOR sur place](pairs.patch) | 932,053 / 925,137 ms | DROP |
| [FxHashMap pour le cache](hash.patch) | 972,956 / 929,027 ms | DROP |
| Témoin encadrant ces trois essais | 939,526 / 907,702 ms | référence |
| [Réutilisation de la dernière branche distribuée](distribute.patch) | 940,651 / 920,295 ms | DROP |
| [Filtrage sur place sans enfant à aplatir](flatfilter.patch) | 939,231 / 958,801 ms | DROP |
| [Cache local contigu](veccache.patch) | 897,830 / 904,637 ms | KEEP, revalidé ci-dessous |
| Témoin encadrant ces trois essais | 944,150 / 958,496 ms | référence |

Les premiers changements ne dégagent pas un gain Loki suffisamment net et
stable par rapport à la variation des témoins. La copie évitée à la dernière
branche distribuée reste une piste, pas un gain confirmé retenu ici.

Une [variante d'emprunt des expressions échantillonnées](samples.patch) et sa
[combinaison avec le cache](combined.patch) ont également été comparées.
L'emprunt seul donne 904,153 / 903,899 ms, le cache seul 864,550 / 867,508 ms,
leur combinaison 904,520 / 879,298 ms, pour des témoins à 912,566 / 900,368 ms.
La combinaison ne confirme pas de gain supplémentaire sur Loki. Elle donne
de meilleurs temps globaux dans cet essai, mais ne fait pas partie du patch
retenu pour cette passe ciblée. Tous les runs sont conservés dans le JSON.

## Mesure finale

Sources finales recompilées séparément, release avec **`--features parse`, sans
JIT**, comme le témoin. Ryzen 9 3900X virtualisé, Rust 1.95.0, CPU 2.
Aucun build, test ou second benchmark simultané. Deux séries de cinq runs
après warm-up, ordre **témoin / final / final / témoin** pour chaque périmètre.
Le témoin inclut le [palier QSynth](../qsynth/README.md) ; ce n'est pas `prettify` brut.

| Périmètre | Avant | Après | Delta |
|---|---:|---:|---:|
| Loki, série 1 | 925,961 ms | 896,437 ms | −3,19 % |
| Loki, série 2 | 895,357 ms | 877,803 ms | −1,96 % |
| **Loki, médiane des dix runs regroupés** | **905,601 ms** | **884,379 ms** | **−2,34 %** |
| 41k, série 1 | 1,848901 s | 1,688973 s | −8,65 % |
| 41k, série 2 | 1,743615 s | 1,692670 s | −2,92 % |
| **41k, médiane des dix runs regroupés** | **1,790035 s** | **1,691001 s** | **−5,53 %** |

La variation des témoins globaux est importante : le gain global observé va
de **2,9 % à 8,7 %** selon la série. La médiane regroupée n'est pas une promesse
de gain stable de 5,5 %. Les compteurs matériels ci-dessous confirment séparément
la baisse de travail sur Loki. Les [données brutes](measurements.json) incluent
temps CPU, warm-ups, chronométrages et empreintes des binaires et sorties.

## Compteurs matériels et allocations

Médianes de trois mesures `perf stat`, chacune portant sur **dix passes Loki** :

| Compteur | Avant | Après | Delta |
|---|---:|---:|---:|
| Cycles | 37 974 606 750 | 36 939 949 582 | −2,72 % |
| Instructions | 72 469 690 246 | 69 080 084 355 | −4,68 % |
| Branchements | 12 403 669 726 | 11 924 818 381 | −3,86 % |
| Mauvaises prédictions | 310 157 158 | 302 209 991 | −2,56 % |

Les sorties brutes figurent dans le JSON. Les secondes globales de `perf`
incluent des phases désactivées et ne servent pas de temps de simplification.

Comptage séparé, **une passe Loki** :

| Compteur | Avant | Après |
|---|---:|---:|
| Allocations | 15 167 365 | 15 162 262 |
| Réallocations | 585 178 | 587 111 |
| Octets demandés cumulés | 921 176 306 | 918 880 194 |

Seulement **5 103 allocations supprimées**, avec **1 933 réallocations de plus**.
Le bénéfice vient principalement du travail de hachage évité, pas d'une baisse
massive des allocations. Tous les compteurs d'appels et de visites instrumentés
restent identiques. [Avant](counts-before.txt), [après](counts-after.txt).
Les octets cumulés ne sont pas le pic de mémoire résidente.

## Validation et reproduction

- **41 000 AST et rendus identiques** au témoin original `prettify`.
- **41 000 OK / 0 OKZ / 0 NG**, avec 200 évaluations aléatoires par expression.
- **33 280 réductions + 896 simplifications générées** : aucune différence.
- Test des deux caches : 256 clés structurelles proches, remplacements,
  absences et indépendance des valeurs retournées.
- `cargo test --workspace --release --all-features` : PASS.
- `cargo test -p rumba-core --release --lib`, sans JIT : PASS.
- Formatage et `git diff --check` : PASS. [Journal](validation.txt).

Exécuter `bash docs/performance/loki/reproduce.sh`. Le script crée deux copies
des sources actuelles, inverse uniquement `changes.patch` dans le témoin,
compile dans deux répertoires Cargo distincts, compare les sorties et mesure
Loki puis le corpus complet en ABBA. Les résultats restent dans le dossier
temporaire affiché.

La [sonde Loki](probe.rs), copiée dans `core/examples/loki_probe.rs`, filtre
`loki_tiny` uniquement dans le harness, jamais dans la bibliothèque. Son mode
`profile CONTROL ACK` effectue dix passes avec activation/désactivation des
compteurs par FIFO. Employer les commandes du
[protocole perf](../phase3-followup/README.md#reproduction) avec ce binaire.

Pour les comptes d'allocations et d'appels, compiler une troisième copie
jetable avec `phase3/probe.rs`, filtrée sur `loki_tiny`, puis appliquer
`phase3/instrument.py` et `phase3-followup/variable_profile.py`. Exécuter `once` ;
ne pas chronométrer cette instrumentation. Les tailles du cache se relèvent
séparément en histogrammant `self.entries.borrow().len()` à l'entrée de
`LocalCache::get`. Aucune instrumentation n'est livrée dans la bibliothèque.

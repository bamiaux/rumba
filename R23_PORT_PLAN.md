# R23 — analyse du brouillon et plan de portage

## Conclusion

**Le portage est intégré comme candidat dans la branche de travail.** Le ZIP
n’était pas intégrable tel quel : LOW appelait une fonction absente, le cut avait
une signature incomplète et plusieurs chemins ne respectaient pas la largeur du
calcul. Ces points ont été raccordés au moteur existant, puis qualifiés sur le
corpus de non-régression et sur le CSV fourni.

Les documents du ZIP sont traités comme des propositions et des déclarations de
leur auteur. Leur « mission », leurs scores et leur demande de rebase ne constituent
ni une validation Rust ni une autorisation en eux-mêmes ; le code intégré est
accepté uniquement au vu des contrôles exécutés ici.

## 1. État vérifié le 15 septembre 2026

### Dépôt et archive

- Base : branche `prune`, commit `e567d1c6c92db4d16caafec3456f72e98e585d0d`.
- Arbre de travail propre avant l’analyse.
- Les 13 empreintes SHA-256 du manifeste correspondent aux fichiers extraits.
- `hidden_cut_reference.rs` est identique octet pour octet au
  `core/src/simplify/hidden_cut.rs` actuel.
- Le commit v1.0.1 cité dans le ZIP existe localement ; ses deux empreintes de
  fichiers correspondent également. Cela vérifie la référence, pas l’opportunité
  du rebase.
- Rust et Cargo 1.95.0 disponibles ; aucune dépendance nouvelle identifiée pour
  ce portage. Les types `Expr`, `BiMap` et `LinearCache`, les manifestes et la CI
  actuels ont été examinés.

### Tests exécutés sur la branche actuelle

```sh
cargo test -p rumba-core --release --all-features -- --nocapture
```

Résultat : **30 tests unitaires, 13 tests d’intégration et 1 doctest réussis**.
Les sept tests de corpus donnent :

| Corpus | Cas | Direct | OKZ | NG |
|---|---:|---:|---:|---:|
| loki_tiny | 25 000 | 25 000 | 0 | 0 |
| neureduce | 10 000 | 10 000 | 0 | 0 |
| mba_flatten | 3 000 | 3 000 | 0 | 0 |
| mba_obf_linear | 1 000 | 1 000 | 0 | 0 |
| mba_obf_nonlinear | 1 000 | 1 000 | 0 | 0 |
| syntia | 500 | 500 | 0 | 0 |
| qsynth_ea | 500 | 500 | 0 | 0 |
| **Total** | **41 000** | **41 000** | **0** | **0** |

« Direct » signifie ici égalité structurelle entre la simplification de SOURCE
et celle de TARGET, calculées séparément. OKZ signifie que leur différence est
réduite à zéro. Les contrôles sémantiques existants utilisent 200 affectations
aléatoires par cas contre la vérité terrain : ils ne constituent pas une preuve
exhaustive. Les temps de cette exécution ne sont pas un benchmark comparatif.

Le README du dépôt présente donc des résultats plus anciens que cette mesure.

### Vérification du brouillon

Dans une copie temporaire du dépôt, les cinq modules du ZIP ont été déclarés et
le champ de métadonnées attendu a été ajouté pour permettre leur vérification de
types. Les nouvelles passes n’ont pas été activées. Un `cargo check --offline
--locked -p rumba-core --lib --features parse` échoue notamment sur :

```text
E0432  low_prefix.rs:10    super::reduce_masked_plain introuvable
E0106  filtered_cut.rs:316  durée de vie manquante dans rank
```

Un `unused_mut` est aussi signalé dans `low_prefix.rs:899`, incompatible avec la
CI qui interdit les avertissements. Le patch LOW passe `git apply --check`, mais
ne fournit pas la fonction manquante. Son applicabilité textuelle ne suffit donc
pas à rendre l’ensemble compilable.

À cette étape préalable, le port complet n’avait pas été compilé ni exécuté et
aucun code de production du dépôt n’avait été modifié.

## Résultat de l’exécution du plan

Les lots LOW/Scalar/Section, largeur et remplacement du cut ont été exécutés :

- `core/src/reduce/low_prefix.rs` est branché avant le flattening, avec une
  entrée interne qui désactive la reprojection pendant sa propre normalisation ;
- les masques préfixes dynamiques sont conservés lors de la polynomialisation,
  et les facteurs binaires idempotents sont réduits ;
- `scalar_precision` normalise les coefficients après le point fixe ;
- `filtered_cut` remplace `hidden_cut`, enregistre la largeur et la provenance
  des coordonnées cachées, puis l’ancien module est supprimé ;
- l’expansion récursive des définitions cachées empêche les coordonnées
  intermédiaires de fuir dans le résultat public.

Validation finale exécutée le 15 septembre 2026 :

| Vérification | Résultat |
|---|---:|
| `cargo fmt --all --check` | OK |
| `cargo clippy --workspace --all-features --all-targets -- -D warnings` | OK |
| `cargo test --workspace --all-features` | OK, 41K corpus inclus |
| corpus release (`rumba-core`, 41 000 cas) | **41 000 direct, 0 OKZ, 0 NG** |
| `maskspark_v6(1).csv`, largeur 8, 28 cas (500 échantillons/cas) | **28/28 sémantiques, 18/28 directs** |

Le CSV ne fournit ni largeur ni oracle versionné ; la mesure MaskSpark est donc
documentée comme contrôle 8 bits reproductible, tandis que le corpus 41K reste
le gate principal demandé. NG54 n’a pas été fourni dans les fichiers accessibles
et n’est pas présenté comme un résultat acquis.

Le cut filtré reste actif dans les résolutions récursives : une variante qui le
désactivait a introduit 17 NG sur `loki_tiny` et a été écartée. Cette décision
conserve le comportement validé du corpus ; la règle « aucun cut récursif » du
plan initial reste une question ouverte à qualifier avec un oracle dédié.

## 2. Ce que le ZIP propose réellement

| Élément | Fonction | Décision de portage |
|---|---|---|
| `qualification_bridge/low_prefix.rs` | Simplifier les bits bas avant les développements du réducteur ; contient les calculs symboliques Shadow, Cell et DisjointPair | Partie nouvelle la plus importante et la plus coûteuse à qualifier ; intégrer à un réducteur unique |
| `section_quotient.rs` | Idempotence d’un facteur valant 0 ou 1 ; élimination de multiples de puissances de deux sous un masque | Qualifier les deux lois et leur position dans la passe |
| `scalar_precision.rs` | Normaliser les coefficients selon la précision réellement nécessaire aux facteurs | Inclure dans les essais de largeur ; établir son rôle avant de supprimer ou fusionner des passages |
| `hidden_orbit.rs` | Partager une coordonnée entre une expression cachée et son complément | Adapter l’internage existant ; éviter une seconde implémentation équivalente |
| `filtered_cut.rs` | Produire des relations par annihilateur du prédécesseur et ordre booléen, puis effectuer une substitution contextuelle | Réutiliser les utilitaires du cut actuel et remplacer ses générateurs de relations |
| `SIMPLIFY_INTEGRATION_SKETCH.rs` | Esquisse des points d’appel et des modes récursifs | Refaire le raccordement à partir de tous les appels réels du dépôt |
| `hidden_cut_reference.rs` | Copie de référence de l’ancien cut | Utiliser pour comparaison ; aucune copie supplémentaire en production |

Le principe LOW correspond à une pratique établie : simplifier un opérande selon
les bits dont son utilisateur a besoin. LLVM applique cette approche dans
[InstCombine / SimplifyDemandedBits](https://llvm.org/doxygen/InstCombineSimplifyDemanded_8cpp_source.html).
Cela appuie le placement sous le masque consommateur ; cela ne valide pas les
quotients algébriques propres à ce ZIP.

## 3. Écarts et risques à résoudre

### A. Le scénario de remplacement CHG est dépassé

Depuis `2b6becd`, `hidden_gauge::intern` s’exécute déjà dans `hide_in_var` et utilise
des clés structurelles et des orbites propres au solveur. Il n’existe plus de
passe globale `hidden_gauge::canonicalize` à supprimer.

Les nouveautés utiles du brouillon sont surtout les restrictions de largeur,
la distinction des résolutions récursives et le refus d’utiliser une coordonnée
cachée sans clé admissible. Le partage des clés par `Arc` est un choix de
représentation à évaluer, pas une raison suffisante de réécrire l’internage.

Depuis `585c55b`, le moteur de patterns et `SimplifyOptions` ont également été
supprimés. Les branches « patterns ON/OFF » de l’archive n’ont pas à être recréées.

Références : `core/src/simplify.rs:99`, `:385` ;
`core/src/simplify/hidden_gauge.rs:103`.

### B. LOW manque d’une frontière de réduction explicite

`low_prefix::render_terms` appelle `reduce_masked_plain`. Une simple redirection
vers le réducteur qui active LOW pourrait rappeler la projection pendant sa
propre normalisation et changer l’ordre attendu des transformations.

Prévoir un réducteur unique avec une entrée interne de normalisation sans
projection LOW, dont le contexte se propage aux enfants. Ce découpage appartient
à l’algorithme ; il ne nécessite ni ancien backend conservé ni option publique.
La projection doit intervenir avant `Reducer::flatten`, qui réduit déjà les
enfants et peut développer l’expression.

Références : archive `low_prefix.rs:1646`, `REDUCE_INTEGRATION_FRESH.patch` ;
`core/src/reduce.rs:94`, `:208`.

### C. La provenance de largeur est incomplètement contrôlée

Une égalité valable modulo `2^k` ne permet pas de réécrire les bits supérieurs dans
un calcul sur `n > k` bits. Le garde-fou `meta.width == solver.n` est donc central,
mais le brouillon ne l’applique pas à tous les chemins :

- `ray` fabrique une vue de variable libre lorsque la métadonnée est absente,
  sans vérifier que la variable n’est pas une coordonnée cachée connue.
- `known`, `fold_known` et la collecte de `order` réemploient les définitions
  résidentes sans consulter leur largeur.
- Les recherches exactes expression/complément de l’esquisse interviennent avant
  la décision `orbit_allowed`. Il faut préciser leur contrat pour les largeurs
  différentes et pour les dépendances entre coordonnées cachées.

Ce sont des obligations de preuve et de test identifiées par lecture, pas des
contre-exemples sémantiques reproduits pendant cette analyse. Une normalisation
scalaire après restauration ne peut pas réparer une relation fausse utilisée
plus tôt.

Références : archive `filtered_cut.rs:193`, `:238`, `:475` ;
`SIMPLIFY_INTEGRATION_SKETCH.rs:61`.

### D. L’esquisse oublie une autre entrée récursive

`merge_hidden::prove_hidden_relation` construit directement un `MBASolver::new`.
Si ce constructeur choisit R23 par défaut, les preuves internes pourront activer
le cut malgré un mode récursif censé l’interdire.

Le contexte doit donc être transmis aussi à ces preuves, ainsi qu’à leurs
descendants. Le port conserve pour l’instant le constructeur local existant et
fait respecter la largeur par les métadonnées ; l’essai d’un mode « cut interdit
en récursion » a régressé `loki_tiny`. Remplacer le compteur TLS par un état
explicite du solveur reste le prochain travail si un oracle dédié justifie cette
séparation.

Référence : `core/src/simplify/merge_hidden.rs:201`.

### E. Les preuves expérimentales annoncées ne sont pas fournies

Le ZIP ne contient ni l’implémentation Python R23 de référence, ni le jeu NG54,
ni leurs sorties détaillées ou le générateur de fuzzing. Le fichier fourni ensuite,
`/home/bamiaux/Downloads/maskspark_v6(1).csv`, est bien exploitable : 28 lignes
`source,expected`, sans identifiant de largeur ni version d’oracle.
Les 12 cas de gauge et le protocole des 82 000 comparaisons ne sont pas livrés
comme un jeu de qualification identifié.

Les scores `49/49`, `54/54`, les 409 200 évaluations et les résultats d’ablation
restent des déclarations du manifeste. **Avant le port**, une exécution
exploratoire du CSV MaskSpark v6 donnait 19/28 équivalences sémantiques et 4/28
égalités structurelles à 8 bits (10/28 et 4/28 à 64 bits). La mesure finale du
candidat est donnée plus haut : 28/28 sémantiques et 18/28 directs à 8 bits.
La largeur attendue n’est toujours pas indiquée par le fichier et plusieurs cas
sont suffisamment longs pour être sensibles au budget de calcul.

La règle de décision pratique suit votre priorité : **41K sans NG est le critère
principal de non-régression**. NG54 et MaskSpark servent de contrôles ciblés pour
les nouvelles capacités ; leur bookkeeping détaillé reste secondaire, mais un
échec doit être localisé avant d’accepter le remplacement.

### F. L’économie de code et le coût d’exécution restent à démontrer

Avec une convention uniforme — lignes non vides, commentaires compris, avant
le premier module de tests — les modules ajoutés dans le candidat représentent
2 607 lignes (1 945 pour LOW, 567 pour le cut filtré et 95 pour Scalar). Le
remplacement supprime 924 lignes de `hidden_cut`; les raccordements et le test de
masques ajoutent encore du code. Cette croissance est mesurée mais n’est pas
présentée comme un gain de simplicité.

La base globale mesurée sur les fichiers Rust de `core/src`, `cli/src` et des
trois bindings est de 4 911 lignes selon cette même convention. La mesure finale
devra inclure tous les fichiers ajoutés ou déplacés dans ces périmètres.

LOW énumère notamment jusqu’à `2^t` mintermes, construit des polynômes de cellules
et des bases de paires ; le cut compare plusieurs couples de définitions et
résout des relations linéaires. La limite locale de 20 variables ne démontre pas
à elle seule le coût total annoncé. Examiner les produits intermédiaires, les
copies, les allocations et les répétitions avant d’affirmer une amélioration.

## 4. Architecture retenue pour le plan

```text
Expression source
  → réduction native, avec LOW au premier AND consommateur d’un masque préfixe
  → boucle de simplification existante
      → polynomialisation / internage caché avec provenance de largeur
      → fusion des composants cachés, avec contexte de preuve explicite
      → résolution polynomiale
      → un cut filtré, avant restauration des coordonnées
      → restauration
  → normalisation scalaire à la largeur du calcul
  → réduction finale / prettify
```

Conserver la borne actuelle de huit passes et la règle d’arrêt sur une passe qui
ne réduit plus la taille. Une modification de cette règle demanderait une
justification mesurée distincte.

| Contexte | Internage par orbite | Cut filtré |
|---|---|---|
| Résolution publique R23 | Seulement avec largeur et provenance admissibles | Oui |
| Résolution cachée récursive 64 bits | Oui, selon la largeur/provenance enregistrée | Oui, même chemin validé par le corpus |
| Résolution cachée récursive sous 64 bits | Internage exact sans orbite de complément | Oui, avec garde de largeur |
| Preuve d’une relation de fusion | Contexte local adapté à sa largeur | Oui, avec les mêmes garde-fous |

Le sort de LOW, Scalar et Section dans les contextes récursifs doit être arrêté
à partir de la référence et de tests : l’esquisse applique les finitions sans
condition, tout en qualifiant `Plain` de comportement « gelé ». Les noms des
modes seuls ne résolvent pas cette ambiguïté.

## 5. Lots de réalisation

### Lot 0 — rendre la qualification reproductible

1. Conserver le commit de référence et les résultats détaillés déjà mesurés.
2. Identifier la largeur et l’oracle de `maskspark_v6(1).csv`, puis obtenir NG54,
   les cas de gauge et l’oracle Python. Le CSV reçu devient le premier témoin
   concret, même s’il ne remplace pas MaskSpark v8.
3. Compléter le banc existant avec la sortie normalisée par cas, les erreurs
   distinctes des NG, les graines et le contrôle SOURCE → résultat.
4. Faire échouer la qualification si le 41K introduit un NG. Pour les témoins
   ciblés, conserver au minimum une vérification sémantique ; ne pas bloquer le
   premier jalon sur une égalité de forme si l’oracle attend une normalisation
   différente.
5. Préparer les mesures de temps et de mémoire avant toute comparaison.

**Sortie :** référence rejouable ; aucun score importé du manifeste comme résultat
Rust acquis.

### Lot 1 — LOW et Section de bout en bout

1. Ajouter `core/src/reduce/low_prefix.rs` avec sa véritable entrée de réduction
   interne ; supprimer l’import orphelin et les avertissements.
2. Brancher la projection avant le développement des enfants dans `reduce_and`.
3. Qualifier Scalar et les deux lois Section, puis les raccorder à la passe.
   Vérifier si la double séquence Scalar/Section de l’esquisse est nécessaire
   avec des témoins ; ne pas multiplier les parcours sans justification.
4. Tester les masques préfixes, nuls, complets, non contigus et imbriqués, ainsi
   que les produits à coefficients pairs et les identités additives annulées.
5. Vérifier aussi `Expr::reduce`, puisque le point d’entrée modifié est public.

**Sortie obtenue :** 41K sans NG et équivalence sémantique conservée ; le CSV
MaskSpark v6 est à 28/28 sémantiques en 8 bits (18 directs). MaskSpark v8 et les
contrôles négatifs restent à faire dès réception d’un jeu identifié. Les
mécanismes cachés actuels restent le témoin de cette étape.

### Lot 2 — internage existant, largeurs et récursion

1. Faire évoluer `hidden_gauge::intern` pour enregistrer la largeur et traiter
   explicitement les coordonnées dont la clé n’est pas admissible.
2. Unifier les conditions d’accès aux définitions et à leurs compléments ; une
   coordonnée cachée sans provenance utilisable ne produit aucun certificat.
3. Introduire les modes privés nécessaires et les transmettre depuis toutes les
   constructions de solveur, y compris `merge_hidden`.
4. Remplacer le garde de preuve TLS par un état local transmis aux descendants.
5. Qualifier indépendamment orbites externes et récursives. Réutiliser les tests
   structurels actuels ; ajouter les cas de largeurs imbriquées et les 12 témoins.

**Sortie obtenue :** un seul internage actif, avec métadonnées de largeur et
internage exact pour les sous-masques ; 41K et MaskSpark sont conservés. Le cut
récursif n’a pas pu être désactivé sans régression, donc il reste partagé par le
chemin public et les preuves internes jusqu’à qualification d’un contexte plus
fin. Un renommage de module est facultatif ; l’ancien mécanisme a été supprimé.

### Lot 3 — remplacement du cut

1. Reprendre les utilitaires nécessaires du cut existant : monômes, coefficients,
   inverse impair, reconstruction et substitution contextuelle. La signature de
   durée de vie correcte de `rank` existe déjà dans le code actuel.
2. Ajouter les certificats d’annihilateur du prédécesseur et d’ordre booléen avec
   des préconditions de largeur explicites sur chaque chemin.
3. Enregistrer les métadonnées à la création des coordonnées et vérifier leur
   cohérence après fusion ou réutilisation.
4. Choisir une seule substitution avec un ordre déterministe et une mesure de
   progrès ; conserver la convention du monôme vide et les tests de contexte.
5. Activer le remplacement au point d’appel actuel après `solve_polynomial` et
   supprimer l’ancien chemin dans le même changement candidat.

**Sortie obtenue :** `filtered_cut` est actif au point d’appel existant et
`hidden_cut` a été supprimé ; 41K reste à 0 NG. Les ablations et les contrôles
négatifs annoncés par le ZIP restent non reproductibles sans NG54/MaskSpark v8.
Le témoin reste disponible dans le commit de base, sans deux cuts actifs dans le
produit.

### Lot 4 — simplification, performances et validation des interfaces

1. Supprimer les utilitaires dupliqués et tout code de qualification en production.
   Réutiliser `make_mask`, les types et les limites du dépôt quand les contrats
   sont identiques. Ne pas introduire de dépendance sans besoin démontré.
2. Mesurer le delta global de code et le coût propre à LOW et au cut.
3. Comparer base/candidat dans des processus neufs, en ordre AB puis BA,
   avec les mêmes entrées, options de compilation et conditions machine.
4. Relever temps CPU global, latence médiane/p95/p99, pire cas et mémoire maximale.
   Utiliser le runner `core/examples/corpus.rs` pour les métriques qu’il fournit
   déjà ; ses cinq répétitions internes ne remplacent pas les processus neufs.
5. Exécuter la CI pertinente sur core, CLI et bindings C/Python/Wasm, avec et sans
   JIT pour les contrôles sémantiques concernés.

**Sortie actuelle :** formatage, Clippy, compilation workspace et tests sont
validés. La comparaison AB/BA sur processus neufs, la mémoire maximale et le
benchmark base/candidat restent à faire avant de conclure sur le coût de la
croissance de code.

## 6. Matrice de validation

| Vérification | Critère |
|---|---|
| Corpus 41K | **0 NG** ; l’égalité directe est le meilleur signal quand elle reste disponible |
| MaskSpark v6 reçu | 28 cas ; largeur/oracle à documenter, puis contrôle sémantique |
| MaskSpark v8 | 49 cas ; score à reproduire en Rust |
| TARGET + 1 | Aucune collision de formes normales sur les 49 cas ; une erreur ne compte pas comme un rejet réussi |
| NG54 | 54 direct ; liste exacte requise |
| Gauge | 12 témoins identifiés ; comparaison SOURCE et TARGET séparée |
| Petite largeur | Énumération exhaustive des affectations pour les petits témoins sur 1 à 5 bits ; génération aléatoire reproductible en complément |
| Frontières de largeur | Cas ciblés 1/2/4/5/8/16/32/63/64, masques imbriqués, complément et multiplication paire |
| Métadonnées | Absentes, largeur différente, dépendance à une coordonnée locale : aucun certificat illégitime |
| Récursion | Aucun certificat hors largeur ; le cut reste actif dans le chemin validé |
| Cache et concurrence | Deux solveurs indépendants, puis cache public partagé, alternance de largeurs, résultats identiques au calcul sans partage |
| Coût | Tests représentatifs de beaucoup de variables cachées, produits et masques imbriqués ; pas de croissance intermédiaire laissée sans analyse |

Les ablations annoncées dans l’archive — prédécesseur désactivé : 1/54 ; ordre
désactivé : 53/54 ; Section désactivée : 47/49 — sont des hypothèses à confronter
aux mesures. La branche actuelle peut déjà couvrir certains témoins autrement.
Une divergence avec ces nombres doit être expliquée ; elle n’est pas à elle
seule une preuve d’erreur. Les interrupteurs d’ablation restent dans les tests
ou dans des variantes de qualification.

Commandes principales à reprendre de la CI :

```sh
cargo fmt --all --check
cargo check --workspace --all-features
cargo clippy -p rumba-core -p rumba --all-features --all-targets -- -D warnings
cargo clippy -p c -p pyrumba -p wasm --all-features --all-targets -- -D warnings
cargo test -p rumba-core -p rumba --release --all-features
cargo test -p rumba-core --release --features parse
cargo test -p c --all-features
```

Les tests Python, le test C lié à la bibliothèque et les tests Wasm sous Node
suivent les recettes existantes de `.github/workflows/ci.yml`. Les nouveaux jeux
de qualification auront leurs commandes documentées avec leurs données.

## 7. Rebase v1.0.1 : livraison distincte, conditionnelle

Le rebase final proposé par le ZIP n’est pas nécessaire à l’analyse ni au premier
portage. Le programmer seulement si livrer sur cette base est un objectif retenu.

Il ne s’agit pas uniquement de différences d’API : v1.0.1 contient le moteur de
patterns supprimé depuis, n’a pas le même retour de fusion des composants cachés
et précède l’optimisation de la seconde polynomialisation. Il faudrait reprendre
explicitement ces décisions utiles, supprimer les chemins obsolètes, puis
rejouer toute la qualification sur la nouvelle base.

**Premier jalon recommandé :** LOW + Scalar + Section et le cut filtré sont
désormais compilés et qualifiés, avec les 41 000 égalités directes conservées et
28/28 témoins MaskSpark sémantiques à 8 bits. Le benchmark comparatif et les jeux
NG54/MaskSpark v8 restent les suites de livraison.

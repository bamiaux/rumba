# État de travail — M2

Date : 2026-07-22
Branche : `bax`

## État architectural

- Z3/SAT a été retiré et ne doit pas être réintroduit.
- Le pipeline de production reste `RUMBA -> P7e -> P9-L`.
- P8 reste derrière `p8-experiments`.
- P9-Poly/P9-P, P10 et P11 restent expérimentaux.
- Ni P11a ni P11b ne sont branchés dans le pipeline de production.

## Reprise effectuée

- La ligne 53 reste à 41 nœuds contre une cible de 27 et atteint son budget de
  candidats.
- Les 7 tests unitaires P11a réussissent.
- Le benchmark des douze cas `Mul` donne 12/12 réduits et 6/12 au coût cible ou
  en dessous. La ligne 486 passe de 36 à 19.
- Pareto ne gagne aucun cas. Les lignes 53 et 481 coûtent environ 43,5 s et
  72,6 s par variante ; P11a reste donc sous garde expérimentale explicite.
- P11b est implémenté dans `core/src/p11b.rs` comme synthèse grammaticale
  bornée autonome pour les quatre cas sans multiplication.
- P11b résout `25, 114, 139, 423` aux coûts 12, 11, 14 et 9, avec sept
  certificats exacts réussis sur sept tentés et un maximum mesuré de 410 ms.
- Sur les seize bloqueurs : 16/16 réduits, 10/16 au coût cible ou en dessous,
  sans oracle de génération.

## Garde P11a

Une garde de temps interne serait non déterministe et une simple borne sur le
nombre de candidats ne couvre pas le coût variable de la normalisation et des
preuves. La décision actuelle est de garder P11a hors production et de ne
l'invoquer que par ses exemples expérimentaux.

## Point exact de reprise suivant

M2 est en cours et ne contient aucune obligation Cut, réservée à la roadmap K.
Le gate porte uniquement sur les 101 sorties réellement retenues. Le dump des
101 meilleurs candidats est diagnostique et non bloquant.

État du gate obligatoire après requête directe, fallback exact 4 x 16 bits et
premières chaînes de congruence structurées :

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

Les 17 requêtes directes encore `UNKNOWN` sont 17 couples, 17 résidus et 17
chaînes uniques. Ils sont tous dans QSynth : 12 P11a, 3 P11b et 2 P7e ; aucun
P9-L obligatoire ne reste inconnu. Les 303 arêtes locales donnent 220 `UNSAT`,
83 `UNKNOWN`, 0 `SAT` au direct court et 52 chaînes complètes après fallback.

M2d a ensuite exporté une preuve de congruence bottom-up pour ces 17 cas : 357
lemmes locaux et 17 ponts finaux. Z3 valide 334 lemmes et 4 ponts ; les chaînes
des lignes 25, 77 et 125 sont complètes, ce qui porte le gate à 87/101. Il reste
23 lemmes et 13 ponts `UNKNOWN`, répartis sur 14 sorties obligatoires. Deux
sondes directes représentatives avec un budget de 120 secondes sont aussi
restées `UNKNOWN` : la prochaine reprise doit exporter les états de
carry/retenue P9 plutôt qu'allonger globalement les timeouts.

Suite diagnostique séparée : `81 UNSAT`, `20 UNKNOWN`, `0 SAT`. Ses timeouts ne
bloquent pas M2.

Les temps observés ne servent pas au gate. Le runner utilise quatre workers par
défaut, soit environ douze activités pour le portfolio Z3, et accepte une borne
explicite via `M2_Z3_JOBS`. Aucun retry long n'est lancé automatiquement. Le
pipeline de production n'a pas changé et M3 n'a pas commencé.

Commandes utiles :

```console
cargo run --release -q -p rumba-core --features parse --example p11a_micro_gate -- 53
cargo run --release -q -p rumba-core --features parse --example p11a_mul_corpus
cargo run --release -q -p rumba-core --features parse --example p11b_corpus
just m2-z3 /chemin/vers/z3
```

## Fichiers de résultats importants

- `results.md` : état du corpus de production.
- `p10_results.md` : P10a/P10b/P10c.
- `p11a_results.md` : P11a-2 après la correction 486 et benchmark des 12.
- `p11b_results.md` : P11b et bilan expérimental des 16.
- `m2_z3_results.md` : protocole, artefacts et gate courant de M2.

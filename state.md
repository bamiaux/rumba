# État de travail — P11a/P11b

Date : 2026-07-21
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

L'implémentation et les rapports P11b sont présents dans l'arbre de travail.
Le pipeline de production n'a pas changé. La prochaine décision est soit de
conserver P11 comme expérience, soit de chercher une compression supplémentaire
des six cas `Mul` encore au-dessus de leur cible, en priorité 53 et 481 sans
augmenter leur coût déjà dominant.

Commandes utiles :

```console
cargo run --release -q -p rumba-core --features parse --example p11a_micro_gate -- 53
cargo run --release -q -p rumba-core --features parse --example p11a_mul_corpus
cargo run --release -q -p rumba-core --features parse --example p11b_corpus
```

## Fichiers de résultats importants

- `results.md` : état du corpus de production.
- `p10_results.md` : P10a/P10b/P10c.
- `p11a_results.md` : P11a-2 après la correction 486 et benchmark des 12.
- `p11b_results.md` : P11b et bilan expérimental des 16.

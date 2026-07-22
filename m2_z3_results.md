# M2 — validation externe avec Z3

## Frontière architecturale

Z3 reste un binaire externe. Aucune crate Z3, aucune FFI et aucun appel Z3 ne
sont ajoutés au code de production. L'exemple Rust exporte uniquement des
formules SMT-LIB ; le script shell invoque ensuite le binaire séparément.

```console
just m2-z3 /chemin/vers/z3
```

Le runner utilise quatre workers par défaut. Chaque worker exécute un portfolio
de trois tactiques, soit environ douze activités de résolution au maximum sur
le Ryzen 3900X. `M2_Z3_JOBS` permet de modifier explicitement cette borne.

## Corpus exporté

Les 101 anciens NG sont reconstruits sans utiliser `expected` pour générer un
candidat :

| Passe créditée | Réécritures |
|---|---:|
| P7e | 16 |
| P9-L | 69 |
| P11a | 12 |
| P11b | 4 |
| **Total** | **101** |

Trois vues sont produites :

1. les 101 sorties réellement retenues, de la source historique à la sortie ;
2. trois arêtes locales par ligne : `source -> ordinary -> P7e -> sortie` ;
3. les 101 meilleurs candidats autonomes, diagnostiques et non bloquants.

Le dump `artifacts/m2_z3/rewrite_comparison.tsv` conserve les expressions, les
passes, les coûts et les hashes. Parmi les meilleurs candidats, 98 sont plus
petits que la forme RUMBA ordinaire et 3 sont plus grands ; aucun n'est de même
coût.

`expected` sert uniquement à reproduire l'attribution historique des 16 cas
arrêtés après P7e. Il n'entre jamais dans P9-L, P11a, P11b ni dans la sélection
du meilleur candidat.

## Formulation Z3

Chaque AST est traduit syntaxiquement en bit-vecteurs de 64 bits. Les
sous-expressions communes sont partagées par des `define-fun`, sans
simplification RUMBA ni hypothèse issue d'un certificat interne.

La requête directe est exactement :

```text
source != candidat
```

Si elle expire, le vecteur est séparé en quatre tranches disjointes de 16 bits.
Quatre réponses `UNSAT` impliquent exactement l'absence de différence sur les
64 bits. Un seul `SAT` ou `UNKNOWN` garde respectivement le statut `SAT` ou
`UNKNOWN` pour la réécriture complète.

## Résultat M2a/M2b/M2c/M2d intermédiaire

Z3 : `4.16.0`, 64 bits.

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

- Le direct obligatoire et son fallback ferment 84/101 cas.
- Les 17 obligations restantes sont 17 couples `(source, candidat)`, 17
  résidus et 17 chaînes distincts : M2b ne révèle aucun doublon exact.
- Les 303 arêtes locales donnent 220 `UNSAT`, 83 `UNKNOWN`, 0 `SAT` au direct.
  Le fallback ciblé porte le total à 52/101 chaînes locales complètes, sans
  fermer de cas obligatoire supplémentaire.
- Les 17 bloqueurs sont tous QSynth : 12 P11a, 3 P11b et 2 P7e. Les 69 sorties
  P9-L sont déjà validées.
- Une décomposition bottom-up supplémentaire exporte 357 lemmes locaux et 17
  ponts de congruence. Z3 valide 334 lemmes et 4 ponts ; trois chaînes complètes
  (lignes 25, 77 et 125) portent le gate à 87/101.
- Les 14 chaînes encore incomplètes contiennent 23 lemmes locaux et 13 ponts
  finaux `UNKNOWN`. Deux sondes directes représentatives à 120 secondes sont
  également restées `UNKNOWN`.
- Aucun `SAT` n'a été observé.

Suite diagnostique séparée :

```text
diagnostic_total=101
diagnostic_unsat=81
diagnostic_sat=0
diagnostic_unknown=20
blocks_m2_gate=false
```

Les durées murales ne sont pas interprétables : la machine exécutait d'autres
charges simultanément.

## Pause et reprise ciblée

Le cache est granulaire par cas : une exécution avec `M2_Z3_REUSE=1` réutilise
les tranches acquises. La reprise de M2d doit maintenant exporter des
obligations inductives sur les états P9 de carry/retenue pour les 14 chaînes
encore incomplètes. Allonger uniformément les timeouts n'est pas la stratégie
retenue. Cut reste entièrement hors de M2 et relève de la série K ; M3 n'a pas
commencé.

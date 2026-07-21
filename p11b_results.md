# P11b — synthèse grammaticale bornée des retenues

## Périmètre

P11b traite les quatre bloqueurs sans multiplication de mots laissés par
`P7e -> P9-L` : `qsynth_ea.csv:{25,114,139,423}`. Il reste expérimental et
n'est pas branché dans le pipeline de production.

La génération utilise uniquement la source :

```text
variables et petites formes signées
-> paires bitwise bornées
-> surfaces additives de retenue bornées
-> compositions imbriquées et corrélées
-> empreinte sur 16 observations déterministes
-> preuve exacte figée RUMBA/P7e/P9
```

Les familles grammaticales couvrent les surfaces de retenue imbriquées, les
conjonctions/disjonctions partageant une surface additive, et une composition
XOR corrélée résolue par meet-in-the-middle. Les observations ne sont qu'un
filtre. Un candidat ne remplace la source qu'après
`prove_zero_without_p11(source - candidat)`.

`expected` est lu par l'exemple uniquement après la synthèse, pour mesurer le
coût atteint. Il n'est jamais injecté dans le générateur.

## Résultats

```console
cargo run --release -q -p rumba-core --features parse --example p11b_corpus
```

| Ligne | Entrée P7e | P11b | Cible | Candidats | Certificats | Temps |
|---:|---:|---:|---:|---:|---:|---:|
| 25 | 50 | **12** | 19 | 64 252 | 2/2 | 56,6 ms |
| 114 | 16 | **11** | 12 | 63 832 | 1/1 | 54,7 ms |
| 139 | 84 | **14** | 14 | 483 121 | 3/3 | 410,0 ms |
| 423 | 25 | **9** | 9 | 67 162 | 1/1 | 54,9 ms |

```text
p11b_cases=4
resolved=4
cost_reduced=4
certifications=7
time_ms=median:56.631,p95:409.988,max:409.988
expected_used_for_candidate_generation=false
```

Sur 25 et 114, la forme autonome est plus petite que la cible du corpus. Les
quatre cas atteignent ou dépassent leur coût cible et chaque résultat retenu
possède un certificat exact.

## Bilan P11 sur les seize bloqueurs

```text
cases=16
cost_reduced=16
resolved_at_or_below_target_cost=10
expected_used_for_candidate_generation=false
production_pipeline_changed=false
```

P11a réduit les douze cas avec multiplication et en résout six au coût cible.
P11b réduit et résout les quatre cas sans multiplication.

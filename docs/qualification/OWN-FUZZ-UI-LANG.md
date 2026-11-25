# Fuzz d'ownership étendu (RFC-UI-LANG-1, critère de sortie d'U1) — 2026-11-25

État des preuves : `TESTED`, Linux x86_64, `rustc 1.95.0`, sur le commit de
ce rapport. Ce n'est pas un `PROVED` : un fuzz ne trouve que ce que son
générateur sait produire.

## Ce qui est testé

[`crates/ravum-lang-jit/tests/ownership_fuzz.rs`](../../crates/ravum-lang-jit/tests/ownership_fuzz.rs)
assemble au hasard 2 à 6 instructions, **légales ou non**, parmi 30 modèles :
`for` sur `&v` / `&mut v` avec `break`, `continue`, `return` ; `match` sur
`&mut b[i]` (élément d'un `Vec` d'agrégats) ; vues `Map::get` / `get_mut` ;
parcours de `Map` ; clone structurel ; déplacements ; primitives de texte ;
écritures concurrentes à une vue ou à un parcours. Le vérificateur décide.

Pour chaque programme :
1. **sûreté** : accepté par Ravum ⇒ le Rust généré compile (le borrow
   checker de rustc est d'accord) ;
2. **parité** : accepté ⇒ la voie C l'abaisse, et rustc, l'interpréteur IR
   et le JIT impriment la même sortie et paniquent avec le même texte ;
3. **déterminisme** : deux `check` donnent le même verdict.

Un refus est permis (le vérificateur est prudent) ; le test exige qu'il y ait
au moins un programme accepté **et** au moins un refusé, pour ne jamais
passer à vide.

```bash
cargo test -p ravum-lang-jit --test ownership_fuzz                        # 3 × 12, en CI
cargo test -p ravum-lang-jit --test ownership_fuzz -- --ignored --nocapture  # 3 × 200
```

## Résultats

| Campagne | Programmes | Acceptés (tous vérifiés) | Refusés | Désaccords |
|---|---|---|---|---|
| rapide | 36 | 12 | 24 | 0 |
| longue | 600 | 201 | 399 | 0 |

Calibrage, chaque modèle seul : les 21 modèles légaux sont acceptés, les
9 modèles illégaux sont refusés avec le code attendu
(`E-OWN-BORROW-CONFLICT`, `E-OWN-USE-AFTER-MOVE`, `E-OWN-MOVE-OUT-OF-BORROW`,
`E-OWN-REF-ESCAPE`). Un seul refus va au-delà de Rust : `let y = x;` sur un
emprunt borné, que Rust accepterait comme réemprunt ; OWN-6 interdit
volontairement de stocker une référence (`E-OWN-REF-ESCAPE`).

## Limites

- Programmes d'une seule fonction `main` et de fonctions auxiliaires fixes ;
  pas de génériques, de closures (pas encore dans le langage) ni d'`async`.
- Le taux de refus (deux tiers) vient des modèles illégaux mélangés : un
  seul modèle illégal suffit à refuser le programme.
- La voie C n'est comparée que sur les programmes acceptés.

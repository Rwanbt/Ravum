# Balayage codegen de la stdlib — chaque surface doit survivre à `build`

> **Statut : garde d'exécution, pas une preuve de correction.** Le balayage
> construit un petit programme par surface documentée de la stdlib et exige qu'il
> passe `check`, `build` puis, quand c'est observable, `run`.

## 1. Pourquoi ce garde existe

Le corpus de conformance ne construit que les programmes qu'il connaît. Une
classe de défaut lui échappe donc entièrement : **un programme accepté par
`check` dont le Rust émis ne compile pas**.

Elle n'est pas théorique. `math::sqrt(2.0)` — l'exemple documenté de
`stdlib.md` §6.2 — passait `check` avec **0 erreur** et échouait à `build` :

```
error[E0689]: can't call method `sqrt` on ambiguous numeric type `{float}`
```

Le littéral `2.0` était émis sans suffixe, donc `{float}` pour `rustc`, et toute
méthode appelée dessus échouait. Aucun test ne couvrait cette forme parce
qu'aucun cas de conformance n'écrit `sqrt(<littéral non typé>)`.

## 2. Ce que le balayage couvre

`scripts/stdlib-codegen-sweep.ps1`, 16 programmes, chacun `check` → `build` →
`run` :

| Surface | Exemple |
|---|---|
| `math` libres | `sqrt`, `floor`/`ceil`/`round`, trigonométrie, `pow`/`min`/`max`, `abs` |
| méthodes `f64` | `abs().sqrt()` sur variable typée |
| méthodes entières | `i32::abs` |
| `String` | `from`, `trim`, `to_uppercase`, `lines().len()` |
| `Vec` | `new`, `push`, `len` |
| `Option` / `Result` | `Some`/`unwrap`, `parse_i64`/`unwrap_or` |
| conversions | `to_string` sur entier, `bool`, `char` |
| `fs` | `read_to_string(...).unwrap_or(String::new())` |
| tableaux | littéral, indexation, taille |

Le balayage vérifie aussi, indirectement, que le compilateur **refuse** ce qui
doit l'être : mes quatre premiers essais (écrits à la main) ont été rejetés par
`check` — `let _ = …` (motif joker non supporté), `fs::write` (nom inconnu),
`Result` utilisé comme instruction (`E-TYP-STATEMENT-VALUE`). Ces refus sont
corrects ; ce sont mes programmes qui étaient faux.

## 3. Exécution

```powershell
cargo build -p ravum-lang-cli
pwsh -File scripts/stdlib-codegen-sweep.ps1        # 16 PASS / 0 FAIL attendu
pwsh -File scripts/stdlib-codegen-sweep.ps1 -Keep  # conserve les paquets générés
```

Le script est portable (`ravum-lang` ou `ravum-lang.exe` selon l'OS) et tourne
dans le job CI **Linux qualification**.

## 4. Limites

- **16 surfaces sur ~60 méthodes documentées** : le balayage échantillonne, il
  n'énumère pas. Une méthode non listée peut toujours casser silencieusement.
- Il ne teste **pas** les combinaisons (génériques imbriqués, effets, FFI), ni le
  comportement sémantique — seulement « le Rust émis compile et le programme
  s'exécute ».
- Un `run` réussi n'est pas une preuve de correction : seul `stdout_contains`
  vérifie une valeur, et une seule par cas.
- Il ne remplace ni le corpus de conformance ni l'audit externe.

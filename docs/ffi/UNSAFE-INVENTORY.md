# Inventaire `unsafe` du langage Ravum Core 1.0 (D7, outillé le 2026-11-14)

> **Statut : inventaire factuel, `PENDING_HUMAN_APPROVAL` pour D7.** Ce document
> ne ratifie rien : il enregistre **tous** les sites `unsafe` des crates du
> langage et la façon dont le compilateur les borne. La ratification D7 et
> l'audit externe (D4/R13) restent des décisions humaines.
> Contrat de frontière : [`SAFETY.md`](SAFETY.md). Mesure de l'écart :
> [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) R5/D7.

## 1. Pourquoi un inventaire, et ce qu'il change

`SAFETY.md` §4 (programme D7, étage 1) demande que `unsafe` n'existe que dans des
modules audités et listés, avec une comparaison en CI. Jusqu'au 2026-11-14 cette
règle était une **intention** : rien, dans le compilateur, n'empêchait d'ajouter
un `unsafe` dans `sema` ou `codegen`.

Deux verrous sont désormais en place, et ils sont de nature différente :

1. **Le compilateur Rust** (`#![forbid(unsafe_code)]` / `#![deny(unsafe_code)]`)
   rend l'ajout d'un `unsafe` non listé **impossible à compiler** dans les
   crates concernés. C'est la seule garantie mécanique, et elle porte sur le
   code présent dans ces crates.
2. **`tools/unsafe_audit.py`** vérifie la *forme* de l'inventaire que le lint ne
   peut pas vérifier : les cinq racines portent l'attribut attendu, aucun
   `#![allow(unsafe_code)]` global n'existe, et les seuls
   `#[allow(unsafe_code)]` locaux sont les deux variantes du module de signaux
   du CLI. Le script tourne dans `gate-contracts.yml` et a ses propres tests.

## 2. Les sites, exhaustivement

| # | Site | Code | Pourquoi `unsafe` | Argument de sûreté |
|---|---|---|---|---|
| 1 | `crates/ravum-lang-cli/src/commands/watch.rs`, `mod signal` (Windows) | `unsafe extern "system" { fn SetConsoleCtrlHandler(...) }` | API Win32 sans crate externe | signature reproduite à l'identique (`HandlerRoutine = unsafe extern "system" fn(u32) -> i32`) ; installation best-effort |
| 2 | idem | `unsafe extern "system" fn handler(_control_type: u32) -> i32` | callback exigé par Win32 | ne fait que `STOP.store(true, SeqCst)` puis retourne `1` ; aucune allocation, aucun verrou, aucune panique |
| 3 | idem | `unsafe { SetConsoleCtrlHandler(Some(handler), 1) }` | appel de l'API | pointeur vers une fonction `extern "system"` de durée de vie statique ; valeur de retour ignorée (best-effort) |
| 4 | `crates/ravum-lang-cli/src/commands/watch.rs`, `mod signal` (Unix) | `unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; }` | `signal(2)` sans crate externe | seule `SIGINT` (2) est installée ; pointeur passé en `usize` selon l'ABI de la plateforme |
| 5 | idem | `unsafe extern "C" fn handler(_sig: i32)` | callback exigé par `signal(2)` | ne fait que `STOP.store(true, SeqCst)` ; travail async-signal-safe uniquement |
| 6 | idem | `unsafe { signal(2, handler as usize) }` | appel de l'API | pointeur de fonction valide ; résultat ignoré (best-effort) |

Aucun autre site `unsafe` n'existe dans `ravum-lang-syntax`, `ravum-lang-sema`,
`ravum-lang-codegen`, `ravum-lang-lsp` ni dans le reste de `ravum-lang-cli` : ces
quatre crates portent `#![forbid(unsafe_code)]`, le CLI porte
`#![deny(unsafe_code)]` et les deux `#[allow(unsafe_code)]` ci-dessus.

## 3. Ce que l'inventaire ne couvre pas

- **Le code généré.** `ravum-lang-codegen` produit un fichier Rust qui porte
  `#![forbid(unsafe_code)]` ; pour un programme avec `extern "C"`, l'en-tête
  passe à `#![deny(unsafe_code)]` et **un seul** `#[allow(unsafe_code)]`
  encadre le wrapper `rvm_ffi_wrap_<id>`. Ce n'est pas un site `unsafe` du
  compilateur, mais c'est un `unsafe` du **produit** : il est couvert par les
  tests backend (un seul `unsafe`, `link_name` correct) et par
  [`SAFETY.md`](SAFETY.md) §2.
- **La bibliothèque C appelée.** Elle n'est pas compilée par Ravum ; aucun
  inventaire ne peut la borner. C'est la phrase TCB de `SAFETY.md` §1.
- **Les dépendances.** L'inventaire porte sur les crates du langage, pas sur
  `serde`/`serde_json` ; leur `unsafe` éventuel n'est pas audité ici.
- **L'audit externe.** Un inventaire outillé n'est pas une revue humaine
  indépendante : D4/R13 restent `BLOCKED`.

## 4. Vérification

```
python tools/unsafe_audit.py
  unsafe inventory (D7): ...
  unsafe_audit: PASS — the code matches the audited inventory

python -m unittest discover -s tools/tests -p "test_unsafe_audit.py" -v
  7 tests, 0 failure
```

Les deux commandes tournent aussi en CI : le script dans `gate-contracts.yml`,
les tests dans `tools/tests`.

## 5. Mise à jour de l'inventaire

Tout nouveau `unsafe` exige, dans la même modification : (a) un argument de
sûreté écrit sur le site (`// SAFETY:`), (b) une ligne dans le tableau §2,
(c) l'ajout du fichier au bon compte dans `ALLOWED_LOCAL_ALLOWS` de
`tools/unsafe_audit.py`, (d) un test qui échoue si le site disparaît. Élargir
`ALLOWED_LOCAL_ALLOWS` sans revue revient à affaiblir D7 : le script échoue
délibérément dans ce cas.

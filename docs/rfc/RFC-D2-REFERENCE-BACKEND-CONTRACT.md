# RFC — D2 : contrat du backend de référence (voie B), mesuré et testable

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL`** · **Date : 2026-11-11**
> **Décision liée : D2** (`DECISIONS_REQUIRED.md`, `CORE-1.0-GATE-MATRIX.md` §4)
> **Ce document ne ratifie rien.** Il définit le contrat
> `source → frontend → CheckedProgram → émission → binaire`, mesure ce qui est
> vrai aujourd'hui, nomme ce qui manque, et propose les tests qui épinglent le
> contrat. Aucun code, aucune spec, aucun test existant n'est modifié ici.

## 1. Décision proposée (énoncé ratifiable)

**P2.1 — Backend de référence.** Pour Core 1.0, le backend de référence est la
**voie B** : le frontend Ravum produit un `CheckedProgram` ; l'émetteur produit
**un fichier Rust unique et autonome** ; `rustc` produit le binaire. La voie C
(backend natif direct) reste l'**objectif futur**, hors périmètre 1.0, avec son
propre critère de réouverture ([`RFC-BACKEND-NATIF-VOIE-C.md`](RFC-BACKEND-NATIF-VOIE-C.md)).

**P2.2 — La validation sémantique appartient à Ravum.** Aucun programme n'est
accepté parce que « Rust l'aurait accepté ». Le checker Ravum est l'autorité :
types, ownership, effets, résolution, exhaustivité. `rustc` est un **second
garde-fou de forme** sur le fichier émis, pas l'oracle sémantique. Inversement,
un refus de `rustc` sur un programme **accepté** par Ravum est un **défaut du
compilateur**, jamais un diagnostic utilisateur légitime (ABI-4.1).

**P2.3 — Pas d'IR multi-niveaux maintenant.** Aucune IR intermédiaire n'est
introduite avant démonstration d'un bénéfice mesuré (§7). L'émetteur travaille
directement sur le `CheckedProgram` (faits typés + résolutions + effets).

**P2.4 — Contrat testé, extensible.** Le contrat est épinglé par des tests qui
échouent quand il est violé (§6), et son extension (nouveau type, nouveau
builtin, nouvelle cible) passe par une procédure écrite plutôt que par un
abaissement ad hoc dans l'émetteur.

## 2. Environnement et provenance des mesures

| Élément | Valeur |
|---|---|
| SHA mesuré | `5d88ecb` (`feat/lang-core-1.0-rc`), export `git archive HEAD` |
| Arbre | copie temporaire hors dépôt (l'arbre partagé contenait, pendant la mesure, des modifications non commitées d'autres agents — **exclues** de ces chiffres) |
| Compilateur | `cargo build -p ravum-lang-cli --bin ravum-lang` dans la copie ; `ravum-lang 0.1.0 (edition core-1.0-draft.1)`, SHA-256 `4D5ED2EB…278686E5` |
| Toolchain | `rustc 1.95.0` (chemin réel `…\toolchains\1.95.0-x86_64-pc-windows-msvc\bin\rustc.exe`), Windows 11 x86-64 |
| Machine | i7-13620H, 10 cœurs / 16 threads, 16 Go |
| Charge | **79–100 % CPU, 3–5 processus `cargo` d'autres agents** pendant les mesures. Les temps de paroi sont des **majorants** ; les temps CPU sont la mesure la plus robuste. Une baseline « machine au repos » n'a pas pu être produite : `NOT_RUN` (commande en §3.3). |
| Sorties brutes | `target/bench-cpu-py2.json`, `target/conformance-head.json`, `target/coerce-matrix-head-5d88ecb.json` (fichiers temporaires, non versionnés) |

## 3. Métriques mesurées

### 3.1 Protocole

```
# 1. export propre du HEAD et binaire
git archive HEAD -o head.tar && tar -xf head.tar -C <tmp>
cd <tmp> && cargo build -p ravum-lang-cli --bin ravum-lang

# 2. temps de paroi + mémoire échantillonnée (script officiel, 3 runs)
pwsh -File scripts/lang-bench.ps1 -Runs 3 -Sizes 100,500,2000

# 3. CPU, rustc réel, tailles d'artefacts (script temporaire, 5 runs check / 3 runs build)
python <copie temporaire>/bench-cpu.py <tmp>/target/debug/ravum-lang.exe out.json
```

Les programmes sont générés par `lang-bench.ps1` : `n` fonctions, 11 lignes de
Ravum par fonction. `n=100` → 1 106 lignes, `n=500` → 5 506, `n=2000` → 22 006.

### 3.2 Tableau des métriques

| Métrique | n=100 | n=500 | n=2000 | Méthode |
|---|---|---|---|---|
| **check** — paroi médiane | **224,9 ms** | **375,3 ms** | **1 058,4 ms** | `lang-bench.ps1`, 3 runs (charge 100 %) |
| check — paroi médiane (2ᵉ passe) | 77,3 ms | 158,1 ms | 704,6 ms | `bench-cpu.py`, 5 runs (charge 79 %) |
| **check — CPU médian** | **31,2 ms** | **140,6 ms** | **656,2 ms** | `GetProcessTimes`, même processus |
| **build — paroi médiane** | **2 027,1 ms** | **2 757,8 ms** | **6 109,0 ms** | `lang-bench.ps1`, 3 runs (charge 100 %) |
| **build — CPU parent médian** | **46,9 ms** | **203,1 ms** | **796,9 ms** | `bench-cpu.py` ; **hors** enfant `rustc` |
| **rustc — paroi médiane** | **557,4 ms** | **1 165,6 ms** | **4 776,4 ms** | invocation directe, mêmes drapeaux que `pipeline.rs` |
| **rustc — CPU médian** | **421,9 ms** | **937,5 ms** | **3 390,6 ms** | binaire *réel* du toolchain, pas le shim rustup |
| part de `rustc` dans `build` (paroi) | 76,7 % | 83,0 % | 87,4 % | dérivé |
| **coût de génération — CPU dérivé** | **15,7 ms** | **62,5 ms** | **140,7 ms** | dérivé = CPU build parent − CPU check |
| **RSS pic échantillonné — check** | 9,8 Mo | 21,0 Mo | 61,6 Mo | échantillonnage 5 ms, **borne inférieure** |
| **RSS pic échantillonné — build** | 10,9 Mo | 22,6 Mo | 65,1 Mo | idem ; **hors** enfant `rustc` (pic 76,2 / 94,6 / 167,7 Mo) |
| **Rust généré** | 58 511 o | 268 245 o | 1 068 750 o | taille du fichier `.ravum/gen/main.rs` |
| **binaire produit** | 184 832 o | 345 600 o | 947 200 o | taille du `.exe` (opt-level=0, debuginfo=2) |
| **edit → binaire** | **= build** | **= build** | **= build** | aucune compilation incrémentale : toute édition déclenche un rebuild complet ; `watch` ne fait que rejouer `build` |
| **génération — coût direct** | **NOT_MEASURED** | **NOT_MEASURED** | **NOT_MEASURED** | aucun point d'instrumentation CLI ; voir §3.3 |

### 3.3 Ce qui n'est pas mesuré, et la commande qui le produirait

| Métrique | État | Commande qui la produirait |
|---|---|---|
| Baseline machine au repos (paroi) | `NOT_RUN` — machine saturée par d'autres agents pendant toute la session | `pwsh -File scripts/lang-bench.ps1 -Runs 5 -Sizes 100,500,2000` sur une machine sans autre build, et **consigner la charge** |
| Coût d'émission seul (sans `rustc`) | `NOT_MEASURED` | un harnais Rust appelant `ravum_lang_codegen::generate(&checked)` sur le `CheckedProgram` déjà construit, chronométré ; ou une option `--timings` dans `pipeline.rs` (inexistante) |
| Mémoire de crête réelle (pas échantillonnée) | `NOT_MEASURED` | `GetProcessMemoryInfo().PeakWorkingSetSize` après sortie, ou `time -v` sous Linux ; l'échantillonnage 5 ms sous-estime |
| CPU total d'un `build` incluant l'enfant `rustc` | `NOT_MEASURED` (seulement décomposé) | Job object Windows avec comptabilité enfants, ou `perf stat`/`/usr/bin/time -v` sous Linux |
| Paroi de `watch` sur édition réelle (boucle complète) | `NOT_RUN` | `ravum-lang watch --run` sur un projet, éditer, mesurer l'écart édition→binaire relancé |

**Lecture honnête :** le poste de coût de `build` est **`rustc`** (77–87 % de la
paroi mesurée), pas le frontend ni l'émetteur. `check` coûte 31–656 ms de CPU
selon la taille ; l'émission coûte 16–141 ms de CPU. Ces chiffres sont des
observations sous charge, pas des budgets ratifiés.

## 4. Ce qui est déjà vrai et prouvé (avec le fichier/test qui le prouve)

| Propriété | Preuve |
|---|---|
| La chaîne `check → build → run` fonctionne réellement | `examples/hello`, `fib`, `ffi-abs`, `dsp-gain-iir` exécutés sur le binaire `5d88ecb`, exit 0 ; `docs/roadmap/CORE-1.0-GATE-MATRIX.md` T1/T5 |
| Le corpus exécute la chaîne descendante | `cargo run -p ravum-conformance -- run --json target/conformance-head.json` → **386 cas, 385 PASS / 0 FAIL / 1 SKIP** ; 53 `run-pass` + 6 `run-fail` passent par `rustc` |
| Le code émis est un **fichier Rust unique, autonome, sans dépendance hors `std`** (ABI-2.3) | `pipeline.rs` (une seule sortie `generated`), `emit.rs`, tests `ravum-lang-codegen` (29 unitaires + 21 audits `backend_audit.rs`) |
| Le code émis est **safe par défaut** (`#![forbid(unsafe_code)]`) | `backend_audit::audit_generated_file_is_safe_no_unsafe_constructs`, `emit.rs:976-1000` |
| Le seul `unsafe` généré est le wrapper FFI, un par `extern`, sous `deny(unsafe_code)` | `backend_audit::audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol`, `emit.rs:1185-1256` |
| L'émetteur refuse une signature ABI non sûre même depuis un `CheckedProgram` forgé | `emit.rs:100` (`ast_ffi_safe`), audit finding F0b |
| Le déterminisme de l'émission est testé | tests `emit.rs` (mêmes entrées → même texte), `backend_audit` déterminisme |
| Les marqueurs `// rv: <fichier>:<ligne>` relient Rust émis et source Ravum (ABI-2.4) | `emit.rs` (`marker`), test d'audit backend |
| Le binaire est reproductible **selon la procédure mesurée** | `docs/qualification/REPRODUCIBILITY.md` (deux racines, deux `target/`, drapeaux documentés) — mesure antérieure, non rejouée ici |
| Les cas `compile-fail` comparent des codes exacts (pas des messages) | `tools/conformance` + `spec/language/diagnostics.md` DIAG-9.4 |

## 5. Ce qui manque (trous du contrat, mesurés)

### 5.1 Le contrat `CheckedProgram → binaire` n'est pas vérifié pour 79 cas acceptés

Le corpus comporte, sur `5d88ecb` (compté dans l'export HEAD, pas dans l'arbre
partagé) :

| Mode | Cas | Passe par `rustc` ? |
|---|---|---|
| `compile-pass` | **79** | **non** — le harnais ne compile que `run-pass`/`run-fail` |
| `run-pass` | 53 | oui |
| `run-fail` | 6 | oui |
| `compile-fail` | 232 | non (refus attendu au `check`) |
| `cli` | 14 | selon la commande |
| `fmt-idempotent` | 2 | non |

**20,5 % des cas (79/386) sont acceptés sans jamais être abaissés vers un
binaire.** `--verify-build` n'existe pas sur `HEAD`. C'est la cause structurelle
pour laquelle un programme « accepté par `check` puis refusé par `rustc` » peut
survivre : le corpus ne le voit pas.

### 5.2 Des programmes acceptés ne produisent aucun binaire (mesuré sur `5d88ecb`)

Sondes indépendantes (programmes complets, `target/coerce-matrix-head-5d88ecb.json`) :

| Programme accepté par `check` | `build` | Erreur rustc |
|---|---|---|
| `takes(owned)` — `String` possédé vers paramètre `&str` | **échec** | `E0308` (expected `&str`, found `String`) |
| `takes(line.trim())` — même forme via `trim()` (STD-9.1) | **échec** | `E0308` |
| `let t: (&str, i32) = (owned, 1);` | **échec** | `E0308` |
| `let xs: [&str; 1] = [owned];` | **échec** | `E0308` |
| `P { s: owned }` où `struct P { s: &str }` | **échec** | `E0308` / `E0106` |
| `struct P { s: &str }` + `P { s: "x" }` (littéral, OWN-3.3) | **échec** | `E0106` (champ `&str` sans lifetime) |

C'est la violation directe de P2.2 (le checker accepte, `rustc` rejette → défaut
du compilateur). Le détail de la coercition est traité dans
[`RFC-D-COERCE-1-MATRIX.md`](RFC-D-COERCE-1-MATRIX.md) ; le champ `&str` est un
défaut de **lowering** indépendant.

### 5.3 ABI-4.1 n'est pas vérifié indépendamment

`ABI-4.1` exige que l'entrée du backend soit un `CheckedProgram` **valide** :
aucun identifiant non résolu, aucun type manquant, aucun effet non calculé. Or
`generate(&CheckedProgram)` reçoit la structure telle quelle ; seules quelques
défenses ponctuelles existent (`ast_ffi_safe`, faits de résolution manquants →
`CodegenError`). Il n'y a **aucune validation exhaustive** des invariants
d'entrée, et un `CheckedProgram` forgé à la main peut encore produire du Rust
invalide hors des cas couverts par les audits.

### 5.4 Le coût d'émission n'est pas instrumenté

`--emit-rust FILE` écrit le fichier mais exécute aussi `rustc` ; il n'existe
aucun point de mesure séparé du frontend, de l'émission et de `rustc`. Le
chiffre « coût de génération » de §3.2 est **dérivé**, pas mesuré.

### 5.5 Aucune compilation incrémentale

`edit → binaire` = `build` complet. Le `watch` rejoue `build` ; il n'y a ni
cache d'émission ni recompilation ciblée. Ce n'est pas un défaut du contrat de
backend, mais cela signifie que « P0-A feedback continu » n'est pas mesuré au
niveau du backend aujourd'hui.

## 6. Contrat de lowering testé et extensible (conception proposée)

### 6.1 Interface (décrite, pas implémentée ici)

```
frontend(source) -> Result<CheckedProgram, Vec<Diagnostic>>     // phases : lex, parse, resolve, typeck, own, effects
CheckedProgram {
    program: ast::Program,             // arbre unique, NodeId stables
    facts: Facts {                     // ce que l'émetteur a le droit de supposer
        type_of:      Map<ExprId, Type>,       // aucun Type::Infer, aucun Type::Param non substitué
        resolutions:  Map<NodeId, Resolution>, // aucun Resolution::Error accepté
        calls:        Map<ExprId, CallTarget>, // aucun CallTarget manquant pour un appel
        effects:      Map<FnId, EffectSet>,    // calculé pour toute fonction du programme
        structs/enums/methods/item_paths/extern_symbols/…
    },
    profile: core | pure | rt,
}
emission(CheckedProgram) -> Result<String, CodegenError>       // un seul fichier Rust autonome
build(String) -> Result<PathBuf, BuildError>                    // rustc, drapeaux documentés
```

**Post-conditions du contrat** (à tester, pas à croire) :

1. **C1 — totalité des types** : aucun `Type::Infer`/`Type::Param` non substitué
   n'atteint l'émetteur ; sinon `CodegenError`, jamais un Rust invalide.
2. **C2 — totalité des résolutions** : tout chemin d'item, toute méthode, tout
   appel porte un fait de résolution non `Error`.
3. **C3 — totalité des effets** : l'ensemble d'effets est défini pour toute
   fonction atteignable ; un effet absent est un `CodegenError`.
4. **C4 — refus fail-closed** : si l'émetteur ne sait pas abaisser une
   construction, il renvoie `CodegenError` (diagnostic interne), il n'émet pas
   de Rust « au mieux ».
5. **C5 — compilabilité** : pour **tout** programme `compile-pass` accepté, le
   Rust émis compile avec les drapeaux du produit. C'est la propriété que le
   corpus ne vérifie pas aujourd'hui.
6. **C6 — déterminisme** : mêmes entrées → même texte, même binaire
   (modulo chemins, cf. `--remap-path-prefix`).

### 6.2 Tests qui épinglent le contrat

| Fichier (proposé) | Ce qu'il asserte |
|---|---|
| `crates/ravum-lang-codegen/tests/lowering_contract.rs` | Pour un jeu de programmes acceptés (y compris les 6 formes de §5.2), `generate` + `rustc` réels : soit un binaire, soit un `CodegenError` explicite — **jamais** une erreur `E0xxx` de rustc. Rejoue les sondes §5.2 comme tests de non-régression. |
| `crates/ravum-lang-sema/tests/checked_program_invariants.rs` | C1–C3 sur des programmes variés : aucun `Type::Infer`/`Param` dans `facts.type_of`, aucun `Resolution::Error`, effets définis pour toute fonction ; compte les violations et échoue avec le détail. |
| `crates/ravum-lang-codegen/tests/lowering_determinism.rs` | C6 : deux émissions successives identiques ; deux compilations → binaires identiques avec les drapeaux de repro. |
| `tools/conformance` — option `--verify-build` | **C5 généralisé** : chaque `compile-pass` est aussi compilé (pas exécuté). Coût estimé sur la mesure §3 : 79 cas × ~0,5 s ≈ 40 s sur une machine au repos — ordre de grandeur, à re-mesurer. Sans cette option, C5 reste non testé. |
| `conformance/cases/types/*` | Les cas de refus de §5.2 (coercition possédée) et le cas de compilation du champ `&str` (ou son refus explicite si le champ reste non supporté). |
| `crates/ravum-lang-codegen/tests/backend_audit.rs` (existant) | Étendre l'audit F0b : `CheckedProgram` forgé avec type non substitué / résolution `Error` → `CodegenError`, pas de Rust invalide. |

### 6.3 Procédure d'extension (pour qu'un nouveau type/builtin/cible ne casse pas le contrat)

1. Ajouter la règle **d'abord** dans `spec/language/` (sinon le cas de
   conformance n'a pas d'oracle) ;
2. ajouter le cas de conformance (acceptation **et** refus) ;
3. implémenter dans le checker **et** l'émetteur dans la même PR ;
4. ajouter le test C5 correspondant (compilation réelle) ;
5. si l'abaissement Rust est impossible, l'échec doit être un `CodegenError`
   explicite et le cas doit être `compile-fail` documenté — pas un
   contournement silencieux.

## 7. Une IR minimale est-elle justifiée ? (évaluation explicite)

**Réponse : non, pas maintenant.** Seul un bénéfice **mesuré** la justifierait.
Ce qui la démontrerait :

| Signal qui justifierait une IR | Mesure actuelle | Verdict |
|---|---|---|
| Un ensemble de MUST inexprimables en Rust safe sans workaround | 1 cas documenté (motifs flottants → garde Rust, D-PAT-1) ; insuffisant à lui seul | pas démontré |
| Un second backend engagé, avec oracle de parité | aucun ; voie C `NOT_IMPLEMENTED`, RFC séparée | pas démontré |
| Le coût d'émission domine le build | **faux** : `rustc` = 77–87 % de la paroi, émission CPU 16–141 ms | pas démontré |
| Des bugs de lowering récurrents dus à l'impédance Rust | **observé** (les 6 formes de §5.2), mais corrigeables sans IR (valider les faits, ajouter les tests C5) | insuffisant |
| Un besoin de cibles multiples partageant une représentation vérifiée | 2 cibles (hôte, `wasm32-wasip1`) via le même émetteur Rust ; aucune duplication d'émetteur | pas démontré |

**Critère de réouverture écrit :** une IR minimale devient justifiée si (a) un
deuxième backend est décidé **et** (b) un oracle de parité automatique existe
(même programme, même sortie, mêmes codes) **et** (c) au moins trois formes MUST
ne peuvent pas être abaissées vers Rust safe sans workaround documenté. Sans les
trois, une IR ajouterait une couche non prouvée entre la spec et le binaire.

## 8. Recommandation (non contraignante)

1. Ratifier **P2.1–P2.4** : voie B pour Core 1.0, validation sémantique côté
   Ravum, pas d'IR, contrat testé.
2. **Avant** toute promotion vers `main` : ajouter C5 (`--verify-build` sur les
   `compile-pass`) et les tests C1–C4. Les 6 formes de §5.2 doivent devenir des
   cas, pas rester des sondes.
3. Corriger les deux défauts mesurés : coercition possédée
   ([`RFC-D-COERCE-1-MATRIX.md`](RFC-D-COERCE-1-MATRIX.md)) et champ `&str`
   (E0106).
4. Instrumenter le coût d'émission (`--timings` ou harnais) avant de citer un
   chiffre de « génération » dans une release.
5. Rejouer `lang-bench` sur une machine au repos et **publier la charge** avec
   les chiffres ; les valeurs de §3.2 sont des majorants sous charge.

## 9. Ce que cette RFC ne fait pas

- Elle ne ratifie pas D2, ne crée aucun ADR, ne modifie aucun code, spec, test
  ni workflow.
- Elle ne présente pas les chiffres sous charge comme une baseline de
  performance ; elle ne mesure pas la voie C (elle n'existe pas).
- Elle ne déclare pas le contrat « testé » : elle propose les tests qui le
  testeraient, et mesure aujourd'hui qu'ils manquent.

# RFC — D-COERCE-1 : la coercition `String → &str`, matrice mesurée et texte normatif proposé

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL`** · **Date : 2026-11-11**
> **Décision liée : D-COERCE-1** (`docs/roadmap/CORE-1.0-GATE-MATRIX.md` §4,
> `docs/qualification/T2-FINDINGS.md` §19.2 et §20.3)
> **Ce document ne ratifie rien et ne change aucun comportement.** Il ne modifie
> ni `spec/language/`, ni `crates/`, ni `conformance/`. Aucun test n'a été
> supprimé ni assoupli pour l'écrire.

## 1. Problème

`spec/language/types.md` énonce deux MUST :

- **TYP-13.1** — « Il n'existe **que deux** ajustements implicites, tous deux en
  position d'argument ou de récepteur : (a) `&[T; N] → &[T]`, (b) `&String →
  &str`. ([…]) Aucune autre conversion n'est implicite ».
- **TYP-8.5** — « La conversion `String → &str` est explicite (`.as_str()`). »

L'implémentation (`crates/ravum-lang-sema/src/typeck.rs`, `fn coerce`) admet en
plus un `String` **possédé** là où un `&str` est attendu, et applique la
coercition à des positions que TYP-13.1 ne liste pas. Le comportement est
conservé depuis la passe 5 pour ne pas casser `examples/stats` et
`conformance/cases/run/stats` : `line.trim()` rend un `String` (STD-9.1, MUST)
et cet `String` est passé à `string::parse_f64(&str)`.

La question de cette RFC n'est pas « qui a raison » mais : **quel comportement
le compilateur a-t-il réellement, quelle est la conséquence observable, et que
doit dire la spécification ?** Le comportement a été mesuré sur un binaire
construit depuis un export propre de `HEAD` (`5d88ecb`), pas sur
`target/debug/` : un autre agent reconstruisait ce répertoire pendant la mesure
(voir §2.1).

## 2. Méthode et provenance des mesures

### 2.1 Environnement déclaré

| Élément | Valeur |
|---|---|
| SHA mesuré | `5d88ecb` (branche `feat/lang-core-1.0-rc`), export `git archive HEAD` |
| Arbre de mesure | copie temporaire hors dépôt (`%TEMP%\ravum-head-5d88ecb`), **pas** l'arbre de travail partagé |
| Compilateur mesuré | `cargo build -p ravum-lang-cli --bin ravum-lang` dans cette copie → `target/debug/ravum-lang.exe`, SHA-256 `4D5ED2EB…278686E5`, `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| Toolchain | `rustc 1.95.0`, Windows 11 x86-64 |
| Charge machine | i7-13620H (10 cœurs / 16 threads), **100 % d'occupation**, 5 processus `cargo` d'autres agents au moment de la mesure ; mesurer le binaire de l'arbre partagé aurait été non reproductible (il a été reconstruit à 11:51 pendant la mesure) |
| Script de sonde | `target/coerce-matrix.py` (fichier temporaire, non versionné ; les programmes sont reproduits en annexe A) |
| Sortie brute | `target/coerce-matrix-head-5d88ecb.json` |

L'arbre de travail partagé contenait, pendant cette mesure, des modifications
non commitées d'un autre agent (dont `crates/ravum-lang-codegen/src/emit.rs`).
**Aucune de ces modifications n'est incluse dans les chiffres ci-dessous** ; elles
ne sont ni évaluées ni attribuées ici. La mesure est donc celle de `HEAD`
`5d88ecb`, et c'est la seule prétention de ce document.

### 2.2 Ce que chaque sonde fait

Pour chaque programme : création d'un package minimal (`ravum.toml` +
`src/main.rv`) dans `%TEMP%`, puis

```
target/debug/ravum-lang.exe check <dir> --json   → exit + codes
target/debug/ravum-lang.exe build <dir>          → exit (seulement si check = 0)
```

`check` = exit 0 / 1 avec la liste exacte des codes du JSON. `build` = 0 avec
binaire, ou 1 avec l'erreur `rustc` en clair. Un programme **accepté par `check`
puis refusé par `build`** est un défaut de contrat, pas une divergence de goût :
le backend reçoit un `CheckedProgram` valide et produit du Rust qui ne compile
pas (ABI-4.1 : « l'entrée du backend est un `CheckedProgram` valide »).

## 3. Matrice mesurée (HEAD `5d88ecb`, rustc 1.95.0)

Légende : `ACCEPT` = `check` exit 0 ; `REFUSE` = `check` exit 1 avec les codes
listés ; `BUILD` = sort du `build` quand `check` a accepté.

| # | Programme (position) | TYP-13.1 / TYP-8.5 exige | `check` | codes | `build` |
|---|---|---|---|---|---|
| 1 | `takes(&a)` — argument `&String` | accepté · (b) | **ACCEPT** | — | **BUILD 0** |
| 2 | `takes(b)` — argument `String` possédé | refusé (TYP-8.5 : explicite) | **ACCEPT** | — | **BUILD 1 — E0308** |
| 3 | `takes(line.trim())` — `trim()` rend `String` (STD-9.1) | refusé (TYP-8.5) | **ACCEPT** | — | **BUILD 1 — E0308** |
| 4 | `let r: &str = &a;` — annotation locale | refusé (positions : argument/récepteur) | REFUSE | `E-TYP-MISMATCH` | — |
| 5 | `let r: &str = b;` | refusé | REFUSE | `E-TYP-MISMATCH` | — |
| 6 | `fn view(s: &String) -> &str { return s; }` | refusé (TYP-8.5) | REFUSE | `E-TYP-MISMATCH`, `E-TYP-RETURN` | — |
| 7 | `fn view(s: &String) -> &str { return s.as_str(); }` | accepté (conversion explicite) | **REFUSE** | `E-OWN-REF-ESCAPE` | — |
| 8 | `let t: (&str, i32) = (b, 1);` — élément de tuple | hors TYP-13.1 | **ACCEPT** | — | **BUILD 1 — E0308** |
| 9 | `let xs: [&str; 1] = [b];` — élément de tableau | hors TYP-13.1 | **ACCEPT** | — | **BUILD 1 — E0308** |
| 10 | `P { s: b }` — champ de struct `&str` | hors TYP-13.1 (et OWN-3.3) | **ACCEPT** | — | **BUILD 1 — E0106** |
| 11 | `takes(b.as_str())` — conversion explicite | accepté | ACCEPT | — | **BUILD 0** |
| 12 | `fn takes(s: &mut str)` + `&mut b` | refusé (G-5.4/TYP-6.5) | REFUSE | `E-RES-UNKNOWN` | — |
| 13 | `takes(&a)` avec `a: [i32; 3]` — (a) `&[T; N] → &[T]` | accepté · (a) | ACCEPT | — | **BUILD 0** |
| 14 | `takes(&v)` avec `v: Vec<i32>` | refusé (TYP-13.1) | REFUSE | `E-TYP-MISMATCH` | — |

Constats mesurés :

1. **Cinq programmes acceptés par `check` ne produisent aucun binaire.** Quatre
   (`#2`, `#3`, `#8`, `#9`) échouent en `E0308` : le Rust émis passe un
   `String` là où la signature Rust attend `&str`, sans insérer d'emprunt. Le
   rustc dit exactement `expected &str, found String` et propose `&b`.
   Le cas `#3` montre que le motif du corpus `run/stats` casse dès que l'appelé
   est une **fonction Ravum** au lieu d'un builtin — le builtin, lui, est abaissé
   par `emit.rs` en `(&(trimmed))` (vérifié dans le Rust généré de
   `examples/stats`).
2. Le cas `#10` échoue en `E0106 missing lifetime specifier` : le champ `&str`
   est abaissé en `&str` dans une struct Rust, ce qui est invalide même avec un
   littéral. C'est un **défaut de lowering indépendant** de la coercition :
   `struct P { s: &str }` + `P { s: "x" }` (spec OWN-3.3, `&'static str` admis)
   est accepté par `check` et ne compile pas. Il est signalé ici parce que la
   même sonde l'a trouvé, mais il appartient au contrat de lowering (P1-A).
3. Les positions **hors argument/récepteur** ne sont pas toutes refusées : le
   tuple et le tableau acceptent, l'annotation locale et le retour refusent. La
   règle n'est donc ni « TYP-13.1 » ni « conversion partout » : c'est un
   ensemble de trois sites d'appel (`check_arg`, élément de tuple, élément de
   tableau) plus le champ de struct via `check_arg`, dont **aucun** n'a de
   contrepartie d'abaissement.
4. Le cas `#7` révèle une **limite indépendante** : même la conversion explicite
   `.as_str()` ne peut pas être **retournée** (OWN-3.3/OWN-3.15 : un prêt de
   paramètre ne s'échappe pas). `&str` en retour n'est donc constructible que
   depuis un littéral, pas depuis un `String` possédé. Ce n'est pas un défaut de
   D-COERCE-1, mais cela signifie que « migrer vers `.as_str()` » ne s'applique
   qu'aux **vues utilisées dans l'instruction**, pas aux valeurs stockées ou
   retournées — la proposition §6 en tient compte.

### 3.1 Ce que le corpus prouve déjà — et ce qu'il ne prouve pas

| Fait | Mesure |
|---|---|
| Cas de corpus dédié à la vue explicite | `conformance/cases/types/str-param-accepts-as-str-pass` utilise `t.as_str()` et **passe** |
| Cas de corpus qui dépend du **possédé** accepté | `1` : `conformance/cases/run/stats` (`string::parse_f64(trimmed)`), cité par F-STD-017 |
| Exemple qui en dépend | `1` : `examples/stats`, même instruction |
| Test unitaire qui documente la divergence | `1` : `crates/ravum-lang-sema/src/check.rs::string_by_value_is_coerced_to_str` (commentaire « KNOWN DIVERGENCE », pas une conformité revendiquée) |
| Cas de corpus couvrant l'acceptation **puis** l'échec de `build` | **0** — aucun cas n'exerce la coercition possédée vers une fonction Ravum ; c'est pourquoi le défaut a survécu |
| Cas dépendant de la coercition dans un **tuple**, un **tableau** ou un **champ** | **0** mesuré (aucun cas du corpus n'utilise ces formes ; les quatre programmes `#2`, `#3`, `#8`, `#9`, `#10` sont des sondes neuves) |

**Impact quantifié, avec sa limite :** retirer l'arm possédé casserait
aujourd'hui **2 programmes** (`examples/stats`, `conformance/cases/run/stats`) et
rendrait faux **1 test unitaire** qui documente le comportement. Le nombre exact
de cas de corpus qui échoueraient *après migration* n'est **pas mesuré** : il
exige de modifier `typeck.rs` (interdit dans cette passe). Commande qui le
produirait, sur une branche jetable :

```
# dans une copie de travail, supprimer l'arm (Type::Str, Type::String) de `fn coerce`,
# puis :
cargo run -p ravum-conformance -- run --json target/conformance-coerce-off.json
python tools/triage_conformance.py target/conformance-coerce-off.json
```

## 4. Texte normatif proposé (PROPOSED — non ratifié)

Deux variantes, exclusives. **Aucune n'est appliquée.**

### Option A — l'implémentation suit la spécification (recommandée)

Remplacer le corps de TYP-13.1 (b) par un texte qui nomme l'exclusion, et
ajouter une phrase à TYP-8.5 :

> **TYP-13.1 (b) — PROPOSED.** `&String → &str` est le seul ajustement de texte
> admis, en position d'argument ou de récepteur. Un `String` **possédé** n'est
> **pas** ajusté : `takes(owned)` est `E-TYP-MISMATCH`, y compris quand la valeur
> provient de `trim()` (STD-9.1). La forme acceptée est `takes(owned.as_str())`
> ou `takes(&owned)`.
>
> **TYP-8.5 — PROPOSED (ajout).** La vue `&str` d'un `String` possédé s'obtient
> par `.as_str()` ; elle ne peut être ni stockée dans une liaison, ni retournée,
> ni placée dans un champ, un tuple ou une collection (OWN-3.3, OWN-3.15). Elle
> n'est utilisable que dans l'instruction qui la crée.

Conséquences d'implémentation, toutes confinées au frontend et aux tests :

1. supprimer l'arm `(Type::Str, Type::String)` de `fn coerce` ;
2. supprimer les trois sites de coercition hors TYP-13.1 (élément de tuple,
   élément de tableau, champ de struct via `check_arg`) **ou** les documenter
   comme positions admises — mais alors il faut aussi les abaisser, ce qu'aucun
   code ne fait aujourd'hui (mesure `#8`, `#9`, `#10`) ;
3. migrer `examples/stats` et `conformance/cases/run/stats` :
   `string::parse_f64(trimmed.as_str())?` ;
4. remplacer `string_by_value_is_coerced_to_str` par un test de **refus**
   (même nom ou nom explicite `string_by_value_is_rejected_as_str`), qui asserte
   `E-TYP-MISMATCH` via le vrai checker ;
5. ajouter les cas listés en §6.

### Option B — la spécification admet le possédé (décision de langage)

Si le responsable veut conserver `takes(owned)`, alors TYP-13.1 doit l'écrire
(ex. « un `String` possédé est emprunté implicitement pour la durée de
l'instruction ») **et** le backend doit insérer l'emprunt, sinon le défaut
« `check` accepte / `build` échoue » reste. Coût : un changement de sémantique
(la valeur est-elle déplacée ou empruntée ?), une règle d'abaissement par site,
et la mise à jour de TYP-8.5. Risque : la règle devient une coercition
« possédé → emprunt » qui n'existe nulle part ailleurs dans le langage et
contredit OWN-3.2 (un prêt ne survit pas à l'instruction) par exception.

### Recommandation (non contraignante)

**Option A.** Trois raisons mesurées :

- elle rétablit la propriété « `check` accepte ⇒ `build` produit un binaire »
  pour les quatre formes fautives ;
- elle n'exige qu'une ligne de migration dans `examples/stats` et
  `run/stats` (`.as_str()`), forme déjà verrouillée par
  `types/str-param-accepts-as-str-pass` ;
- elle ne crée aucune sémantique nouvelle : `&String → &str` reste admis et
  compilé par la déréférencement Rust, déjà couvert.

Le point 2 (sites hors TYP-13.1) doit être tranché **en même temps** : garder la
coercition dans un tuple sans l'abaisser laisse un programme accepté qui ne
compile pas, quel que soit le sort de l'argument.

## 5. Migration de `examples/stats` (proposée, non appliquée)

```diff
     let mut count = 0;
     for line in lines {
         let trimmed = line.trim();
         if trimmed.is_empty() {
             continue;
         }
-        let value = string::parse_f64(trimmed)?;
+        let value = string::parse_f64(trimmed.as_str())?;
         sum += value;
         count += 1;
     }
```

La même modification s'applique à `conformance/cases/run/stats/src/main.rv`.
Vérification attendue après migration, avec le binaire reconstruit :

```
target/debug/ravum-lang.exe check conformance/cases/run/stats --json   # exit 0
target/debug/ravum-lang.exe build conformance/cases/run/stats          # exit 0
target/debug/ravum-lang.exe run   conformance/cases/run/stats -- sample.txt  # 2.5
```

`examples/stats` est aussi dans le smoke (`scripts/lang-smoke.ps1`) : la
migration doit laisser **31 PASS / 0 FAIL** (mesure antérieure publiée, non
rejouée ici).

## 6. Tests requis (à écrire dans une passe ultérieure)

| Fichier de cas (proposé) | Mode | Ce qu'il asserte |
|---|---|---|
| `conformance/cases/types/string-by-value-arg-rejected` | `compile-fail` | `takes(owned)` → `E-TYP-MISMATCH` (aujourd'hui accepté : `#2`) |
| `conformance/cases/types/string-by-value-tuple-rejected` | `compile-fail` | `(owned, 1)` attendu `(&str, i32)` → `E-TYP-MISMATCH` (`#8`) |
| `conformance/cases/types/string-by-value-array-rejected` | `compile-fail` | `[owned]` attendu `[&str; 1]` → `E-TYP-MISMATCH` (`#9`) |
| `conformance/cases/types/string-by-value-field-rejected` | `compile-fail` | `P { s: owned }` → `E-TYP-MISMATCH` (`#10` ; le champ est déjà non constructible pour une autre raison, cf. #10/E0106) |
| `conformance/cases/run/string-view-as-str-instruction` | `run-pass` | `takes(owned.as_str())` **exécuté**, valeur attendue (verrouille la forme de migration) |
| `conformance/cases/types/str-param-accepts-as-str-pass` | existant | reste `compile-pass` (la vue explicite est admise) |
| `crates/ravum-lang-sema/tests/checker_holes.rs` | test unitaire | refus du possédé dans les 4 positions, avec `E-TYP-MISMATCH` exact |

Le contrat de lowering correspondant (P1-A) doit aussi être piné :
**« tout programme `compile-pass` doit produire un binaire »** — c'est
précisément ce que les cas `#2`, `#3`, `#8`, `#9`, `#10` violent, et aucun cas
du corpus ne le vérifie (les `compile-pass` ne compilent pas réellement ; seuls
`run-pass` exercent `rustc`).

## 7. Risques et contradictions ouvertes

1. **Contradiction interne de la spec, non résolue par cette RFC** : TYP-13.1
   exige que l'ajustement `&String → &str` s'applique aussi « en position de
   récepteur », mais `fn coerce` n'est appelé que pour les arguments (et les
   éléments d'agrégat). Aujourd'hui la clause est **inobservable** : toutes les
   méthodes de texte existent à la fois sur `String` et sur `&str`
   (`builtins.rs`, `STRING_KINDS` / `STR_KINDS`), donc aucun programme ne la
   distingue. Proposition : soit l'implémenter, soit **retirer « ou de
   récepteur »** du texte ; la seconde option est moins coûteuse et plus
   honnête tant qu'aucun cas ne peut la tester.
2. **Champ `&str` non abaissable** (`E0106`) : défaut indépendant, à traiter
   dans le contrat de lowering (P1-A, §« ce qui manque »). Il rend
   `struct P { s: &str }` inutilisable même sans coercition.
3. **Position de retour** : `.as_str()` ne peut pas être retourné
   (`E-OWN-REF-ESCAPE`). La spec doit le dire (proposition TYP-8.5 ci-dessus) ;
   sinon un lecteur peut croire que TYP-8.5 offre une conversion universelle.
4. **L'arbre partagé est sale** : au moment de la mesure, `typeck.rs` et
   `emit.rs` étaient modifiés par un autre agent (non commités). Le
   comportement de ces modifications n'est ni mesuré ni revendiqué ici. Toute
   re-mesure doit repartir d'un export propre, sinon les chiffres ne sont pas
   attribuables.
5. **Le corpus ne teste pas le contrat de lowering** : 386 cas, dont la majorité
   en `compile-pass` qui ne compilent jamais avec `rustc`. C'est structurel, et
   c'est la cause racine pour laquelle `check`-accepte/`build`-échoue n'a pas
   été vu plus tôt.

## 8. Ce que cette RFC ne fait pas

- Elle ne modifie ni `spec/language/`, ni `crates/`, ni `conformance/`.
- Elle ne supprime ni n'assouplit aucun test ; elle en propose.
- Elle ne ratifie pas D-COERCE-1 et ne présente aucune option comme décidée.
- Elle ne mesure pas le nombre exact de cas cassés par le retrait (marqué
  **NOT_MEASURED**, commande fournie) ni ne rejoue le smoke.

## Annexe A — programmes des sondes

Tous les programmes sont complets (`use std::io;` implicite dans le tableau
ci-dessous) et ont été exécutés seuls, dans un package neuf, sur le binaire
`5d88ecb` décrit en §2.1.

```ravum
// #1  takes(&a)
fn takes(s: &str) -> usize { return s.len(); }
fn main() { let a = String::from("ab"); let n = takes(&a); io::println(&n.to_string()); }

// #2  takes(b)  → check ACCEPT, build E0308
fn takes(s: &str) -> usize { return s.len(); }
fn main() { let b = String::from("cd"); let n = takes(b); io::println(&n.to_string()); }

// #3  takes(line.trim())
fn takes(s: &str) -> usize { return s.len(); }
fn main() { let line = String::from("  x "); let n = takes(line.trim()); io::println(&n.to_string()); }

// #8  let t: (&str, i32) = (b, 1);
fn main() { let b = String::from("cd"); let t: (&str, i32) = (b, 1); io::println(t.0); }

// #9  let xs: [&str; 1] = [b];
fn main() { let b = String::from("cd"); let xs: [&str; 1] = [b]; io::println(xs[0]); }

// #10 struct P { s: &str } … P { s: b }
struct P { s: &str }
fn main() { let b = String::from("cd"); let p = P { s: b }; io::println(p.s); }
```

Commande reproductible (script temporaire `target/coerce-matrix.py`, non
versionné — il crée les packages dans `%TEMP%` et n'écrit rien dans le dépôt) :

```
python target/coerce-matrix.py      # binaire de target/debug (non reproducible sous charge)
RAVUM_PROBE_BIN=<binaire exporté de HEAD> python target/coerce-matrix.py
```

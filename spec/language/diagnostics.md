# Ravum Core 1.0 — Diagnostics, schéma JSON et codes de sortie

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §2, §3, §4.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> La liste des codes ci-dessous est **canonique** : c'est celle que l'implémentation émet. Tout code absent est interdit. Tous les codes listés sont **actifs** ; aucun code `reserved` ne subsiste.

## 1. Modèle de diagnostic

Un diagnostic est un objet immuable produit par une phase du compilateur. Il contient : un **code stable**, une **sévérité** (`error` | `warning`), une **phase**, un **message** (non stable), un **span primaire**, des **labels** optionnels, des **notes**, une **aide** optionnelle, et le cas échéant un `feature_id` ([`features.md`](features.md)) et un `rule_id` (identifiant de règle de la spécification).

| ID | Règle | Niveau |
|---|---|---|
| DIAG-1.1 | Tout diagnostic **MUST** porter un `code` de §3 et une `severity`. Aucun diagnostic anonyme. | MUST |
| DIAG-1.2 | Le **code** est stable et contractuel. Le **message** ne l'est pas : il peut être reformulé sans changement de version de schéma. | MUST |
| DIAG-1.3 | Un diagnostic **MUST** référencer un fichier et des offsets valides du fichier original (jamais un fichier généré). | MUST |
| DIAG-1.4 | Les phases produisent des diagnostics **dans l'ordre de §4**, et une phase précoce **empêche** les phases suivantes sur le même nœud. | MUST |
| DIAG-1.5 | Une erreur n'est **jamais** convertie en avertissement pour obtenir un vert. Aucun moyen (CLI, attribut, variable d'environnement) ne permet de dégrader une erreur. | MUST |
| DIAG-1.6 | Les avertissements sont une **liste fermée** : `W-RES-UNUSED-IMPORT`, `W-OWN-UNUSED-VARIABLE`. Aucun autre `W-*` n'existe en Core 1.0. | MUST |
| DIAG-1.7 | Le nombre de diagnostics par fichier est **borné** (budget déterministe) : au-delà, un unique `E-PARSE-TOO-LARGE` est ajouté et l'analyse du fichier s'arrête. | MUST |
| DIAG-1.8 | Deux exécutions sur la même source et la même configuration produisent la **même liste**, dans le même ordre (T3). | MUST |

## 2. Sévérités et effets sur la compilation

| Sévérité | Effet | Code de sortie |
|---|---|---|
| `error` | la compilation échoue, aucun binaire n'est produit, aucune sortie n'est activée | `1` |
| `warning` | la compilation continue, le binaire est produit | `0` (sauf autre erreur) |

Un `warning` ne peut **jamais** être promu en `error` en Core 1.0 (pas de `--deny` global ; `lint --deny CODE` ne concerne que les codes `W-*` réellement émis).

## 3. Registre canonique des codes

### 3.1 Lexique et syntaxe

| Code | Phase | Sens |
|---|---|---|
| `E-LEX-COMMENT` | lex | commentaire de bloc non fermé avant EOF |
| `E-LEX-STRING` | lex | littéral de chaîne non fermé avant EOF |
| `E-LEX-CHAR` | lex | littéral de caractère vide, multiple, non fermé, ou surrogat |
| `E-LEX-ESCAPE` | lex | échappement invalide, inconnu ou non terminé |
| `E-LEX-NUMBER` | lex | littéral numérique mal formé (séparateur, base, exposant, flottant infini) |
| `E-LEX-INT-OVERFLOW` | lex | littéral entier hors de `u128` |
| `E-LEX-INT-SUFFIX` | lex | suffixe de type entier inconnu |
| `E-PARSE-EXPECTED` | parse | token inattendu là où une forme précise était attendue (`;`, identifiant, type, `:`, champ, item) |
| `E-PARSE-UNEXPECTED` | parse | token inattendu dans une position donnée (`*` dans `use`, `..` hors `for`, comparaison chaînée, `::<`) |
| `E-PARSE-UNCLOSED` | parse | délimiteur `(`, `[`, `{` non fermé |
| `E-PARSE-DEPTH` | parse | profondeur d'imbrication dépassée (limite déterministe) |
| `E-PARSE-TOO-LARGE` | parse | fichier, nombre de nœuds ou budget de diagnostics dépassé |
| `E-UNSUPPORTED-IDENT` | lex | identifiant non ASCII (Unicode hors Core 1.0) |
| `E-UNSUPPORTED-TOKEN` | lex/parse | construction syntaxique reconnue mais hors Core 1.0 (voir [`unsupported.md`](unsupported.md)) |
| `E-UNSUPPORTED-TRAIT` | parse | `trait`, borne `T: X`, `dyn T` |
| `E-UNSUPPORTED-ATTR` | parse | attribut `#[...]` / `#![...]` |
| `E-UNSUPPORTED-MACRO` | parse | `nom!(...)` / `macro_rules!` |
| `E-UNSUPPORTED-ASYNC` | parse | `async`, `await` |
| `E-UNSUPPORTED-UNSAFE` | parse | `unsafe fn`, pointeurs bruts, opérations non vérifiées (le bloc `unsafe { }` est supporté) |
| `E-UNSUPPORTED-EXTERN` | parse | `extern "C" fn` avec corps (le bloc `extern "C" { ... }` est supporté) |
| `E-UNSUPPORTED-ABI` | parse | ABI autre que `"C"` dans un bloc `extern` |
| `E-FFI-TYPE` | resolve | paramètre/retour `extern` non FFI-sûr |
| `E-UNSAFE-CONTEXT` | types | appel de fonction `extern` hors bloc `unsafe` |
| `E-UNSUPPORTED-WHERE` | parse | clause `where` |
| `E-UNSUPPORTED-IMPL-TRAIT` | parse | `impl Trait` en position de type |
| `E-UNSUPPORTED-EFFECT` | sema | déclaration d'un effet non implémenté (`net`, `unsafe`, `ffi`, `async`) |
| `E-UNSUPPORTED-EDITION` | modules | `edition` de `ravum.toml` inconnue |
| `E-UNSUPPORTED-RANGE` | parse | `..`/`..=` hors `for`, slicing `&v[a..b]`, `..base` |
| `E-UNSUPPORTED-CLOSURE` | parse | `|x| e` |
| `E-UNSUPPORTED-DROP-IMPL` | sema | `impl Drop` / destructeur utilisateur |

### 3.2 Résolution des noms

| Code | Phase | Sens |
|---|---|---|
| `E-RES-UNKNOWN` | resolve | nom, module, fonction, méthode, champ de module, variante non qualifiée d'un enum utilisateur, `str` seul, `io.println` (point) |
| `E-RES-PRIVATE` | resolve | item, champ ou module privé accédé hors de son sous-arbre |
| `E-RES-DUPLICATE` | resolve | deux items/imports de même nom dans le même module |
| `E-RES-CONFLICT` | resolve | un import entre en collision avec une déclaration locale ou un autre import ambigu |
| `E-RES-SELF` | resolve | `self`/`Self` hors contexte valide (`Self` hors `impl`, `self` param hors `impl`) |
| `E-RES-IMPL-UNKNOWN` | resolve | cible d'`impl` qui n'est pas un type nominal |
| `E-RES-IMPL-DUPLICATE` | resolve | deux méthodes de même nom dans le même `impl` (ou impls redondants) |
| `E-RES-IMPL-SELF-TYPE` | resolve | `impl` sur un type étranger (`i32`, `String`, `Vec<T>`) |

### 3.3 Modules et packages

| Code | Phase | Sens |
|---|---|---|
| `E-MOD-MISSING` | modules | fichier de module absent, ou manifeste/lockfile absent quand requis |
| `E-MOD-AMBIGUOUS` | modules | `foo.rv` **et** `foo/mod.rv` existent |
| `E-MOD-CYCLE` | modules | cycle de dépendances entre packages |
| `E-MOD-DUPLICATE` | modules | module déclaré deux fois / deux packages de même nom |
| `E-MOD-PATH` | modules | chemin de module ou de dépendance invalide : lien symbolique, hors `src/`, forme incorrecte |
| `E-MOD-PATH-ESCAPE` | modules | sortie de périmètre : chemin absolu, `..` interdit, profondeur > 8 |
| `E-MOD-RESERVED-NAME` | modules | nom de chemin réservé Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`) |
| `E-MOD-MANIFEST` | modules | `ravum.toml` invalide, clé inconnue, nom/version malformés |
| `E-MOD-DEPENDENCY` | modules | dépendance malformée (pas de `path`, nom≠`package.name`, cible sans `src/lib.rv`, conflit) |
| `E-MOD-BUILD-SCRIPT` | modules | `build.rs`, `[build]`, script refusé |
| `E-MOD-BOTH-ROOTS` | modules | `src/main.rv` **et** `src/lib.rv` présents (interdit, Q-MOD-1) |
| `E-MOD-LOCK` | modules | `lockfile_version` inconnu ou lockfile illisible |

### 3.4 Typage

| Code | Phase | Sens |
|---|---|---|
| `E-TYP-MISMATCH` | types | types incompatibles (opérandes, branches, arguments, retour, affectation) |
| `E-TYP-ARITY` | types | mauvais nombre d'arguments, de paramètres génériques, de champs de tuple |
| `E-TYP-FIELD` | types | champ inexistant, champ manquant, indice de tuple hors bornes |
| `E-TYP-CALL` | types | appel d'une valeur non-fonction |
| `E-TYP-INDEX` | types | index non `usize`, type non indexable (`String`), index de tuple invalide |
| `E-TYP-CAST` | types | `as` hors paires admises (entier→`char`, entier→`bool`, référence, agrégat) |
| `E-TYP-SHIFT` | types | décalage négatif ou quantité invalide |
| `E-TYP-EQ` | types | `==`/`!=` sur un type non comparable (agrégat, `Option`, `Result`, `T`), `assert_eq` invalide |
| `E-TYP-CMP` | types | `< <= > >=` sur un type non ordonné (texte, agrégat) |
| `E-TYP-AMBIGUOUS` | types | inférence non résolue (`Vec::new()` sans contexte, littéral non contraint) |
| `E-TYP-IF-NO-ELSE` | types | `if` sans `else` utilisé en valeur |
| `E-TYP-STATEMENT-VALUE` | types | expression non-`()` en position d'instruction |
| `E-TYP-NONEXHAUSTIVE` | types | `match` non exhaustif (enum, `bool`, `Option`, `Result`) |
| `E-TYP-TRY` | types | `?` hors contexte compatible |
| `E-TYP-RETURN` | types | `return` sans valeur dans une fonction non-`()`, ou valeur incompatible |
| `E-TYP-MAIN` | types | signature de `main` invalide (paramètres, génériques, retour ≠ `()`/`Result<(), Error>`), `test fn` invalide |
| `E-TYP-MAIN-MISSING` | types | aucune `fn main` dans un crate binaire |
| `E-TYP-MAIN-DUPLICATE` | types | plusieurs `fn main` |
| `E-TYP-LITERAL-RANGE` | types | littéral entier qui ne tient pas dans le type inféré |
| `E-TYP-FLOAT-LITERAL` | types | littéral flottant invalide pour le type inféré (`1e39` en `f32`) |
| `E-TYP-BREAK-OUTSIDE` | types | `break`/`continue` hors boucle |
| `E-TYP-RANGE` | types | bornes de plage de types différents ou non entières |
| `E-TYP-ARRAY-REPEAT` | types | `[e; n]` invalide (n non littéral/0, `e` non `Copy`), longueur de tableau invalide |
| `E-TYP-RECURSIVE` | types | type infini sans indirection (`struct Node { next: Option<Node> }`) |
| `E-TYP-CONST` | types | `const` non constant ou évaluation de constante en erreur (débordement, /0) |

### 3.5 Propriété et emprunts

| Code | Phase | Sens |
|---|---|---|
| `E-OWN-UNINIT` | ownership | lecture/emprunt d'une place non initialisée sur un chemin |
| `E-OWN-USE-AFTER-MOVE` | ownership | usage d'une place déplacée |
| `E-OWN-DOUBLE-MOVE` | ownership | deux déplacements de la même place sans usage entre eux |
| `E-OWN-PARTIAL-MOVE` | ownership | déplacement partiel interdit (champ de struct, `Vec[i]`), agrégat réutilisé après move partiel |
| `E-OWN-BORROW-CONFLICT` | ownership | prêt exclusif/partagé recouvrant un accès incompatible |
| `E-OWN-REF-ESCAPE` | ownership | référence stockée, retournée, mise en champ/collection |
| `E-OWN-MOVE-OUT-OF-BORROW` | ownership | déplacement d'une valeur derrière une référence |
| `E-OWN-IMMUTABLE` | ownership | affectation d'une place immuable (méthode `&mut self` sur temporaire incluse) |

### 3.6 Effets

| Code | Phase | Sens |
|---|---|---|
| `E-EFF-UNKNOWN` | effects | nom d'effet inconnu (pas un des 9 groupes) |
| `E-EFF-UNDECLARED` | effects | effet inféré absent de la déclaration majorante |
| `E-EFF-PROFILE` | effects | effet interdit par le profil `pure` ou `rt` |

### 3.7 Avertissements

| Code | Phase | Sens |
|---|---|---|
| `W-RES-UNUSED-IMPORT` | resolve | import jamais utilisé |
| `W-OWN-UNUSED-VARIABLE` | ownership | variable liée jamais utilisée (y compris `mut` inutile) |

> Les codes `reserved` sont documentés pour que la spécification soit complète, mais l'implémentation actuelle ne les émet pas. Un test de conformance qui les attend **doit** être marqué `PENDING_IMPL`, jamais `PASS`.

## 4. Ordre de production et déterminisme
### 3.1 Codes ajoutés par la passe de qualification (2026-11-10)

L'audit `python tools/registry_audit.py` compare **tous** les littéraux de code
des sources à ce registre. Il a montré que le compilateur émettait des codes
absents de §3, ce que DIAG-1.1 interdit. Plutôt que de les remplacer par des
codes génériques — ce qui aurait dégradé des messages précis —, ils sont
**enregistrés ici** avec leur phase et leur sens.

| Code | Phase | Sens |
|---|---|---|
| `E-LIMIT-FILE-SIZE` | parse | fichier source au-delà de la taille maximale analysable |
| `E-LIMIT-FILES` | parse | nombre de fichiers du paquet au-delà de la limite |
| `E-OWN-LIMIT` | ownership | limite interne de l'analyse d'ownership atteinte |
| `E-PARSE-BAD-SUFFIX` | parse | suffixe de littéral entier inconnu |
| `E-RES-CONST-NOT-CONST` | resolve | valeur de constante non représentable dans les faits du checker |
| `E-RES-IMPL-TARGET` | resolve | paramètres génériques d'`impl` incohérents avec la cible |
| `E-RES-MODDECL` | resolve | déclaration `mod nom;` non étendue par le chargeur |
| `E-RES-SELF-OUTSIDE-IMPL` | resolve | `Self` utilisé hors d'un `impl` |
| `E-TYP-MISSING-FIELD` | types | champ manquant dans un littéral de struct |
| `E-UNSUPPORTED-ALIAS` | parse | alias `use … as …` refusé |
| `E-UNSUPPORTED-ASSOC-TYPE` | parse | type associé sur `Self` refusé |
| `E-UNSUPPORTED-BOUNDS` | parse | bornes génériques (`T: X`) refusées |
| `E-UNSUPPORTED-COMPOUND-ASSIGN` | parse | affectation composée refusée |
| `E-UNSUPPORTED-FN-TYPE` | parse | type pointeur de fonction (`fn(...)`) refusé |
| `E-UNSUPPORTED-MOVE` | parse | `move` refusé |
| `E-UNSUPPORTED-NEG-PAT` | parse | motif de littéral négatif refusé |
| `E-UNSUPPORTED-RANGE-PAT` | parse | motif de plage (`a..b`) refusé |
| `E-UNSUPPORTED-REEXPORT` | parse | `pub use` (ré-export) refusé |
| `E-UNSUPPORTED-REST-PAT` | parse | motif `..` (reste) refusé |
| `E-UNSUPPORTED-STATIC` | parse | item `static` refusé |
| `E-UNSUPPORTED-TUPLE-STRUCT` | parse | struct tuple (`struct S(...)`) refusée |
| `E-UNSUPPORTED-TYPE-ALIAS` | parse | alias de type (`type`) refusé |
| `E-UNSUPPORTED-UFCS` | parse | appel UFCS (`Type::method(receiver, …)`) refusé |
| `E-UNSUPPORTED-VIS` | parse | visibilité restreinte `pub(...)` refusée |

**Un code reste hors registre, volontairement** : `W-TYP-UNREACHABLE` (branche de
`match` inatteignable). DIAG-1.6 déclare la liste des avertissements **fermée**
(`W-RES-UNUSED-IMPORT`, `W-OWN-UNUSED-VARIABLE`) : enregistrer ce code
contredirait un MUST. Soit la liste s'ouvre (changement de spécification), soit
l'avertissement disparaît (perte de fonctionnalité). C'est une décision humaine,
pas une correction de documentation.


| Ordre | Phase | Codes |
|---|---|---|
| 1 | `lex` | `E-LEX-*`, `E-UNSUPPORTED-IDENT`, `E-UNSUPPORTED-TOKEN` (lexical) |
| 2 | `parse` | `E-PARSE-*`, `E-UNSUPPORTED-*` syntaxiques |
| 3 | `modules` | `E-MOD-*`, `E-UNSUPPORTED-EDITION` |
| 4 | `resolve` | `E-RES-*`, `W-RES-UNUSED-IMPORT` |
| 5 | `types` | `E-TYP-*` |
| 6 | `ownership` | `E-OWN-*`, `W-OWN-UNUSED-VARIABLE` |
| 7 | `effects` | `E-EFF-*` |

| ID | Règle | Niveau |
|---|---|---|
| DIAG-4.1 | À l'intérieur d'une phase, les diagnostics sont triés par `(file, start_byte, code)` croissants en ordre lexicographique ASCII. | MUST |
| DIAG-4.2 | Une phase ne produit pas de diagnostic sur un nœud déjà rejeté par une phase antérieure. | MUST |
| DIAG-4.3 | Le budget par fichier est de **100 diagnostics** ; au-delà, `E-PARSE-TOO-LARGE` unique et arrêt du fichier (DIAG-1.7). | MUST |
| DIAG-4.4 | La sortie est identique octet pour octet entre deux exécutions identiques, hors champs explicitement variables (`tool_version` si version différente, `duration_ms` s'il est exposé). | MUST |

## 5. Spans, lignes/colonnes, UTF-8 et UTF-16

Un **span** est `{ file, start_byte, end_byte, start_line, start_col, end_line, end_col, start_utf16, end_utf16 }`, demi-ouvert `[start_byte, end_byte)`, offsets **absolus en octets** du fichier original.

| ID | Règle | Niveau |
|---|---|---|
| DIAG-5.1 | `start_byte`/`end_byte` sont des offsets **octets** dans le fichier tel qu'il est sur disque (BOM refusé, donc aucun décalage). | MUST |
| DIAG-5.2 | `line` est **1-based** ; `col` est **1-based**, comptée en **code units UTF-16** depuis le début de la ligne. C'est la convention LSP, directement consommable par un IDE. | MUST |
| DIAG-5.3 | Une fin de ligne `CRLF` compte pour **un** saut de ligne et **deux** octets : `line` est incrémentée une fois, `col` repart à 1 après le `LF`. | MUST |
| DIAG-5.4 | `start_utf16`/`end_utf16` sont les offsets en code units UTF-16 **depuis le début du fichier** (pour les clients qui n'utilisent pas ligne/colonne). | MUST |
| DIAG-5.5 | Un span de diagnostic **MUST** couvrir au moins un scalaire et **MUST** ne pas dépasser les bornes du fichier. Un span vide n'est admis que pour un point d'insertion explicitement documenté (`end_byte == start_byte`) et uniquement si le code le prévoit. | MUST |
| DIAG-5.6 | Les colonnes ne sont **jamais** des octets ; un IDE qui affiche des colonnes en caractères doit convertir (caractères = points de code, pas de code units). | MUST |
| DIAG-5.7 | Le compilateur ne modifie **jamais** le fichier pour produire un span (pas de normalisation CRLF, pas de réécriture). | MUST |

```text
Exemple de mapping (fichier ASCII puis non-ASCII) :
ligne 1 : "let s = \"é\";"   -> 'é' = 2 octets, 1 code unit UTF-16
          l'offset byte de ';' est 13 ; `col` = 12 (11 code units avant)
```

> `é` (U+00E9) = 2 octets UTF-8, 1 code unit UTF-16 ; `😀` (U+1F600) = 4 octets UTF-8, **2** code units UTF-16. C'est exactement pourquoi `col` en UTF-16 diffère de `col` en caractères.

## 6. Format texte humain

```text
error[E-OWN-USE-AFTER-MOVE]: use of moved value: `s`
  --> examples/tests-demo/src/main.rv:14:12
   |
12 |     let t = String::from("a");
   |             ----------------- value moved here
13 |     let s = t;
   |             - second move
14 |     io::println(s.as_str());
   |                ^ use after move
   |
   = note: `String` is not `Copy`; use `.clone()` to duplicate
   = help: reorder the use before the move
```

| ID | Règle | Niveau |
|---|---|---|
| DIAG-6.1 | Ligne 1 : `severity[CODE]: message`. | MUST |
| DIAG-6.2 | Ligne 2 : `  --> <fichier>:<line>:<col>` (deux espaces, flèche, une espace). | MUST |
| DIAG-6.3 | Chaque label est affiché avec son span, aligné par numéro de ligne ; le label primaire porte `^`, les labels secondaires `-`. | MUST |
| DIAG-6.4 | Les notes sont préfixées `= note: `, l'aide `= help: `. | MUST |
| DIAG-6.5 | **Aucune couleur** si stdout/stderr n'est pas un terminal, si `--json` est présent, ou si `NO_COLOR` est défini. Aucune couleur n'est **jamais** émise sur la sortie JSON. | MUST |
| DIAG-6.6 | Le format texte est destiné aux humains : il n'est **pas** stable au caractère près. Seul le code est contractuel. | MUST |
| DIAG-6.7 | Les diagnostics texte vont sur **stderr** ; la sortie JSON va sur **stdout**. | MUST |
| DIAG-6.8 | Aucune trace de pile, aucun chemin absolu de la machine, aucun nom d'utilisateur dans un diagnostic. | MUST |

## 7. Schéma JSON versionné (`schema_version: 1`)

### 7.1 Enveloppe

```json
{
  "schema_version": 1,
  "tool": "ravum-lang",
  "tool_version": "0.0.0",
  "spec_version": "core-1.0-draft.1",
  "command": "check",
  "status": "failure",
  "exit_code": 1,
  "target": { "kind": "package", "path": "examples/tests-demo" },
  "profile": "core",
  "dependencies_digest": "sha256:3f1c0a...9e2",
  "lockfile_stale": false,
  "summary": { "errors": 1, "warnings": 0 },
  "diagnostics": [
    {
      "code": "E-OWN-USE-AFTER-MOVE",
      "severity": "error",
      "phase": "ownership",
      "message": "use of moved value: `s`",
      "feature_id": "F-OWN-002",
      "rule_id": "OWN-5.1",
      "primary": {
        "file": "src/main.rv",
        "start_byte": 187,
        "end_byte": 188,
        "start_line": 14,
        "start_col": 16,
        "end_line": 14,
        "end_col": 17,
        "start_utf16": 178,
        "end_utf16": 179,
        "node_id": 42
      },
      "labels": [
        {
          "message": "value moved here",
          "file": "src/main.rv",
          "start_byte": 121,
          "end_byte": 122,
          "start_line": 12,
          "start_col": 13,
          "end_line": 12,
          "end_col": 14,
          "start_utf16": 112,
          "end_utf16": 113,
          "node_id": 31
        }
      ],
      "notes": ["`String` is not `Copy`"],
      "help": "reorder the use before the move",
      "related": []
    }
  ]
}
```

### 7.2 Schéma formel (résumé)

| Champ | Type | Obligatoire | Notes |
|---|---|---|---|
| `schema_version` | entier | oui | **`1`** ; toute rupture incrémente |
| `tool` | chaîne | oui | `"ravum-lang"` |
| `tool_version` | chaîne | oui | version du binaire |
| `spec_version` | chaîne | oui | `core-1.0-draft.1` (ou version ratifiée) |
| `command` | chaîne | oui | `check`, `build`, `run`, `test`, `lint`, `doc`, `fmt`, `doctor`, `init` |
| `status` | chaîne | oui | `success` \| `failure` \| `unsupported` |
| `exit_code` | entier | oui | voir §8 |
| `target` | objet | oui | `{ kind: "file"\|"package"\|"workspace", path }` |
| `profile` | chaîne | non | `core` \| `pure` \| `rt` (si applicable) |
| `dependencies_digest` | chaîne | non | `sha256:<hex>` ([`modules.md`](modules.md) MOD-7.4) |
| `lockfile_stale` | booléen | non | `true` si `ravum.lock` ne correspond plus |
| `summary` | objet | oui | `{ errors, warnings }` |
| `diagnostics` | tableau | oui | ordonné (§4), éventuellement vide |
| `diagnostics[].code` | chaîne | oui | §3 |
| `diagnostics[].severity` | chaîne | oui | `error` \| `warning` |
| `diagnostics[].phase` | chaîne | oui | §4 |
| `diagnostics[].message` | chaîne | oui | non stable |
| `diagnostics[].feature_id` | chaîne | non | `F-*` ([`features.md`](features.md)) |
| `diagnostics[].rule_id` | chaîne | non | ex. `OWN-5.1` |
| `diagnostics[].primary` | span | oui | objet span §7.3 |
| `diagnostics[].labels` | tableau de labels | oui | peut être vide |
| `diagnostics[].notes` | tableau de chaînes | oui | peut être vide |
| `diagnostics[].help` | chaîne | non | — |
| `diagnostics[].related` | tableau d'objets | non | `{ code, span }` |

### 7.3 Objet `span`

| Champ | Type | Obligatoire |
|---|---|---|
| `file` | chaîne | oui |
| `start_byte`, `end_byte` | entiers ≥ 0 | oui |
| `start_line`, `end_line` | entiers ≥ 1 | oui |
| `start_col`, `end_col` | entiers ≥ 1 (UTF-16) | oui |
| `start_utf16`, `end_utf16` | entiers ≥ 0 | oui |
| `node_id` | entier ≥ 0 | non (présent si le nœud AST existe) |

| ID | Règle | Niveau |
|---|---|---|
| DIAG-7.1 | Un objet JSON **MUST** être émis même en cas d'échec (y compris erreur de manifeste) : `--json` ne produit jamais autre chose que du JSON sur stdout. | MUST |
| DIAG-7.2 | Le JSON est **UTF-8 sans BOM**, sans couleur, sans commentaire, sur **une seule ligne** par objet ; un seul objet par commande en Core 1.0 (pas de flux NDJSON). | MUST |
| DIAG-7.3 | Les champs inconnus d'un objet doivent être **ignorés** par le consommateur (compatibilité additive) ; un producteur ne doit **pas** retirer un champ sans incrémenter `schema_version`. | MUST |
| DIAG-7.4 | Les tableaux vides (`labels`, `notes`, `related`, `diagnostics`) sont **présents** et vides, jamais `null`. | MUST |
| DIAG-7.5 | Aucun champ ne contient de valeur dépendante de la machine (chemin absolu, nom d'utilisateur, durée non déterministe). Les chemins sont **relatifs** à la racine du package et normalisés en `/`. | MUST |

## 8. Codes de sortie du CLI

| Code | Sens | Commandes |
|---|---|---|
| `0` | succès | toutes |
| `1` | erreur de compilation (≥ 1 diagnostic `error`), test en échec, ou panic du programme exécuté | `check`, `build`, `test`, `run` |
| `2` | usage/CLI invalide (argument inconnu, `--profile` inconnu, chemin invalide) | toutes |
| `3` | outil externe absent (`rustc`) ou environnement non supporté | `build`, `run`, `test`, `doctor` |

| ID | Règle | Niveau |
|---|---|---|
| DIAG-8.1 | `run` **propage** le code de sortie du programme exécuté ; `0`, `1`, `2`, `3` n'ont alors **pas** la signification ci-dessus. Un script qui a besoin de distinguer doit utiliser `--json` (`status`, `exit_code`). | MUST |
| DIAG-8.2 | Un panic (message `panic: ...`) donne `1`, comme une erreur de compilation : la distinction se fait par le texte (`panic:` vs `error[...]`) ou par `--json` (`status`). | MUST |
| DIAG-8.3 | `test` rend `1` si **au moins un** test échoue (assertion, `Err`, panic), `0` sinon. Le JSON contient la liste des tests. | MUST |
| DIAG-8.4 | `--json` n'altère **jamais** le code de sortie : il reste celui de la commande. | MUST |
| DIAG-8.5 | Une erreur d'usage (code `2`) produit un message court sur stderr et **aucun** binaire, quel que soit `--json`. | MUST |

## 9. Garanties de stabilité

| ID | Règle | Niveau |
|---|---|---|
| DIAG-9.1 | Un code de §3 est **immuable** : il ne change jamais de sens, n'est jamais réutilisé pour un autre défaut, et n'est jamais supprimé (au plus marqué `deprecated`, jamais dans Core 1.0). | MUST |
| DIAG-9.2 | Ajouter un code est **rétrocompatible** ; le retirer ou changer sa sévérité ne l'est pas. | MUST |
| DIAG-9.3 | `schema_version` incrémente si et seulement si un champ obligatoire est retiré, renommé, ou change de type/sémantique. Ajouter un champ optionnel ne l'incrémente pas. | MUST |
| DIAG-9.4 | Les messages, les notes et les aides sont **hors contrat** ; un test de conformance ne doit jamais asserter leur texte. | MUST |
| DIAG-9.5 | L'apparition d'un **nouveau** code n'est pas une rupture (les consommateurs doivent tolérer les codes inconnus) ; un code n'est jamais retiré ni réutilisé. | MUST |
| DIAG-9.6 | Le `feature_id` et le `rule_id` sont **stables et contractuels** ; les renommer est une rupture de compatibilité ([`compatibility.md`](compatibility.md) COMPAT-2.9). Leur absence reste tolérée côté consommateur. | MUST |

## 10. Exemples complets

### 10.1 Texte (erreur unique)

```text
error[E-TYP-MISMATCH]: mismatched types
  --> examples/hello/src/main.rv:5:14
   |
 5 |     let n: i64 = 1;
   |                  ^ expected `i64`, found `i32`
   |
   = note: no implicit numeric conversion in Core 1.0
   = help: write `1i64` or `1 as i64`
```

### 10.2 JSON (succès sans diagnostic)

```json
{"schema_version":1,"tool":"ravum-lang","tool_version":"0.0.0","spec_version":"core-1.0-draft.1","command":"check","status":"success","exit_code":0,"target":{"kind":"package","path":"examples/hello"},"profile":"core","dependencies_digest":"sha256:0000","lockfile_stale":false,"summary":{"errors":0,"warnings":0},"diagnostics":[]}
```

### 10.3 JSON (deux diagnostics, dont un avertissement)

```json
{"schema_version":1,"tool":"ravum-lang","tool_version":"0.0.0","spec_version":"core-1.0-draft.1","command":"check","status":"failure","exit_code":1,"target":{"kind":"package","path":"examples/stats"},"profile":"core","summary":{"errors":1,"warnings":1},"diagnostics":[{"code":"W-RES-UNUSED-IMPORT","severity":"warning","phase":"resolve","message":"unused import: `std::math`","primary":{"file":"src/main.rv","start_byte":10,"end_byte":23,"start_line":3,"start_col":5,"end_line":3,"end_col":18,"start_utf16":10,"end_utf16":23},"labels":[],"notes":[],"related":[]},{"code":"E-TYP-MISMATCH","severity":"error","phase":"types","message":"mismatched types","feature_id":"F-TYP-001","rule_id":"TYP-7.1","primary":{"file":"src/main.rv","start_byte":88,"end_byte":97,"start_line":6,"start_col":20,"end_line":6,"end_col":29,"start_utf16":88,"end_utf16":97,"node_id":17},"labels":[],"notes":["no implicit numeric conversion in Core 1.0"],"help":"write `1i64` or `1 as i64`","related":[]}]}
```

## 11. Questions ouvertes

- **Q-DIAG-1 (résolue par le Lead)** — Sont **canoniques** : `E-MOD-MANIFEST`, `E-MOD-DEPENDENCY`, `E-MOD-BUILD-SCRIPT`, `E-MOD-LOCK`, `E-MOD-BOTH-ROOTS`, `E-MOD-PATH-ESCAPE`, `E-MOD-RESERVED-NAME`, `E-OWN-IMMUTABLE`, `E-TYP-RECURSIVE`, `E-TYP-CONST`, `E-UNSUPPORTED-DROP-IMPL`. Aucun code `reserved` ne subsiste.
- **Q-DIAG-2 (résolue par le Lead)** — `E-RES-VISIBILITY` est **supprimé** : la visibilité utilise `E-RES-PRIVATE` (pas de doublon).
- **Q-DIAG-3** — Les avertissements sont limités à deux codes. Cela exclut : import de dépendance inutilisée, variable `mut` inutile distincte, bras inatteignable, effet déclaré non inféré. Assumé ; à étendre seulement par décision.
- **Q-DIAG-4 (résolue par le Lead)** — `rule_id` est **contractuel et stable** : le renommer est une rupture de compatibilité ([`compatibility.md`](compatibility.md) COMPAT-2.9). Les identifiants `OWN-5.1`, `TYP-7.1`, etc. sont donc des noms publics.
- **Q-DIAG-5** — Le format texte (DIAG-6) est spécifié mais non contractuel. Faut-il un mode stable (`--error-format=json` suffit-il) pour les agents ? Recommandation : oui, JSON uniquement.
- **Q-DIAG-6** — `run` propage le code du programme (DIAG-8.1), ce qui rend `1` ambigu. Un mode `--wrap-exit` (préfixe `10+`) n'existe pas ; à décider si un CI en a besoin.
- **Q-DIAG-7** — Le budget de 100 diagnostics par fichier (DIAG-4.3) n'est pas dans le design ; il est nécessaire à P0-H (bornes). Valeur à ratifier.

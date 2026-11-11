# Ravum Core 1.0 — Spécification lexicale

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** : ADR-000…041 restent `PROPOSED` et aucune règle écrite ici n'est acceptée du fait de sa présence. Aucune implémentation n'est revendiquée ; `NOT_IMPLEMENTED` / `NOT_COVERED` marquent l'état réel.
> Conventions : **MUST** = obligatoire, **MUST NOT** = interdit, **SHOULD** = visé, **MAY** = permis. Prose en français, identifiants et API en anglais.
> Les codes de diagnostic cités sont ceux de [`diagnostics.md`](diagnostics.md) ; les identifiants de fonctionnalité sont ceux de [`features.md`](features.md).

## 1. Portée

Cette spécification définit la **forme concrète du texte source** `.rv` : encodage, fins de ligne, commentaires, identifiants, mots-clés, littéraux, normalisation, et la grammaire lexicale (tokens). Elle ne définit **ni** la grammaire syntaxique (voir [`grammar.md`](grammar.md)) **ni** le typage (voir [`types.md`](types.md)).

Le lexer produit une séquence de tokens avec des `Span` exprimés en **octets du fichier original** (§9). Le lexer **MUST NOT** paniquer sur une entrée hostile : toute entrée invalide produit un diagnostic `E-LEX-*` et le lexer reprend au prochain point de reprise déterministe (§11).

## 2. Fichiers et encodage

| ID | Règle | Niveau |
|---|---|---|
| LEX-2.1 | Un fichier source Ravum a l'extension `.rv` et un contenu **UTF-8 valide**. | MUST |
| LEX-2.2 | Un BOM UTF-8 (`EF BB BF`) **en tête de fichier est accepté et ignoré** : il ne produit aucun diagnostic et n'est pas un token. Les offsets restent **absolus dans le fichier original** (le premier token commence à l'octet 3). Un `U+FEFF` ailleurs qu'en tête est un caractère non supporté (`E-UNSUPPORTED-TOKEN`). | MUST |
| LEX-2.3 | Une séquence d'octets qui n'est pas de l'UTF-8 valide (octet de continuation orphelin, séquence tronquée, surrogat encodé CESU-8, dépassement `U+10FFFF`) produit `E-UNSUPPORTED-TOKEN` sur la séquence concernée. Le fichier est refusé ; aucune réparation n'est tentée. | MUST |
| LEX-2.4 | Les octets NUL (`0x00`) dans le texte source hors littéraux de chaîne/caractère sont invalides (`E-UNSUPPORTED-TOKEN`). Un NUL **dans** un littéral est autorisé seulement via l'échappement `\0`. | MUST |
| LEX-2.5 | Le langage ne définit **aucune** normalisation Unicode du texte source (NFC/NFD/NFKC). La comparaison porte sur les octets. Les identifiants étant ASCII (LEX-4.1), les collisions de normalisation sont impossibles dans les identifiants ; les contenus de chaînes et de caractères sont préservés exactement. | MUST |

**Justification du BOM ignoré** : le refuser casserait les fichiers produits par des éditeurs Windows qui en ajoutent un systématiquement ; l'ignorer en tête est sans risque (les offsets restent absolus, le premier token commence à l'octet 3) et `fmt` peut le retirer. Un `U+FEFF` en milieu de fichier reste refusé.

### Exemples

```rv
// POSITIF — premier octet = '#' du commentaire
// hello.rv
fn main() { }
```

```rv
// NÉGATIF — E-UNSUPPORTED-TOKEN (span: octets 0..3)
﻿fn main() { }
```

```rv
// NÉGATIF — E-UNSUPPORTED-TOKEN (séquence 0xC3 seule, tronquée)
fn main() { } // Ã
```

> Encodage d'un caractère non-ASCII **dans une chaîne** : autorisé, c'est de l'UTF-8 valide.
> `let s = "é";` est valide (LEX-2.1) ; `let é = 1;` est refusé par LEX-4.1 (`E-UNSUPPORTED-IDENT`).

## 3. Fins de ligne

| ID | Règle | Niveau |
|---|---|---|
| LEX-3.1 | La fin de ligne canonique est `LF` (`0x0A`). | MUST |
| LEX-3.2 | `CRLF` (`0x0D 0x0A`) est **accepté en lecture** et traité comme **un seul** terminateur de ligne. Les spans restent exprimés dans le fichier original : un CRLF occupe 2 octets et incrémente la ligne de 1. | MUST |
| LEX-3.3 | Un `CR` seul (non suivi de `LF`) produit `E-UNSUPPORTED-TOKEN`. Il n'est pas converti en fin de ligne. | MUST |
| LEX-3.4 | `ravum-lang fmt` normalise `CRLF` → `LF` et garantit un `LF` final. Le compilateur, lui, n'exige pas de `LF` final. | MUST |
| LEX-3.5 | Les lignes sont numérotées à partir de **1** ; les colonnes à partir de **1** (voir §9). | MUST |

```rv
// POSITIF — LF uniquement
fn main() {
}
```

```rv
// NÉGATIF — E-UNSUPPORTED-TOKEN (CR isolé en fin de ligne 1)
fn main() {
}
```

```text
// NÉGATIF — E-UNSUPPORTED-TOKEN : octet 0x0D isolé, ligne 2
fn main() { }\r
```

`check` **MUST NOT** produire de diagnostic pour un fichier écrit en CRLF : le fichier est valide, et `fmt` le normalise en LF (LEX-3.4). Aucun avertissement de fin de ligne n'existe en Core 1.0 : la liste des avertissements est **fermée** (`W-RES-UNUSED-IMPORT`, `W-OWN-UNUSED-VARIABLE`, voir [`diagnostics.md`](diagnostics.md) §2).

## 4. Identifiants

| ID | Règle | Niveau |
|---|---|---|
| LEX-4.1 | Un identifiant est ASCII et vérifie `[A-Za-z_][A-Za-z0-9_]*`. Aucune autre forme n'est acceptée. | MUST |
| LEX-4.2 | Un identifiant contenant un caractère non-ASCII (ex. `café`, `变量`, `λ`) produit `E-UNSUPPORTED-IDENT` (et non `E-UNSUPPORTED-TOKEN`), car l'Unicode est explicitement hors Core 1.0. Le span couvre l'identifiant complet. | MUST |
| LEX-4.3 | `_` seul n'est **pas** un identifiant : c'est le pattern joker (§ [`grammar.md`](grammar.md)). `_x` est un identifiant ordinaire. | MUST |
| LEX-4.4 | Deux identifiants qui ne diffèrent que par la casse sont **distincts**. Aucune conversion de casse n'est effectuée. | MUST |
| LEX-4.5 | Un identifiant **MUST NOT** être un mot-clé réservé (§5). Les mots-clés contextuels (§5.3) sont des identifiants ordinaires hors de leur position contextuelle. | MUST |

```rv
// POSITIF
let _tmp2 = 1;      // identifiant valide
let café = 1;       // NÉGATIF : E-UNSUPPORTED-IDENT
```

```rv
// NÉGATIF — E-UNSUPPORTED-IDENT (colonne 5..9)
let λ = 1;
```

> **Note** : les noms de modules, de champs, de variantes, de types et de fonctions suivent exactement la même règle. Il n'existe pas d'espace de noms Unicode séparé.

## 5. Mots-clés

### 5.1 Liste réservée (exhaustive, design §3)

```
as    break   const   continue  crate   else    enum    extern  static
false fn      for     if        impl    in      let     loop
match mod     move    mut       pub     return  self    Self
struct super  test    trait     true    unsafe  use     where
while async   type
```

Cette liste est **exhaustive** : elle est celle du design §3, reproduite sans ajout. Un token qui n'y figure pas est un identifiant.

| ID | Règle | Niveau |
|---|---|---|
| LEX-5.1 | Les **35** mots-clés ci-dessus sont réservés dans **toutes** les positions ; ils ne peuvent pas servir d'identifiant. `static` et `type` sont réservés (Q-LEX-1) mais **non supportés** à l'usage : `E-UNSUPPORTED-STATIC` / `E-UNSUPPORTED-TYPE-ALIAS`. | MUST |
| LEX-5.2 | `Self`, `self`, `crate`, `super` ont un sens de chemin (§ [`modules.md`](modules.md)) ; `Self` n'est valide que dans un `impl`. | MUST |
| LEX-5.3 | `true`/`false` sont des littéraux booléens, pas des identifiants ni des constantes du prélude. | MUST |
| LEX-5.4 | Les mots-clés réservés sont **reconnus par le lexer**. Ceux qui n'ont pas de construction Core 1.0 (`move`, `trait`, `where`, `async`) sont rejetés par le parser avec le code `E-UNSUPPORTED-*` correspondant (§ [`unsupported.md`](unsupported.md)). `extern` et `unsafe` ont une construction supportée (bloc `extern "C" { ... }`, bloc `unsafe { ... }`) ; seules leurs autres formes (`unsafe fn`, `extern "C" fn` avec corps) sont refusées (`E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-EXTERN`). Aucune forme n'est acceptée silencieusement. | MUST |

### 5.2 Mots-clés contextuels

| Mot | Position contextuelle | Hors position |
|---|---|---|
| `effects` | immédiatement après la signature (`)` ou type de retour) d'un `fn` | identifiant ordinaire |
| `str` | uniquement comme second token de `&str` en position de type | identifiant ordinaire |

| ID | Règle | Niveau |
|---|---|---|
| LEX-5.5 | `effects` **MUST NOT** être ajouté à la liste réservée : le design §3 ne le liste pas. Il est reconnu comme clause uniquement en position de signature. | MUST |
| LEX-5.6 | `str` **MUST NOT** être un type nommé autonome : seul `&str` est un type valide (`E-RES-UNKNOWN` pour `str` seul). | MUST |

### 5.3 Mots **non** réservés — conséquence assumée

`ref`, `dyn`, `await`, `box`, `yield`, `union`, `macro`, `final`, `abstract`, `override`, `virtual`, `package`, `interface`, `implements` **ne figurent pas** dans la liste réservée : ce sont des **identifiants valides** en Core 1.0 (le codegen les mangle). Conséquences normatives :
- `let ref = 1;`, `let dyn = 2;`, `let await = 3;` sont **acceptés** (identifiants).
- `dyn T` en position de type est un nom de type inconnu (`E-RES-UNKNOWN`), pas `E-UNSUPPORTED-TRAIT`.
- `x.await` est une méthode inconnue (`E-RES-UNKNOWN`), pas `E-UNSUPPORTED-ASYNC`.
- `ref x` en pattern est un motif de liaison `ref` suivi d'un token inattendu (`E-PARSE-EXPECTED`).

> `static` et `type` **sont réservés** (35 mots-clés, §5.1) mais non supportés à l'usage :
> un item `static` est `E-UNSUPPORTED-STATIC`, un alias de type `E-UNSUPPORTED-TYPE-ALIAS`
> (codes dédiés, Q-LEX-1), pas `E-UNSUPPORTED-TOKEN`.

### 5.4 Mots-clés d'items et de visibilité

`pub` est un modificateur de visibilité, pas un item. `test` est un modificateur de fonction (`test fn`). `const` introduit un item constant. Aucun autre modificateur n'existe en Core 1.0 (pas d'`unsafe fn`, pas d'`extern fn`, pas d'`async fn`).

## 6. Commentaires

| ID | Règle | Niveau |
|---|---|---|
| LEX-6.1 | `//` ouvre un commentaire de ligne jusqu'au terminateur de ligne (exclu). | MUST |
| LEX-6.2 | `/*` ouvre un commentaire de bloc, fermé par le premier `*/`. Les commentaires de bloc **ne s'imbriquent pas** : `/* a /* b */` se termine au premier `*/`. | MUST |
| LEX-6.3 | Un `/*` non fermé avant EOF produit `E-LEX-COMMENT`, span du `/*` ouvrant à EOF. | MUST |
| LEX-6.4 | Les commentaires et les blancs sont de la **trivia** : ils ne produisent pas de token et n'ont aucun effet sémantique. Ils sont conservés en trivia associée pour `fmt`/`doc`. | MUST |
| LEX-6.5 | `///` (doc externe) et `//!` (doc interne) sont des **commentaires de documentation** : trivia retenue pour `ravum-lang doc`, sans effet sémantique, sans influence sur la résolution ni le typage. | MUST |
| LEX-6.6 | `//!` n'est significatif qu'en tête de fichier ou de module inline ; ailleurs il reste un simple commentaire de doc, **sans diagnostic** (liste d'avertissements fermée). | SHOULD |
| LEX-6.7 | Les attributs `#[...]`, `#![...]` ne sont **pas** des commentaires : ils sont refusés par le parser (`E-UNSUPPORTED-ATTR`). | MUST |

```rv
// POSITIF
/// Calcule la norme. (doc)
fn norme(x: f64, y: f64) -> f64 { return math::sqrt(x * x + y * y); }
/* commentaire
   de bloc, non imbriqué */
```

```rv
// NÉGATIF — E-LEX-COMMENT (span: ligne 2 col 1 → EOF)
fn main() { }
/* jamais fermé
fn main() { }
```

```rv
// NÉGATIF — E-UNSUPPORTED-ATTR
#[derive(Debug)]
struct P { x: i32 }
```

## 7. Blancs et caractères autorisés

| ID | Règle | Niveau |
|---|---|---|
| LEX-7.1 | Les blancs sont `U+0020` (espace), `U+0009` (tabulation), `LF` et `CRLF`. Les séparateurs de trivia au sein d'une ligne sont sans signification syntaxique. | MUST |
| LEX-7.2 | Le lexer **MUST NOT** accepter : `U+000B` (VT), `U+000C` (FF), `U+0085` (NEL), `U+2028`/`U+2029`, ni tout autre caractère de contrôle C0 hors `\t`, `\n`, `\r` (dans CRLF). Chacun produit `E-UNSUPPORTED-TOKEN`. | MUST |
| LEX-7.3 | Une tabulation dans l'indentation est acceptée ; `fmt` la remplace par 4 espaces. Aucun diagnostic n'est émis : il n'existe pas de lint de formatage en Core 1.0. | SHOULD |
| LEX-7.4 | Les caractères Unicode non-ASCII sont autorisés **dans** les littéraux de chaîne et de caractère, et interdits partout ailleurs (identifiants : §4 ; commentaires : autorisés, ce sont des commentaires). | MUST |

## 8. Littéraux

### 8.1 Entiers

Formes : décimale, hexadécimale (`0x`), octale (`0o`), binaire (`0b`). Suffixe de type optionnel parmi `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 isize usize`.

| ID | Règle | Niveau |
|---|---|---|
| LEX-8.1 | Un séparateur `_` est autorisé **uniquement entre deux chiffres** du même littéral. Il est refusé en tête, en fin, juste après un préfixe (`0x_FF`), doublé (`1__0`) ou adjacent à un point/exposant (`1_.0`). Violation : `E-LEX-NUMBER`. | MUST |
| LEX-8.2 | Le préfixe `0x`/`0o`/`0b` exige **au moins un** chiffre de la base ; un chiffre hors base (`0b12`, `0o8`) ou un suffixe inconnu (`1z`) produit `E-LEX-NUMBER`. | MUST |
| LEX-8.3 | Le suffixe, s'il est présent, **MUST** être exactement l'un des 12 suffixes ci-dessus, sans séparateur et collé au littéral. Sinon `E-LEX-INT-SUFFIX` (span du suffixe) — distinct de `E-LEX-NUMBER`. | MUST |
| LEX-8.4 | Un littéral entier **MUST** tenir dans 128 bits non signés (`u128`) après application de la base. Au-delà : `E-LEX-INT-OVERFLOW` (« integer literal out of range »). | MUST |
| LEX-8.5 | La valeur du littéral est **non signée** à l'analyse lexicale ; l'appartenance au type (et le signe via `-` unaire) est vérifiée au typage : voir `E-TYP-LITERAL-RANGE` dans [`types.md`](types.md). | MUST |
| LEX-8.6 | Il n'existe **aucun** littéral octal implicite de style `0777` (zéro initial) : `0777` est le décimal `777`. | MUST |
| LEX-8.7 | Aucun séparateur de chiffres n'est autorisé dans un exposant flottant autre qu'entre chiffres (§8.2). | MUST |

```rv
// POSITIF
let a = 1_000_000;      // décimal, séparateurs
let b = 0xFF_FFu16;     // hex + suffixe
let c = 0b1010_0101u8;
let d = 0o755;
```

```rv
// NÉGATIF — E-LEX-NUMBER (séparateur doublé)
let a = 1__0;
// NÉGATIF — E-LEX-NUMBER (chiffre hors base)
let b = 0b12;
// NÉGATIF — E-LEX-INT-SUFFIX (suffixe inconnu `z`)
let c = 10z;
// NÉGATIF — E-LEX-NUMBER (séparateur après préfixe)
let d = 0x_FF;
```

### 8.2 Flottants

Formes acceptées (obligatoirement un chiffre **après** le point, pas de point final) :

```
FLOAT = DEC "." DEC [ EXP ] [ "f32" | "f64" ]
      | DEC EXP [ "f32" | "f64" ]
EXP   = ("e"|"E") ["+"|"-"] DEC
```

Exemples : `1.0`, `3.14f32`, `1e9`, `1.5e-3f64`, `1_000.5`.

| ID | Règle | Niveau |
|---|---|---|
| LEX-8.8 | `1.` (point sans chiffre ensuite) est **invalide** : `E-LEX-NUMBER`. Cela lève l'ambiguïté avec l'accès de champ entier `t.0`. | MUST |
| LEX-8.9 | Un `.` précédé d'un identifiant, de `)`, de `]` ou d'un littéral est le token `Dot` (accès de champ / appel de méthode) ; les chiffres qui suivent forment un littéral **entier** d'indice de champ. `t.0` = `t` `Dot` `0`. Aucune fusion `0.1` n'a lieu après un `Dot`. | MUST |
| LEX-8.10 | Un littéral flottant **MUST** être fini : un débordement vers `±inf` (`1e999`) produit `E-LEX-NUMBER`. Un sous-dépassement vers `0.0` (`1e-999`) est accepté silencieusement (valeur `0.0`, signe conservé). Un littéral qui ne tient pas dans le type flottant **inféré** (ex. `1e39` contraint en `f32`) est `E-TYP-FLOAT-LITERAL` (typage, [`types.md`](types.md)). | MUST |
| LEX-8.11 | Sans suffixe, un littéral flottant a le type par défaut `f64` ; le typage peut le contraindre à `f32` par le contexte ([`types.md`](types.md)). | MUST |
| LEX-8.12 | Il n'existe pas de littéraux `NaN`, `inf`, `-inf` : `NaN` et `INFINITY` sont **identifiants**, donc `E-RES-UNKNOWN` hors déclaration utilisateur. | MUST |

```rv
// POSITIF
let x = 1.0;        // f64
let y = 3.5f32;     // f32
let t = (1, 2);
let z = t.0;        // accès de champ entier, pas de flottant
```

```rv
// NÉGATIF — E-LEX-NUMBER (`1.` sans chiffre)
let x = 1.;
// NÉGATIF — E-LEX-NUMBER (débordement vers +inf)
let y = 1e999;
```

### 8.3 Caractères

`'c'` contient **exactement un** scalaire Unicode (échappement compris) : ni zéro, ni deux.

| ID | Règle | Niveau |
|---|---|---|
| LEX-8.13 | Un littéral de caractère vide (`''`), multiple (`'ab'`) ou multi-scalaire produit `E-LEX-CHAR`. | MUST |
| LEX-8.14 | Un `'` non fermé avant fin de ligne ou EOF produit `E-LEX-CHAR`. | MUST |
| LEX-8.15 | Un scalaire non-ASCII est autorisé (`'é'`, `'😀'`), encodé UTF-8 dans le fichier (donc 2 à 4 octets). | MUST |
| LEX-8.16 | Un échappement invalide ou non terminé produit `E-LEX-ESCAPE`. | MUST |
| LEX-8.17 | Un `\u{...}` doit désigner un **scalaire Unicode valide** : 0 ≤ v ≤ 0x10FFFF et v ∉ [0xD800,0xDFFF]. Un surrogat produit `E-LEX-CHAR` ; un dépassement ou une syntaxe invalide (`\u{}`, `\u{110000}`, `\u{D800}`) produit `E-LEX-ESCAPE`/`E-LEX-CHAR` selon le cas (table §8.5). | MUST |
| LEX-8.18 | Il n'existe **aucun** littéral de byte (`b'x'`, `b"..."`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| LEX-8.19 | Il n'existe **aucun** littéral de caractère « lifetime » (`'a`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| LEX-8.20 | `'` n'est **jamais** utilisé pour délimiter autre chose qu'un caractère en Core 1.0. | MUST |

```rv
// POSITIF
let a = '\n';
let b = 'é';
let c = '\u{1F600}';
let d = '\'';
```

```rv
// NÉGATIF — E-LEX-CHAR (deux scalaires)
let a = 'ab';
// NÉGATIF — E-LEX-CHAR (surrogat)
let b = '\u{D800}';
// NÉGATIF — E-UNSUPPORTED-TOKEN
let c = b'x';
// NÉGATIF — E-UNSUPPORTED-TOKEN
let d = 'static;
```

### 8.4 Chaînes

`"..."` contient une séquence de scalaires Unicode et/ou d'échappements. Les **retours à la ligne bruts** sont autorisés dans un littéral de chaîne et sont conservés tels quels (`LF` dans le contenu ; `CRLF` normalisé en `LF` par LEX-3.2 pour le contenu **et** les spans).

| ID | Règle | Niveau |
|---|---|---|
| LEX-8.21 | Un `"` non fermé avant EOF produit `E-LEX-STRING`. | MUST |
| LEX-8.22 | Un échappement invalide produit `E-LEX-ESCAPE`. | MUST |
| LEX-8.23 | `\xNN` **MUST** produire un octet ≤ 0x7F (ASCII) dans une chaîne, comme en Rust. `\xFF` produit `E-LEX-ESCAPE`. | MUST |
| LEX-8.24 | `\u{...}` suit LEX-8.17. | MUST |
| LEX-8.25 | Aucune chaîne brute (`r"..."`, `r#"..."#`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| LEX-8.26 | Aucune chaîne de bytes (`b"..."`, `br"..."`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| LEX-8.27 | Aucune continuation de ligne par `\` en fin de ligne (`E-LEX-ESCAPE`). | MUST |
| LEX-8.28 | Un littéral de chaîne a le type `&'static str`, noté `&text` dans les diagnostics ([`types.md`](types.md) §6). | MUST |
| LEX-8.29 | Le contenu d'une chaîne n'est **jamais** normalisé, interpolé, ni échappé implicitement. Il n'y a pas d'interpolation de chaînes en Core 1.0 (`"a{b}"` est littéral). | MUST |

```rv
// POSITIF
let s = "bonjour\n";
let t = "une ligne
sur deux lignes";      // LF brut conservé
let u = "\u{1F600}\x41";
```

```rv
// NÉGATIF — E-LEX-ESCAPE (\xFF > 0x7F dans une chaîne)
let s = "a\xFF";
// NÉGATIF — E-LEX-STRING
let t = "jamais fermé
// NÉGATIF — E-UNSUPPORTED-TOKEN
let u = r"brut";
```

### 8.5 Table complète des échappements

| Échappement | Sens | Valide en char | Valide en string |
|---|---|---|---|
| `\n` | LF `0x0A` | oui | oui |
| `\r` | CR `0x0D` | oui | oui |
| `\t` | TAB `0x09` | oui | oui |
| `\0` | NUL `0x00` | oui | oui |
| `\\` | `\` | oui | oui |
| `\'` | `'` | oui | oui |
| `\"` | `"` | oui | oui |
| `\xNN` | octet NN, **≤ 0x7F** | oui | oui |
| `\u{H...}` | scalaire, 1..6 chiffres hex | oui | oui |
| `\<LF>` | (interdit) | non | non |
| autre | `E-LEX-ESCAPE` | non | non |

| ID | Règle | Niveau |
|---|---|---|
| LEX-8.30 | Tout échappement absent de cette table est refusé (`E-LEX-ESCAPE`), y compris `\a`, `\e`, `\v`, `\?`, `\uXXXX` sans accolades. | MUST |

## 9. Normalisation et mapping des positions

| ID | Règle | Niveau |
|---|---|---|
| LEX-9.1 | Le lexer **MUST NOT** modifier le texte : aucune suppression de blancs, aucune conversion de fin de ligne, aucun retrait de commentaire n'affecte les offsets. | MUST |
| LEX-9.2 | Un `Span` est `{ file_id, start_byte, end_byte }` en demi-ouvert `[start_byte, end_byte)`, offsets **absolus en octets** du fichier original. | MUST |
| LEX-9.3 | La projection ligne/colonne est calculée à la demande : `line` = nombre de terminateurs (`LF`, ou `CRLF` compté 1) avant `start_byte`, + 1 ; `column` = nombre de **code units UTF-16** entre le début de la ligne et `start_byte`, + 1. Ce choix rend `column` directement compatible LSP. | MUST |
| LEX-9.4 | Les colonnes ne sont **jamais** exprimées en octets ni en points de code : la conversion est centralisée (voir [`diagnostics.md`](diagnostics.md) §5). | MUST |
| LEX-9.5 | `fmt` normalise `CRLF`→`LF` et un `LF` final ; c'est la **seule** normalisation de texte autorisée. `fmt` **MUST** être idempotent et préserver l'AST (invariant `F-DIAG-003`). | MUST |

## 10. Grammaire lexicale (EBNF)

```ebnf
source_file   = { trivia | token } EOF ;

trivia        = whitespace | line_comment | block_comment | doc_comment ;
whitespace    = " " | "\t" | "\n" | "\r\n" ;
line_comment  = "//" { any_except_eol } ( "\n" | "\r\n" | EOF ) ;
doc_comment   = ( "///" | "//!" ) { any_except_eol } ( "\n" | "\r\n" | EOF ) ;
block_comment = "/*" { any_char | "/*" } "*" "/" ;   (* non imbriqué : le premier "*/" ferme *)

token         = keyword | ident | int_lit | float_lit | char_lit | string_lit | punct ;

keyword       = "as" | "break" | "const" | "continue" | "crate" | "else" | "enum"
              | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let"
              | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "return"
              | "self" | "Self" | "static" | "struct" | "super" | "test" | "trait" | "true"
              | "type" | "unsafe" | "use" | "where" | "while" | "async" ;

ident         = ident_start { ident_cont } ;            (* ASCII seulement *)
ident_start   = "A".."Z" | "a".."z" | "_" ;
ident_cont    = ident_start | "0".."9" ;

int_lit       = ( dec_lit | hex_lit | oct_lit | bin_lit ) [ int_suffix ] ;
dec_lit       = dec_digit { [ "_" ] dec_digit } ;
hex_lit       = "0x" hex_digit { [ "_" ] hex_digit } ;
oct_lit       = "0o" oct_digit { [ "_" ] oct_digit } ;
bin_lit       = "0b" bin_digit { [ "_" ] bin_digit } ;
int_suffix    = "i8" | "i16" | "i32" | "i64" | "i128"
              | "u8" | "u16" | "u32" | "u64" | "u128"
              | "isize" | "usize" ;

float_lit     = ( dec_lit "." dec_digit { [ "_" ] dec_digit } [ exponent ] [ float_suffix ] )
              | ( dec_lit exponent [ float_suffix ] ) ;
exponent      = ( "e" | "E" ) [ "+" | "-" ] dec_lit ;
float_suffix  = "f32" | "f64" ;

char_lit      = "'" char_item "'" ;
char_item     = any_scalar_except( "'", "\\", LF, CR ) | escape ;
string_lit    = "\"" { string_item } "\"" ;
string_item   = any_scalar_except( "\"", "\\" ) | escape | LF ;
escape        = "\\" ( "n" | "r" | "t" | "0" | "\\" | "'" | "\""
                     | "x" hex_digit hex_digit
                     | "u{" hex_digit { hex_digit } "}" ) ;

punct         = "(" | ")" | "{" | "}" | "[" | "]" | "," | ";" | ":" | "::"
              | "." | ".." | "..=" | "->" | "=>"
              | "+" | "-" | "*" | "/" | "%" | "!" | "&" | "|" | "^"
              | "<<" | ">>" | "+="
              | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
              | "=" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
              | "?"
              | "&mut" ;   (* « &mut » est deux tokens « & » « mut » ; voir LEX-11.4 *)

dec_digit     = "0".."9" ;
hex_digit     = dec_digit | "a".."f" | "A".."F" ;
oct_digit     = "0".."7" ;
bin_digit     = "0" | "1" ;
any_except_eol= ? tout scalaire sauf LF et CR ? ;
```

**Note de mise en œuvre** : `&mut` n'est pas un token unique ; c'est `&` suivi de `mut`. `&mut` ne doit pas être fusionné, car `&mut` en position de type et `&` binaire doivent rester distinguables par le parser.

`!` est un token de ponctuation utilisable en **position de type** (type Never, voir [`types.md`](types.md) TYP-1.6) et en position préfixe (négation logique ou bit à bit). Les deux usages sont distingués par la position, comme `&` et `*`.

## 11. Règles de reprise après erreur

| ID | Règle | Niveau |
|---|---|---|
| LEX-11.1 | Le lexer est **total** : pour toute entrée d'octets, il termine. Aucune entrée ne provoque de panique, de boucle infinie ni d'allocation non bornée. | MUST |
| LEX-11.2 | Après `E-UNSUPPORTED-TOKEN`, le lexer avance d'un octet et émet un token `Error` ; l'analyse continue pour produire au plus 100 diagnostics par fichier (budget borné, déterministe). | MUST |
| LEX-11.3 | Après un littéral invalide, le lexer consomme jusqu'au prochain délimiteur sur la même ligne et émet un token `Error` ; il n'essaie pas de réinterpréter le contenu. | MUST |
| LEX-11.4 | La classification d'un token est **déterministe** et ne dépend d'aucun état du parser, à l'exception de LEX-8.9 (`Dot` puis entier). | MUST |
| LEX-11.5 | Deux exécutions du lexer sur les mêmes octets produisent la même séquence de tokens et de spans (déterminisme exigé par T3). | MUST |

## 12. Table des tokens

| Classe | Formes | Exemple |
|---|---|---|
| `Ident` | §4 | `point` |
| `Keyword` | §5.1 | `fn` |
| `IntLit` | §8.1 | `0xFF_FFu16` |
| `FloatLit` | §8.2 | `1.5e-3f64` |
| `CharLit` | §8.3 | `'é'` |
| `StringLit` | §8.4 | `"a\n"` |
| `Punct` | §10 | `::`, `->`, `..` |
| `Error` | §11 | — |
| `EOF` | fin de fichier | — |

## 13. Traçabilité des fonctionnalités

| FeatureId | Objet | Statut |
|---|---|---|
| `F-LEX-001` | Fichier UTF-8 (BOM de tête accepté et ignoré) | candidate-stable |
| `F-LEX-002` | Fins de ligne LF/CRLF | candidate-stable |
| `F-LEX-003` | Commentaires ligne/bloc non imbriqués | candidate-stable |
| `F-LEX-004` | Identifiants ASCII | candidate-stable |
| `F-LEX-005` | Mots-clés réservés | candidate-stable |
| `F-LEX-006` | Entiers base/suffixe/séparateurs | candidate-stable |
| `F-LEX-007` | Flottants IEEE (formes lexicales) | candidate-stable |
| `F-LEX-008` | Littéraux char + échappements | candidate-stable |
| `F-LEX-009` | Littéraux string + échappements | candidate-stable |
| `F-LEX-010` | Refus explicite Unicode/byte/raw/lifetime | candidate-stable |

## 14. Questions ouvertes

- **Q-LEX-1 (résolue par le Lead)** — `static` et `type` sont **réservés** (et refusés à l'usage : `E-UNSUPPORTED-STATIC` pour un item `static`, `E-UNSUPPORTED-TYPE-ALIAS` pour un alias de type, codes dédiés enregistrés dans [`diagnostics.md`](diagnostics.md) §3.1). `ref`, `dyn`, `await` restent des **identifiants valides** (mangling par le codegen), donc `dyn T` → `E-RES-UNKNOWN`, `x.await` → `E-RES-UNKNOWN`, `ref x` en pattern → `E-PARSE-EXPECTED`.
- **Q-LEX-2** — `effects` et `str` sont des mots-clés contextuels. Le design ne les liste pas. Confirmer le statut contextuel plutôt que réservé.
- **Q-LEX-3** — Les continuations de ligne `\` en fin de chaîne (Rust) sont refusées (LEX-8.27). Si des corpus multi-lignes en ont besoin, cette décision doit être révisée.
- **Q-LEX-4** — Le refus de `1.` (LEX-8.8) et la règle `Dot` (LEX-8.9) sont cohérents entre eux mais diffèrent de Rust sur `1.f32` ; confirmer.
- **Q-LEX-5** — `\xNN` limité à l'ASCII dans les chaînes (LEX-8.23) suit Rust ; autoriser `\xNN` arbitraire imposerait de définir la validité UTF-8 **au lexer** plutôt qu'au runtime. Maintenir le refus sauf besoin démontré.
- **Q-LEX-6** — Faut-il un littéral brut multi-lignes de style `r#"..."#` pour les chemins Windows dans les corpus ? Refusé pour l'instant (`E-UNSUPPORTED-TOKEN`) ; cela force `"C:\\x"`.
- **Q-LEX-7** — Les commentaires de doc `///` sont-ils normatifs (contrat de documentation) ou purement informatifs ? Ce document les traite comme trivia sans effet sémantique ; `ravum-lang doc` les consomme sans garantie de format.
- **Q-LEX-8 (résolue)** — La liste des avertissements est fermée à `W-RES-UNUSED-IMPORT` et `W-OWN-UNUSED-VARIABLE` ; aucun lint de style/format n'existe. `ravum-lang lint` ne peut donc signaler que ces deux codes. À étendre seulement par décision explicite.

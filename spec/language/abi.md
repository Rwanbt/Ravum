# Ravum Core 1.0 — ABI public et contrats d'émission

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §1, §4, §5.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** (notamment pas ADR-017/018 sur les backends ni ADR-021 sur le hot swap). Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).

## 1. Décision : aucun ABI stable en Core 1.0

| ID | Règle | Niveau |
|---|---|---|
| ABI-1.1 | Core 1.0 **ne définit aucun ABI public stable** : ni convention d'appel, ni disposition mémoire, ni nommage de symboles, ni format de bibliothèque. | MUST |
| ABI-1.2 | Aucun artefact binaire n'est **destiné** à être lié par un autre langage, un autre compilateur, ou une version différente de Ravum. | MUST |
| ABI-1.3 | Le seul « ABI » existant est **interne et privé** : la forme du fichier Rust généré, qui n'est pas un contrat et peut changer à chaque version. | MUST |
| ABI-1.4 | Toute affirmation d'ABI stable ou de compatibilité binaire en Core 1.0 est **fausse** et doit être refusée dans une revue. L'interopérabilité native se limite au sous-ensemble FFI scalaire expérimental (§3) : pas de layout exposé, pas de symbole Ravum exporté, aucune garantie de version. | MUST |

### Pourquoi

1. **Aucun besoin réel** : la seule frontière native est une **FFI minimale expérimentale** (`extern "C"` avec primitives scalaires, appelable dans `unsafe`), sans plugin, sans bibliothèque précompilée, sans registre public, et sans ABI Ravum stable. Les packages sont compilés **depuis la source** dans un seul programme ([`modules.md`](modules.md) MOD-6.6).
2. **Le layout n'est pas observable** : sans pointeur, `size_of`, `transmute`, ni type FFI composé, aucun programme ne peut dépendre d'une disposition d'octets ([`types.md`](types.md) TYP-12). Figer un ABI n'apporterait aucune garantie utilisable et créerait une dette irréversible.
3. **Le backend est provisoire** : la voie B (`rustc` comme générateur de code) est retenue pour cette itération, la voie C (IR propre → Cranelift) est expérimentale. Un ABI figé maintenant lierait le langage à un backend non ratifié.
4. **Fail-closed** : un ABI implicite (« ça marche par accident ») est plus dangereux qu'une absence déclarée. L'absence est explicite et testable.
5. **Le hot swap n'est pas livré** : sans activation à chaud (T8 = `UNSUPPORTED`), il n'y a pas de contrainte de compatibilité de code chargé.

## 2. Ce qui est réellement garanti

| ID | Garantie | Niveau |
|---|---|---|
| ABI-2.1 | **Sémantique du source** : pour un programme accepté, le comportement observable (E/S, `panic`, code de sortie, valeurs rendues) est celui spécifié dans [`expressions.md`](expressions.md), [`stdlib.md`](stdlib.md) et [`core-machine.md`](core-machine.md). | MUST |
| ABI-2.2 | **Déterminisme du compilateur** : mêmes sources + même configuration ⇒ mêmes diagnostics, même binaire fonctionnellement équivalent, même code Rust généré (hors commentaires de mapping si le chemin change). | MUST |
| ABI-2.3 | **Auto-contenu** : le code généré est un **unique fichier Rust autonome**, prélude inclus, sans dépendance hors `std`, compilable par `rustc --edition 2021` (design §4). | MUST |
| ABI-2.4 | **Auditabilité** : le code généré contient, avant chaque item/instruction, un commentaire `// rv: <fichier>:<ligne>` permettant de relier le Rust émis au source Ravum ; `--emit-rust FILE` l'écrit. | MUST |
| ABI-2.5 | **Pas de `unsafe` généré hors frontière FFI** : pour un programme **sans `extern`**, le backend **MUST NOT** émettre de bloc `unsafe`, d'appel `unsafe`, ni de pointeur brut. Pour un programme avec `extern`, il émet **exactement** un wrapper `unsafe` par fonction `extern` (sous `deny(unsafe_code)`), et rien d'autre. | MUST |
| ABI-2.6 | **Pas de contournement du checker** : le backend ne « répare » aucune erreur, n'insère aucune conversion implicite supplémentaire, ne réintroduit aucun effet non déclaré. | MUST |
| ABI-2.7 | **Pas d'API privée de `rustc`** : le backend invoque `rustc` en **CLI** uniquement, avec des options documentées ; aucune dépendance à `rustc_*` (INV-11). | MUST |
| ABI-2.8 | **Tailles des primitifs** : `i8..i128`, `u8..u128`, `f32`, `f64`, `bool`, `char` ont les tailles de [`types.md`](types.md) TYP-1.1. `isize`/`usize` suivent la cible. | MUST |
| ABI-2.9 | **Aucune garantie de layout** pour les types composés, les enums, `Vec`, `String`, `Option`, `Result` (TYP-12). | MUST |
| ABI-2.10 | **Aucune garantie de nom** : les symboles générés sont internes, non documentés, non stables. | MUST |

```rv
// POSITIF — seul contrat : la sémantique observable
fn main() { io::println("hello"); }        // affiche "hello\n", code 0
```

```text
// NÉGATIF — aucune de ces affirmations n'est vraie en Core 1.0 :
// « une struct Point est { f64, f64 } contiguë »
// « une fonction Ravum est appelable depuis C »
// « deux versions de Ravum peuvent partager un module compilé »
```

## 3. Frontières : ce qui est accepté et ce qui est refusé

| Frontière | Forme | Code |
|---|---|---|
| FFI minimale (acceptée) | `extern "C" { fn f(primitives) -> primitives; }`, appel dans `unsafe` | acceptée ; appel hors `unsafe` → `E-UNSAFE-CONTEXT` |
| FFI hors sous-ensemble | `#[no_mangle]`, `#[link]`, ABI non-C, types composés/pointeurs dans la signature, `extern "C" fn` avec corps | `E-UNSUPPORTED-ATTR`, `E-UNSUPPORTED-ABI`, `E-FFI-TYPE`, `E-UNSUPPORTED-EXTERN` |
| Unsafe | `unsafe fn`, pointeurs bruts ; le **bloc** `unsafe { }` est accepté (effet `unsafe`, aucune capacité nouvelle) | `E-UNSUPPORTED-UNSAFE` |
| ABI explicite | `#[repr(C)]`, `#[repr(u8)]` | `E-UNSUPPORTED-ATTR` |
| Layout observable | `size_of`, `align_of`, `offset_of`, `transmute` | `E-RES-UNKNOWN` |
| Bibliothèque précompilée | `.rlib`, `.so`, `.dll` en dépendance | `E-MOD-DEPENDENCY` |
| Linker maison | édition de liens personnalisée | non exposé (aucune option CLI) |
| Build script | `build.rs`, `[build]` | `E-MOD-BUILD-SCRIPT` |
| Plugin / macro procédurale | `proc_macro`, `macro_rules!` | `E-UNSUPPORTED-MACRO` |
| Hot swap de code | chargement d'un module compilé | `UNSUPPORTED` (T8) |

| ID | Règle | Niveau |
|---|---|---|
| ABI-3.1 | Toute frontière native **hors** du sous-ensemble FFI minimal est refusée au parse ou à la résolution, jamais ignorée : ABI non-C, types composés/pointeurs dans une signature `extern`, attributs de linkage. | MUST |
| ABI-3.2 | Le refus est un diagnostic explicite : `E-UNSUPPORTED-*` pour les formes syntaxiques, `E-FFI-TYPE` pour un type non FFI-sûr, avec la justification « aucune ABI stable en Core 1.0 ». | MUST |
| ABI-3.3 | Le code généré ne contient **aucun** `extern`, `#[repr]`, ni `unsafe` **pour un programme sans `extern`** ; pour un programme avec `extern`, il contient uniquement les déclarations `extern "C"` et un bloc `unsafe` par wrapper, vérifiable par `--emit-rust` et par l'audit backend. | MUST |

## 4. Contrat d'émission (backend)

```text
codegen::generate(&CheckedProgram, GenOptions) -> Result<String, CodegenError>
```

| ID | Règle | Niveau |
|---|---|---|
| ABI-4.1 | L'entrée du backend est un `CheckedProgram` **valide** : aucun identifiant non résolu, aucun type manquant, aucun effet non calculé (design §4). Un `CheckedProgram` invalide est un **bug interne**, jamais une entrée utilisateur. | MUST |
| ABI-4.2 | La sortie est un `String` UTF-8, **un seul fichier**, sans dépendance hors `std`. | MUST |
| ABI-4.3 | Le prélude runtime (affichage d'`Error`, hook de panic, `main -> Result`) est **inclus** dans le fichier généré ; il n'y a pas de bibliothèque runtime séparée. | MUST |
| ABI-4.4 | L'ordre d'évaluation spécifié (EXPR-1) est **préservé** : le backend ne peut pas réordonner des appels à effet, ni fusionner des opérations flottantes (pas de FMA, pas de réassociation). | MUST |
| ABI-4.5 | Le backend **MUST** émettre les contrôles de débordement, division par zéro, décalage et bornes d'index, ou compiler avec `-C overflow-checks=on` et des accès vérifiés. Aucune optimisation ne peut les supprimer. | MUST |
| ABI-4.6 | Le panic est un **arrêt contrôlé**, sans destructeurs : en mode `run`, hook qui imprime `panic: <msg>` puis `exit(1)` ; en mode `test`, le runner capture l'*unwind* et marque le test en échec. Le hook est installé **aussi en release** (design §3.8). | MUST |
| ABI-4.7 | Le code généré **MUST** compiler sans avertissement bloquant ; un avertissement `rustc` est un défaut à corriger, pas à masquer (`#[allow]` interdit). | MUST |
| ABI-4.8 | Le temps de compilation n'est pas contractuel ; aucun chiffre n'est publié. | MUST |
| ABI-4.9 | `--emit-rust` est un **outil d'audit**, pas une API : le contenu exact n'est pas stable et ne doit pas être parsé par des outils tiers. `--emit-ir` n'émet **pas** d'IR (la voie B n'en a pas) : c'est un **dump de faits de typage** de débogage, non stable et non contractuel (Q-ABI-4). | MUST |

```rv
// POSITIF — source, et code généré attendu (extrait, non contractuel)
fn main() { io::println("ok"); }

// // rv: src/main.rv:2
// fn main() { ravum_io_println("ok"); }
```

```text
// NÉGATIF — le backend ne doit jamais produire :
// unsafe { ... }                 (sauf le wrapper d'une déclaration `extern`, un par fonction)
// #[repr(C)]
// extern "C" fn ...              (le code généré ne contient que des déclarations `extern`, pas de définition)
// a + b + c  réassocié en (a + c) + b
```

## 5. Procédure pour un futur ABI versionné

Aucune de ces étapes n'est franchie ; elles sont listées pour que l'absence soit une **décision**, pas un oubli.

| Étape | Contenu | Prérequis |
|---|---|---|
| 1 | Décision humaine (ADR accepté) : cas d'usage réel démontré (FFI, plugin, hot swap) | ADR-009/017/021 acceptés |
| 2 | Spécifier la **disposition** des types exposés (`repr` stable, enums, `Option`/`Result`) | modèle de layout observable |
| 3 | Spécifier la **convention d'appel**, le nommage, la gestion d'erreur à la frontière | tests croisés |
| 4 | Spécifier la **négociation de version** (ABI majeure/mineure, symboles, sonde) | fail-closed |
| 5 | Ajouter `unsafe`/`extern` **audités**, avec frontière explicite et manifeste | ADR-009, P0-C |
| 6 | Conformance **différentielle** contre un oracle indépendant, versions croisées | INV-12 |
| 7 | Publier un `abi_version` dans `--json` et dans les artefacts | schéma `schema_version` |
| 8 | Garantir une **durée de compatibilité** et une procédure de retrait | gouvernance |

| ID | Règle | Niveau |
|---|---|---|
| ABI-5.1 | Tant que les étapes 1–6 ne sont pas faites, aucun `abi_version` n'est exposé et aucun engagement n'est pris. | MUST |
| ABI-5.2 | Un futur ABI **MUST** être versionné et refusé (`fail-closed`) en cas d'incompatibilité, jamais « meilleur effort ». | MUST |
| ABI-5.3 | Un ABI ne peut pas être introduit sans que le layout devienne **observable** et spécifié (contradiction actuelle avec TYP-12). | MUST |

## 6. Risques et limites

| ID | Limite | Niveau |
|---|---|---|
| ABI-6.1 | Un bug du compilateur ou du backend peut produire un comportement non spécifié ; l'absence d'`unsafe` au source ne prouve pas l'absence de bug (INV-12). | MUST |
| ABI-6.2 | Le binaire produit dépend de la version de `rustc` et de la plateforme ; aucune reproductibilité bit-à-bit du binaire n'est revendiquée. | MUST |
| ABI-6.3 | « Aucun ABI » n'est pas « aucune compatibilité » : la compatibilité **source** est traitée dans [`compatibility.md`](compatibility.md). | MUST |
| ABI-6.4 | Aucune preuve n'est apportée ici : ce document est un contrat de refus, pas un théorème de sûreté. | MUST |

## 7. Questions ouvertes

- **Q-ABI-1** — Faut-il exposer `--emit-rust` comme **stable** pour les audits de sécurité ? Recommandation : non (ABI-4.9), mais le documenter comme outil de revue.
- **Q-ABI-2** — Le hot swap (T8) est `UNSUPPORTED`. Si un ABI de module est un jour nécessaire, il faudra d'abord un modèle de layout **et** une procédure d'activation atomique (ADR-021). Aucun travail ne doit commencer avant décision.
- **Q-ABI-3** — Un futur ABI devra définir le comportement de `String`/`Vec` à la frontière (propriété, allocateur). Non commencé, volontairement.
- **Q-ABI-4 (résolue par le Lead)** — `--emit-ir` est un **dump de faits de débogage**, pas une IR et pas un contrat. `ravum-lang conformance` n'existe **pas** : le harnais est le binaire séparé `ravum-conformance` ([`features.md`](features.md), `conformance/`).

# T13 étape 3 — vérification incrémentale d'un paquet, faits complets, LSP (2026-11-20)

> Gate **T13** (ADR-012/020, V1), décision V1-T2. État : `TESTED` par oracle,
> **pas** `PROVED`.

## Objectif

L'étape 2 ne réutilisait que des diagnostics, et sur un seul fichier.
L'étape 3 vise trois choses :
- couvrir un paquet de plusieurs fichiers ;
- produire un `CheckedProgram` **complet**, utilisable par le survol, la
  navigation, la génération de code et le JIT ;
- servir le LSP à chaque frappe.

## Identifiants de nœuds stables

Un `NodeId` porte le `FileId` dans ses 32 bits de poids fort et un compteur
propre au fichier dans les 32 autres. Le loader réutilise le parse d'un
fichier dont le texte et le `FileId` n'ont pas changé, et le LSP garde la
table `FileId` de sa session. **Tous les `NodeId` d'un fichier inchangé sont
donc identiques** d'une analyse à l'autre ; aucune renumérotation n'est
nécessaire.

Le parseur numérote dans l'ordre de lecture et donne son identifiant à un
item après ses enfants. Les nœuds d'une fonction d'identifiant `X` sont donc
l'intervalle `]item précédent du fichier, X]`.

## Modification

1. **`Facts::merge_reused`** (`ravum-lang-sema`) :
   - recopie, pour les nœuds des corps réutilisés, les faits de la
     vérification précédente : types, résolutions, appels, méthodes, faits
     d'ownership ;
   - les identifiants de liaison, numérotés dans l'ordre de vérification,
     sont ré-internés par (position, nom) ;
   - la structure est déstructurée champ par champ : un champ ajouté plus tard
     est une erreur de compilation, jamais un fait périmé oublié.
2. **`ravum-lang-incr::package::check_package`**. Un corps est réutilisé
   seulement si toutes ces conditions tiennent :
   - son fichier est inchangé ;
   - le corps est en cache depuis une vérification sans erreur ;
   - tout ce que ses faits désignent (cibles d'appel, items résolus, types
     utilisateur) est dans un fichier inchangé ;
   - aucun de ses identifiants ne nomme un item d'un fichier édité dont
     l'interface a changé, est apparu ou a disparu.
3. **Fail-closed** : tout est revérifié quand
   - il n'y a pas d'état précédent complet ;
   - une empreinte ne peut pas être planifiée, ou un item structurel (`use`,
     `mod`) a changé ;
   - une erreur tombe hors de toute fonction ;
   - un statut `&'static str` change.
4. **LSP** : `Analysis::run_incremental`, avec un état par racine de paquet
   dans le serveur ; chaque `didChange` l'utilise.

## Tests

| Test | Résultat |
|---|---|
| `ravum-lang-incr/tests/package_cold_warm.rs` : 3 paquets générés de 4 fichiers (structs, enums, appels entre modules, `use crate::…`, `String`), 81 éditions (corps valide, corps invalide, double déplacement, signature, renommage, type de champ), puis retour à la base depuis l'état chaud | **diagnostics identiques** sur toutes ; **faits complets identiques** (normalisés sur les identifiants de liaison) sur les 24 éditions sans erreur ; retours identiques à la base ; 366 corps réutilisés, 282 revérifiés |
| trouvé par l'oracle | `fn_used_imports` absent pour un corps réutilisé : corrigé |
| `ravum-lang-lsp/tests/incremental.rs` : paquet à 2 fichiers, 5 éditions dans `main.rv` (dont erreur de type et nom inconnu, puis retour) | analyse incrémentale = analyse complète (diagnostics et types d'expressions) |
| étape 2 (`cold_warm.rs`, un seul fichier) et étape 1 (`oracle.rs`) | inchangées, vertes |

## Limites

- **Grain** : seuls les corps des fichiers non édités sont réutilisés. Dans le
  fichier édité, tout est revérifié, car ses identifiants peuvent avoir
  glissé.
- **Après une vérification en erreur**, la suivante est complète : les faits
  d'une vérification échouée ne sont pas gardés.
- **`watch --backend jit`** travaille sur un seul fichier : il n'utilise pas
  encore cet état. C'est le cas d'un fichier unique, couvert par l'étape 2
  (diagnostics seulement).
- **Oracle** : corpus généré (3 formes de paquet) ; le corpus réel
  multi-fichiers ne compte que 9 paquets, surtout des cas d'erreur.

## Étape 4 (2026-11-20) — réutilisation dans le fichier édité, `watch` branché

**Réutilisation dans le fichier édité.** Le parseur numérote les nœuds dans
l'ordre de lecture et donne son id à un item après ses enfants : les items qui
se terminent **avant le premier octet modifié** du fichier (marge d'un octet
pour le lexer) gardent leurs `NodeId` et leurs spans. `bound[f]` est le plus
grand id d'item de ce préfixe. Un corps du fichier édité est réutilisé si son
id est ≤ `bound[f]`, si chaque nœud auquel ses faits renvoient est dans un
fichier inchangé ou ≤ `bound` de son fichier (le cache garde désormais, par
fichier, l'id **maximal** référencé au lieu du seul fichier), si ses appelés le
sont aussi, et si la règle de noms (interfaces changées) le permet.

**`watch` (backend `rustc`)** garde une `IncrementalSession` (cache de parse,
table `FileId` stable, état de paquet) et vérifie chaque reconstruction avec
`check_package` ; les faits étant complets, le build qui suit est inchangé.
Ligne affichée : `[ravum] check: N bodies reused, M checked[ (full: raison)]`.

| Test | Résultat |
|---|---|
| `package_cold_warm.rs`, 117 éditions (ajout des éditions « item ajouté en fin », « item ajouté en tête » — tous les ids du fichier glissent —, « item ajouté invalide ») | diagnostics **et** faits identiques au contrôle complet sur toutes ; 693 corps réutilisés dont **84 dans des fichiers édités** (assertion > 0), 279 revérifiés |
| `watch_continuity.rs::a_rebuild_reuses_the_unchanged_function_bodies` | 1ʳᵉ vérification complète (`first check`) ; après l'édition de `main.rv` : ≥ 3 corps réutilisés (`twice`, `thrice` du module intact, `helper` avant le changement), **1** revérifié (`main`) ; le programme relancé affiche `v2` |

Limites restantes : un corps **après** le premier changement de son fichier
est revérifié (pas de remise en correspondance des ids glissés) ;
`watch --backend jit` (fichier unique) garde l'étape 2 ; après une
vérification en erreur, la suivante est complète.

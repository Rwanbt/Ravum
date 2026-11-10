# Brief d'audit externe — ce qu'un auditeur indépendant doit attaquer

> **Statut : document de cadrage, écrit par l'implémenteur.** Il n'est **pas** un
> audit : il dit ce qu'un auditeur doit faire pour que `T4`, `T10` et `T12`
> puissent sortir de `BLOCKED`. Il est donc à lire comme une **cible**, pas comme
> une preuve — et l'auditeur doit suspecter les zones qu'il désigne, pas les
> croire.

## 1. Pourquoi ce document existe

Les gates `T4` (sécurité mémoire, ownership, effets), `T10` (qualification et
audits) et `T12` (dogfood et Go/No-Go) exigent un **audit humain indépendant**.
Un audit mené par un agent, même un autre agent, reste un audit interne. Tant que
personne n'a ouvert ce dossier, ces trois gates restent `BLOCKED`, quel que soit
le nombre de tests verts.

Le reste est fait : 323 cas de conformance, 1020 tests, campagnes de fuzz
rejouées, Miri, repro-bilité bit-à-bit, manifeste de provenance. Ce dossier
existe pour que l'auditeur n'ait pas à refaire ce travail, et puisse consacrer
son temps à ce qui n'a **pas** été prouvé.

## 2. Périmètre exact

| Inclus | Exclu |
|---|---|
| `crates/ravum-lang-{syntax,sema,codegen,cli,lsp}` | l'orchestration de collaboration (`ravum-collab-*`, `ravum-unit-graph`) |
| le code Rust émis par la voie B et son exécution | la voie C (n'existe pas) |
| la frontière `unsafe`/FFI telle que spécifiée | macOS, ARM, ABI stable |
| le corpus `conformance/cases/` et ses limites | la performance temps réel |

## 3. Ce qui est déjà prouvé — et ne demande pas d'audit

| Élément | Commande | Résultat attendu |
|---|---|---|
| Suite workspace | `cargo test --workspace --all-targets -- --test-threads=2` | 82 suites, 1020 passés, 0 échec, 2 ignorés |
| Conformance | `cargo run -p ravum-conformance -- run` | 323 cas, 322 PASS / 0 FAIL / 1 SKIP |
| Traçabilité des règles | `python tools/features_coverage.py --strict` | 187/187 règles testables |
| Codes de diagnostic | `python tools/registry_audit.py` | 112 codes, 1 conflit documenté |
| Fuzz lexer/parser | `RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored` | 0 échec |
| Fuzz sema | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored` | 0 échec |
| Miri | job CI *Linux qualification* | 159 tests, 0 échec |
| Reproductibilité | `pwsh -File scripts/repro-check.ps1` | deux binaires identiques |
| Provenance | `pwsh -File scripts/provenance.ps1 -Verify` | PASS (non signé : D9) |

Un audit qui se contente de rejouer ces commandes ne prouve rien de neuf.

## 4. Où attaquer — liste dérivée des défauts réellement trouvés

Chaque ligne ci-dessous est un endroit où **l'implémentation était fausse il y a
peu** et a été corrigée. La probabilité qu'un défaut voisin subsiste est élevée :
c'est là qu'il faut chercher.

| Défaut corrigé | Classe de risque à sonder |
|---|---|
| `assert_eq` comparait sans exiger un type comparable (`E-TYP-EQ`) | **autres builtins qui ne vérifient pas leur contrat** : `println` (format), `panic`, `to_string`, `min`/`max` sur types hétérogènes |
| `Vec<T>::clone` accepté pour `T` mobile | **autres méthodes qui supposent `Copy`** sans le vérifier (`sort`, `contains`, `resize`, …) |
| `&mut self` appelé sur une place immuable | **règles d'ownership non vérifiées ailleurs** : réassignation d'un champ via une référence partagée, méthodes sur temporaires |
| `mut` sur un paramètre accepté puis ignoré | **modificateurs acceptés et ignorés** : `pub` sur variante, `mut` sur motif, attributs |
| plages `0..3` hors `for` reclassées en erreur de type | **erreurs de phase** : diagnostics émis par la mauvaise phase (`E-TYP-*` là où la spec attend `E-PARSE-*`) |
| `size_of::<i32>()` attendu en `E-RES-UNKNOWN` mais inatteignable | **exigences de spec inatteignables** : chercher les règles dont le code exigé ne peut pas être produit |
| 17 codes émis hors du registre §3 | **contrat de diagnostic** : `python tools/registry_audit.py` en trouve encore un (`W-TYP-UNREACHABLE`, interdit par DIAG-1.6) |
| `'a` produisait quatre diagnostics en cascade | **reprise après erreur** : toute construction non supportée produit-elle un seul code ? |
| `r"…"` produisait deux tokens | **frontière lexer/parser** : les formes non supportées sont-elles reconnues assez tôt ? |
| `W-OWN-UNUSED-VARIABLE` est déclaré par DIAG-1.6 et **jamais émis** | **capacités annoncées et absentes** : vérifier chaque ligne du registre §3 et chaque warning de DIAG-1.6 contre une émission réelle |

## 5. Questions auxquelles l'auditeur doit répondre

1. Existe-t-il un programme **accepté** par `check` qui produit du Rust émis
   invalide, ou un comportement différent de la spécification ?
2. Existe-t-il un programme **refusé** alors que la spécification l'autorise ?
3. La frontière `unsafe`/FFI (ADR-009, D7) est-elle correctement isolée : le
   code émis hors `extern "C"` contient-il `unsafe`, `transmute` ou un pointeur
   brut ?
4. Les diagnostics sont-ils **déterministes** (même entrée ⇒ même liste, même
   ordre) et toujours munis d'un code du registre ?
5. Le borrow checker refuse-t-il ce qu'il doit refuser sur des cas **absents du
   corpus** ? (`crates/ravum-lang-sema/tests/own_audit.rs` en contient 85 : en
   écrire de nouveaux, pas rejouer ceux-là.)

## 6. Format attendu du rendu

Un finding = programme minimal + commande + sortie observée + règle de spec
citée (`spec/language/<fichier>.md#<ancre>`) + sévérité (`critical`, `high`,
`medium`, `low`). Un finding sans programme reproductible n'est pas un finding.

Les défauts trouvés devront devenir des cas de conformance **avant** d'être
corrigés, pour que la régression soit permanente.

## 7. Ce que l'auditeur ne trouvera pas ici

- Aucune garantie ABI stable, aucun support macOS/ARM.
- Aucune preuve de performance (les chiffres de `PERFORMANCE.md` sont des
  observations, pas des garanties).
- Aucune signature de release : le manifeste de provenance est **non signé**
  (décision D9 non prise).
- Aucune preuve d'absence de bug : le corpus est fini, le fuzz est déterministe,
  et l'absence d'échec n'est pas l'absence de défaut.

# Serveur de langage Ravum — qualification LSP

> **Statut : `TESTED` (client LSP réel sur le protocole), `PARTIAL` pour
> l'intégration éditeur.** Ce document décrit exactement ce qui est vérifié, par
> quelle commande, et ce qui ne l'est pas. Aucun `PROVED`, aucun « compatible
> VS Code » revendiqué sans session VS Code réelle.

## 1. Ce qui est implémenté

Binaire : `ravum-lang lsp` (`crates/ravum-lang-lsp`). Transport : JSON-RPC 2.0
sur **stdio** avec le *base protocol* LSP (`Content-Length`). Rien d'autre que
des octets de protocole n'est écrit sur `stdout` ; les erreurs vont sur `stderr`.

| Capacité | État | Preuve |
|---|---|---|
| `initialize` / `initialized` / `shutdown` / `exit` | Implémenté | `tests/lsp_client.rs::initialize_advertises_a_real_language_server` |
| Synchronisation **incrémentale** (`change: 2`) | Implémenté | `incremental_change_is_applied_to_the_buffer` |
| Buffers **non sauvegardés** analysés depuis la mémoire | Implémenté | `unsaved_buffer_is_analysed_without_touching_the_file` (le fichier disque est relu et comparé) |
| `publishDiagnostics` (syntaxe + sémantique, mêmes codes que `check`) | Implémenté | `unsaved_buffer_is_analysed_without_touching_the_file` |
| `textDocument/hover` | Implémenté | `hover_shows_the_builtin_signature` |
| `textDocument/definition` | Implémenté | `definition_and_references_find_the_declaration` |
| `textDocument/references` | Implémenté | idem |
| `textDocument/rename` | Implémenté (depuis un usage **et** depuis la déclaration) | `rename_rewrites_declaration_and_uses` |
| `textDocument/prepareRename` | Implémenté (requis par VS Code) | `navigation_from_the_declaration_name_works` |
| Oracle **cold vs warm** | Les éditions incrémentales convergent vers l'analyse froide du même texte | `warm_incremental_edits_match_a_cold_analysis` |
| `textDocument/completion` | Implémenté | `symbols_completion_and_formatting_answer` |
| `textDocument/documentSymbol` | Implémenté | idem |
| `workspace/symbol` | Implémenté | requête couverte par le dispatch ; pas d'assertion dédiée |
| `textDocument/formatting` | Implémenté | `symbols_completion_and_formatting_answer` |
| Annulation (`$/cancelRequest`) | Accepté, sans effet | analyse synchrone, voir §4 |
| Synchronisation des fichiers non ouverts (`didChangeWatchedFiles`) | Ré-analyse | sans assertion dédiée |

## 2. Ce qui n'est **pas** implémenté

Le serveur répond par une **erreur JSON-RPC** (`-32601 method not found`), jamais
par un succès simulé :

- `textDocument/codeAction`
- `textDocument/semanticTokens`
- `textDocument/inlayHint`
- `textDocument/signatureHelp`
- `textDocument/prepareCallHierarchy`
- tout `workspace/applyEdit` (le serveur ne modifie jamais de fichier)

Preuve que l'absence est explicite : `unknown_request_returns_method_not_found`.

## 3. Décisions de conception et leurs conséquences

1. **Un seul analyseur.** Le serveur appelle `sema::loader` + `sema::check`, les
   mêmes points d'entrée que le CLI. Un diagnostic d'éditeur et un diagnostic
   `ravum-lang check` ne peuvent pas diverger par construction.
2. **Overlays, pas d'écriture.** Les buffers ouverts passent par
   `LoadOptions::overrides` (ajouté pour cela) : le texte vit en mémoire, le
   fichier n'est jamais réécrit, aucun fichier temporaire n'est créé.
3. **Analyse synchrone de paquet entier.** Core 1.0 impose une seule racine de
   crate et aucun macro-traitement : un paquet est petit et l'analyse complète
   prend quelques millisecondes. Conséquence : aucun résultat périmé ne peut être
   publié, puisqu'il n'y a pas deux analyses concurrentes. Limite : pas de
   scalabilité à un très gros espace de travail en un seul paquet.
4. **Diagnostics identiques supprimés.** Un `publishDiagnostics` dont l'ensemble
   est inchangé n'est pas réémis. Un client ne doit donc pas attendre une
   notification après chaque frappe.
5. **Refus plutôt que devinette.** Sans fait du checker pour la position du
   curseur, la fonctionnalité renvoie `null` au lieu d'une destination plausible
   mais fausse.

## 4. Limites explicites

- **Locales** : `references`/`rename` sur une variable locale ne sont proposés
  que si le nom est déclaré **exactement une fois** dans le fichier. Core 1.0
  n'a pas de table de résolution par liaison ; avec masquage (shadowing), un
  rename par nom pourrait toucher la mauvaise liaison, donc il est **refusé**.
- **Définitions inter-paquets** : une définition qui pointe dans un autre paquet
  n'est localisée que si le fichier cible existe à son chemin d'affichage
  relatif au paquet. Sinon le serveur renvoie `null` (pas de `Location` fausse).
- **Encodage** : `positionEncoding: utf-16` (défaut LSP). Les colonnes sont
  comptées en unités UTF-16, pas en octets ; c'est couvert par les tests
  `convert::utf16_columns_count_code_units_not_bytes`.
- **Annulation** : `$/cancelRequest` est accepté puis ignoré, aucune requête ne
  survivant à son message.
- **Intégration éditeur réelle** : **testée** par le harnais `tools/vscode-ravum`
  dans un vrai processus VS Code (`docs/qualification/VSCODE.md` : 10/10 au
  repos et 10/10 sous `cargo test --workspace` concurrent ; job CI
  `vscode-e2e`). Ce qui reste hors preuve : cohabitation avec d'autres
  extensions, publication marketplace, rendu visuel des diagnostics.

## 5. Reproduction

```powershell
cargo test -p ravum-lang-lsp            # unités : positions, URI, framing, changements
cargo test -p ravum-lang-cli --test lsp_client   # 9 tests client LSP réels
```

## 6. Configuration éditeur minimale

Un client LSP standard suffit. Exemple pour un client générique :

```json
{
  "command": "ravum-lang",
  "args": ["lsp"],
  "filetypes": ["ravum"],
  "rootPatterns": ["ravum.toml"]
}
```

L'extension de développement est fournie dans `tools/vscode-ravum` (elle n'est
pas publiée sur un marketplace) ; le harnais est décrit dans
[`VSCODE.md`](VSCODE.md). La configuration ci-dessus reste la configuration
générique minimale pour un autre client.

## 7. Cache de parse par fichier (2026-11-13, phase 7)

**Ce qui est implémenté.** `ravum_lang_sema::loader::ParseCache` mémorise le
résultat du parser par **`(hash FNV-1a 64 du contenu, FileId)`**. La clé inclut le
`FileId` parce que les `NodeId` et les spans des diagnostics l'embarquent : un
résultat de parse ne peut être réutilisé que pour le même `FileId`. Le cache est
borné à `PARSE_CACHE_LIMIT = 512` entrées (au-delà, il se vide) et vit dans
l'instance `Server` ; `Analysis::run` (API publique) en crée un neuf à chaque
appel, donc le comportement existant est inchangé.

Ce qui est mis en cache est **le parser seulement** : `resolve`, `typeck`,
`own` et `effects` restent globaux et recalculés à chaque analyse. Le cache ne
change aucun diagnostic : il évite de re-parser les fichiers dont le contenu et
l'identité n'ont pas bougé.

**Sûreté INV-07.** Deux tests d'intégration
(`crates/ravum-lang-lsp/tests/parse_cache.rs`) :

- `unchanged_file_is_not_reparsed` : deux analyses identiques ⇒ 2 hits / 0 miss
  supplémentaires ; une édition de `main.rv` ⇒ 1 miss (le fichier édité) et
  1 hit (`util.rv` inchangé) ;
- `stale_diagnostics_are_never_reused_for_the_new_buffer` : un programme avec
  `E-TYP-MISMATCH` est analysé, puis corrigé dans le même cache ; l'analyse
  corrigée ne contient **aucun** diagnostic de l'ancien buffer.

**FileId stables en session (T5, 2026-11-14).** Un `FileId` qui bouge invalide
le cache : le loader allouait les ids dans l'ordre de découverte, donc ajouter un
module déclaré *avant* les autres renumérotait les fichiers existants. La session
LSP tient maintenant une table `chemin canonique -> FileId` **croissante et
jamais réattribuée** (`LoadOptions::file_ids` / `LoadedPackage::file_ids`), que
`Analysis::run_with_cache_and_file_ids` fusionne après chaque analyse ; le CLI
garde les ids denses historiques (`file_ids: None`) et le corpus est inchangé.
Deux tests d'intégration et un test loader couvrent le contrat :

- `session_file_ids_keep_untouched_files_stable` : `main.rv` + `a.rv` + `b.rv` +
  `c.rv`, puis un 4e module `aa.rv` déclaré **avant** `a` ⇒ les trois fichiers
  inchangés gardent leur `FileId` et leurs `NodeId`, **3 hits** de cache de parse,
  et `aa.rv` prend `max + 1` ;
- `without_a_session_table_a_new_module_renumbers_the_other_files` : la même
  édition **sans** table ⇒ ids décalés, **0 hit**, `NodeId` changés (preuve que le
  test positif n'est pas vide) ;
- `loader::tests::a_session_table_that_maps_one_id_to_two_files_fails_closed` :
  une table incohérente est refusée (`E-IO`, aucun programme produit).

**Mesure (7.2).** `cargo test -p ravum-lang-lsp --test parse_cache -- --ignored
--nocapture` → 30 éditions d'une ligne sur `examples/package-demo`, analyse
**en processus** (transport JSON-RPC et éditeur exclus) :

| Configuration | p50 | p95 |
|---|---|---|
| sans cache de parse | 1,140 ms | 1,836 ms |
| avec cache de parse | **1,078 ms** | **1,362 ms** |

Cible P0-A « diagnostic p95 < 50 ms » : **atteinte au niveau de la phase
d'analyse** (1,4 ms), avec une marge de plus de 30×. Ce chiffre **ne mesure
pas** la latence de bout en bout vue par l'éditeur (transport, sérialisation,
affichage) : elle est mesurée séparément ci-dessous (p95 2,826 ms sur deux
fichiers, **337,6 ms à 5 110 lignes**) et ne doit pas être confondue avec la
cible.

**Limites.** La vérification sémantique reste globale (pas d'incrémentalité de
`own`/`typeck`) ; le cache ne couvre que le parser ; une seule machine et un seul
package mesurés ; le gain observé (≈ 25 % de p95) porte sur un package de deux
fichiers et ne s'extrapole pas à un gros projet.

**Mesure de bout en bout, transport inclus (2026-11-13, round 3).** Le même
harnais client (`crates/ravum-lang-cli/tests/lsp_client.rs`) mesure 20 éditions
d'une ligne de `examples/package-demo/app/src/main.rv` en attendant chaque
`textDocument/publishDiagnostics` :

```
cargo test -p ravum-lang-cli --test lsp_client -- --ignored --nocapture \
  end_to_end_diagnostic_latency
[lsp-e2e] package-demo (2 files), 20 one-line edits, transport included:
          p50=2.061 ms p95=2.826 ms
```

Cela inclut le cadrage JSON-RPC, le serveur, l'analyse (cache de parse actif) et
la publication. La cible P0-A « p95 < 50 ms » est **atteinte sur cette mesure
locale** (pipes locaux, une machine) ; ce n'est ni une mesure réseau, ni une
mesure avec un éditeur réel qui rend les diagnostics. Le fichier est édité **en
mémoire** : aucun fichier du dépôt n'est modifié.



**Mesure de bout en bout à l'échelle (2026-11-14, T3, `379b6fe`).** Même
harnais, paquet généré de 150 modules / **5 110 lignes** (même forme que
`tools/gen_scale_corpus.py`, vérifiée), 20 éditions d'une ligne dans `main` :

```
cargo test -p ravum-lang-cli --test lsp_client \
  end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture
[lsp-e2e] scale corpus (5110 lines, 150 modules), 20 one-line edits,
          transport included: p50=322.4 ms p95=337.6 ms
```

**La cible P0-A « p95 < 50 ms » n'est pas tenue à cette échelle** : le coût est
celui du `check` complet (365 ms p50 à 5 110 lignes, `LANG-TIMINGS.md` §9),
l'analyse sémantique reste globale et le cache de parse ne couvre que le parser.
La cible reste tenue sur les petits paquets (p95 2,826 ms). Un test fonctionnel
associé (`scale_package_navigation_and_diagnostics_work_end_to_end`, 35 modules,
1 025 lignes) vérifie la navigation inter-modules et l'aller-retour de
diagnostic.

**Mesure de bout en bout à l'échelle, profil `release` (2026-11-14, `214bf5e`).**
Le chiffre debug ci-dessus n'est pas le chiffre produit : le binaire livré est
compilé en `release`. Même harnais et même corpus, binaire release :

```
cargo test --release -p ravum-lang-cli --test lsp_client \
  end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture
[lsp-e2e] scale corpus (5110 lines, 150 modules), 20 one-line edits,
          transport included: p50=116.7 ms p95=163.5 ms
```

Le profil release divise la latence par ~2 (337,6 → **163,5 ms** de p95, 322,4 →
116,7 ms de p50), et **reste au-dessus de la cible de 50 ms** (×3,3). Deux
conclusions distinctes : (a) publier un chiffre debug comme chiffre produit
serait faux ; (b) même en release, l'analyse sémantique globale à 5 110 lignes
ne tient pas la cible — la décision de périmètre (cible réservée aux petits
paquets) ou d'incrémentalité sémantique reste humaine
([`HUMAN-DECISIONS-REQUIRED.md`](../reports/HUMAN-DECISIONS-REQUIRED.md),
`D-2026-11-14-5`).

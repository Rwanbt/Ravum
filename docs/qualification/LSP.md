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
| `textDocument/rename` | Implémenté | `rename_rewrites_declaration_and_uses` |
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
- **Intégration éditeur réelle** : **non testée dans VS Code**. Le test est un
  client LSP écrit à la main qui parle le protocole sur des tubes réels
  (`crates/ravum-lang-cli/tests/lsp_client.rs`). C'est une preuve
  d'interopérabilité de protocole, **pas** une preuve d'usage dans un IDE.

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

L'extension VS Code n'est **pas** fournie dans ce dépôt à ce stade ; la
déclaration ci-dessus ne constitue pas une affirmation de compatibilité testée.

# T7 — session VS Code réelle avec le serveur de langage Ravum

> **Statut : preuve d'intégration IDE réelle, verte.** Ce document décrit une
> session Visual Studio Code **effectivement exécutée**, pas une simulation : VS
> Code démarre lui-même `ravum-lang lsp`, et les assertions passent par l'API
> publique `vscode` (fournisseurs de survol, de définition, de références, de
> renommage, de complétion, de symboles, de formatage, et le canal de
> diagnostics).

| Élément | Valeur |
|---|---|
| Hôte | Windows 11 x86-64 |
| VS Code utilisé par le test | **1.145.2** (stable, résolu et mis en cache par `@vscode/test-electron`) |
| Extension | `tools/vscode-ravum` (extension de développement, `publisher.ravum-lang`) |
| Serveur | `target/debug/ravum-lang.exe lsp` — binaire réel, `stdio` |
| Lanceur | `node tools/vscode-ravum/test/runTest.js` |
| Résultat | **10 tests, 10 PASS / 0 FAIL** — rejoué après les 14 correctifs du compilateur |

## 1. Ce qui est réellement prouvé

Le test s'exécute **dans le processus extension host de VS Code**. Chaque
assertion appelle une commande standard de l'éditeur, qui interroge le
fournisseur enregistré pour le langage `ravum` — donc le client LSP de
l'extension, donc le processus `ravum-lang lsp`.

| # | Assertion | Ce que cela prouve |
|---|---|---|
| 1 | `document.languageId == "ravum"`, grammaire `source.ravum` enregistrée | l'extension est active et le langage reconnu |
| 2 | aucun diagnostic d'erreur sur un programme Core 1.0 valide | l'analyse réelle ne produit pas de faux positif |
| 3 | `vscode.executeDocumentSymbolProvider` renvoie `add` et `main` | les symboles viennent de l'AST du compilateur |
| 4 | `executeCompletionItemProvider` propose `add` (élément du package) et `fn` | la complétion mélange items du programme et mots-clés |
| 5 | `executeHoverProvider` sur `add(1, 2)` renvoie `fn add(…) -> i32` | le survol vient des signatures du checker |
| 6 | `executeDefinitionProvider` saute sur la ligne `fn add` | navigation réelle |
| 7 | `executeReferenceProvider` renvoie ≥ 3 emplacements depuis la **déclaration** | références depuis la déclaration |
| 8 | `executeFormatDocumentProvider` reformate un buffer volontairement mal espacé vers la forme canonique | le formateur du compilateur est branché dans l'éditeur |
| 9 | `executeDocumentRenameProvider` renomme `add` → `sum` sur la déclaration **et** les deux appels, puis l'édition est appliquée | renommage réel, multi-occurrences |
| 10 | après insertion de `let broken: i32 = "text";`, `vscode.languages.getDiagnostics` expose une erreur portant un code `E-*` | le canal de diagnostics `publishDiagnostics` alimente bien l'éditeur |

Le test 7 a mis en évidence une limite réelle du serveur — la navigation
(reférences, renommage) ne fonctionnait que depuis un **site d'usage**, pas
depuis la déclaration. Elle a été corrigée côté serveur (`item_name_at`,
`sources d'analyse`), puis couverte par un test Rust (`navigation_from_the_declaration_name_works`).
Le test 9 a mis en évidence un second défaut : `textDocument/prepareRename`
n'était pas implémenté, ce qui faisait échouer la commande de renommage de VS
Code **même avec une édition valide**. Le serveur répond maintenant par un
`Range` nu, et la capacité annonce `renameProvider.prepareProvider = true`.

## 2. Reproduire

```powershell
# 1. Construire le serveur
cargo build -p ravum-lang-cli

# 2. Installer les dépendances de l'extension (une fois)
cd tools/vscode-ravum
npm install --no-audit --no-fund

# 3. Lancer la session VS Code et la suite d'intégration
node test/runTest.js
```

Comportement attendu : une fenêtre VS Code s'ouvre sur un package Ravum
temporaire, la suite s'exécute, `10 passing`, code de sortie `0`.

### Variables d'environnement

| Variable | Effet |
|---|---|
| `RAVUM_LSP_BIN` | chemin absolu du binaire `ravum-lang` (sinon `target/{debug,release}/`) |
| `RAVUM_E2E_VSCODE` | chemin absolu d'un `Code.exe` à utiliser au lieu de la version téléchargée |
| `RAVUM_E2E_VSCODE_VERSION` | version VS Code demandée au téléchargement (`stable` par défaut) |
| `RAVUM_E2E_KEEP` | conserve le package temporaire pour inspection |

## 3. Piège d'environnement documenté

Si `ELECTRON_RUN_AS_NODE=1` est présent dans l'environnement, **tout** binaire
Electron — VS Code inclus — s'exécute comme un simple `node`, et le premier
argument est alors interprété comme un module à charger :

```
Error: Cannot find module 'C:\...\ravum-vscode-XXXX'
```

Le lanceur supprime explicitement cette variable avant de démarrer VS Code. Ce
n'est pas un contournement de test : c'est la condition pour obtenir un vrai
processus Electron.

## 4. Limites honnêtes

- Le test tourne avec `--disable-extensions` et un profil temporaire : il prouve
  l'intégration de **cette** extension, pas la cohabitation avec d'autres
  extensions.
- La version de VS Code est celle du canal stable téléchargée par l'outil ; le
  numéro exact est affiché par le test et consigné dans les preuves de run.
- Aucune assertion ne porte sur l'apparence (coloration, thème) au-delà de
  l'existence de la grammaire TextMate.
- Le test ne couvre ni le multi-root workspace, ni les documents non sauvegardés
  dans un onglet non ouvert, ni les très gros fichiers.
- Il ne remplace pas un audit externe (T4/T10/T12) : c'est une preuve
  d'intégration produit, pas une revue indépendante.

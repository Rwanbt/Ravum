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
cargo test -p ravum-lang-cli --test lsp_client   # 12 tests client LSP réels (+ 3 mesures #[ignore])
```

Les mesures de latence S8 (§8) sont des tests `#[ignore]` qui n'écrivent un
export que si `RAVUM_LSP_LATENCY_OUT` est défini. Le chemin doit être **absolu** :
`cargo test` exécute le binaire de test avec la racine du **paquet**
(`crates/ravum-lang-cli`) comme répertoire courant.

```powershell
$env:RAVUM_LSP_LATENCY_OUT = "$PWD/target/lsp/latency.json"
cargo test --release -p ravum-lang-cli --test lsp_client -j 4 `
  end_to_end_diagnostic_latency_on_the_thousand_line_corpus -- --ignored --nocapture --test-threads=2
cargo test --release -p ravum-lang-cli --test lsp_client -j 4 `
  end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture --test-threads=2
python tools/timings_gate.py --lsp target/lsp/latency.json
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
116,7 ms de p50). Ce chiffre est **historique** : il précède la correction de la
phase `own` de la vague 3 (§8 ci-dessous) et il n'est plus le chiffre courant. Il
était déjà **au-dessus de la cible S8 de 150 ms** (×1,09) : c'est la correction de
`own` qui fait passer la mesure sous cette cible, pas un changement de cible.

## 8. Latence de bout en bout : cibles S8 et re-mesure (2026-11-14, vague 3)

La décision du responsable **S8** (2026-11-14) refuse d'assouplir la cible pour
absorber un défaut algorithmique. Elle fixe deux cibles **produit** sur la même
mesure que ci-dessus — cadrage JSON-RPC, serveur, analyse et publication
inclus, 20 éditions d'une ligne, profil `release` :

| Cible S8 | Corpus de mesure | Comparaison |
|---|---|---|
| `p95 < 50 ms` jusqu'à 1 000 lignes | **1 025 lignes** (35 modules × 5 fonctions) | strictement inférieure |
| `p95 ≤ 150 ms` à 5 110 lignes | **5 110 lignes** (150 modules × 6 fonctions) | inférieure ou égale |

Le petit corpus a son propre test de mesure,
`end_to_end_diagnostic_latency_on_the_thousand_line_corpus` (il vérifie d'abord
`lines == 1 025`, donc le point de mesure est **au-dessus** de l'enveloppe
« jusqu'à 1 000 lignes » : le verdict est conservateur) ; le grand reste
`end_to_end_diagnostic_latency_on_the_scale_corpus`. Les deux écrivent leur ligne
dans le fichier désigné par `RAVUM_LSP_LATENCY_OUT`, et
`tools/timings_gate.py --lsp` applique les cibles : corpus manquant, ligne
mesurée en `debug`, statistique non positive ou non finie, `p95 < p50` ou nombre
d'éditions différent de 20 échouent **fermé**, sans arrondi et sans facteur.

### 8.1 Mesures et verdicts

Base : **`7b615ed`** (branche `goal/wave-3`, worktree `D:\App\ravum-wt-g`,
`CARGO_TARGET_DIR=D:\App\ravum-target-g`). Machine : Intel Core i7-13620H
(10 cœurs, 16 processeurs logiques), 16 Go de RAM, Windows 11 Professionnel,
`rustc 1.95.0`, binaire `ravum-lang` compilé en `release`. Trois passages
consécutifs du même protocole :

| Passage | Corpus | Lignes | Éditions | p50 | p95 | idle machine (médiane) |
|---|---|---|---|---|---|---|
| 1 (`22:58:34`) | 35 × 5 | 1 025 | 20 | 18,1181 ms | **19,3938 ms** | 89 % |
| 1 (`22:58:34`) | 150 × 6 | 5 110 | 20 | 77,2263 ms | **80,7698 ms** | 83 % |
| 2 (`22:58:41`) | 35 × 5 | 1 025 | 20 | 18,7559 ms | **19,9060 ms** | 90 % |
| 2 (`22:58:41`) | 150 × 6 | 5 110 | 20 | 76,0915 ms | **79,7376 ms** | 83 % |
| 3 (`22:59:04`) | 35 × 5 | 1 025 | 20 | 18,6272 ms | **19,7785 ms** | 90 % |
| 3 (`22:59:04`) | 150 × 6 | 5 110 | 20 | 75,5331 ms | **81,2691 ms** | 85 % |

**Verdict 1 — `p95 < 50 ms` à 1 025 lignes : TENUE.** Les trois passages donnent
19,3938 / 19,9060 / 19,7785 ms ; le plus défavorable vaut **38 % de la cible**
(marge ×2,51). Sur les **130 runs** enregistrés, 126 tiennent la cible ; les 4
exceptions mesuraient toutes à ≤ 20 % d'idle (machine saturée par les builds
concurrents des autres agents).

**Verdict 2 — `p95 ≤ 150 ms` à 5 110 lignes : TENUE.** Les trois passages donnent
80,7698 / 79,7376 / 81,2691 ms ; le plus défavorable vaut **54 % de la cible**
(marge ×1,85). Sur les **129 runs**, 107 tiennent la cible ; aucun run à moins de
40 % d'idle ne la tient, et les 22 dépassements mesuraient tous à ≤ 79 % d'idle
(un seul au-dessus de 74 %, à 157,13 ms).

Le chemin CI complet a été rejoué localement (les deux tests écrivent le même
export, puis le gate) : `scale-1025 p95=28.824 ms`, `scale-5110 p95=122.345 ms`,
`timings_gate: PASS`. Ce rejeu est plus lent que les trois passages ci-dessus
parce qu'il a été exécuté pendant que la machine portait d'autres builds.

### 8.2 Condition de mesure (machine partagée)

Cette vague s'est déroulée sur une machine **partagée** avec les jobs des autres
sous-agents (`cargo-mutants`, `cargo-llvm-cov`, `differential_oracle`, Defender).
**259 runs** ont été enregistrés (130 à 1 025 lignes, 129 à 5 110), avec une
charge étrangère mesurée de ~0 % à ~95 % de la machine. Le critère
d'acceptation, fixé avant les mesures, était : ne publier que les passages dont
la machine était **≥ 85 % idle** pendant le run.

| Corpus | Runs | Runs à ≥ 80 % d'idle | p95 de ces runs | p95 max, tous runs |
|---|---|---|---|---|
| 1 025 lignes | 130 | 12 | 18,84–27,42 ms (4 runs à ≥ 84 % : 18,84–19,91 ms) | 54,53 ms |
| 5 110 lignes | 129 | 4 | **78,23–81,27 ms** | 237,91 ms |

Les trois passages publiés sont trois passages consécutifs dont les deux runs
sont les plus proches du critère : 89/83, 90/83 et 90/85 % d'idle. Un seul
(passage 3) atteint strictement 85 % sur **les deux** corpus ; les passages 1 et
2 sont à 83 % sur le grand corpus. Ils tombent dans la bande 78–81 ms ci-dessus,
qui est la plus basse observée.

Deux lectures honnêtes :

- Le critère de 85 % n'a été atteint que **4 fois sur 130** (petit corpus) et
  **1 fois sur 129** (grand corpus) : la machine n'a jamais été au repos plus de
  quelques secondes pendant la vague. Les passages publiés sont donc les plus
  propres disponibles, pas une mesure sur machine idle.
- Les 4 runs à ≥ 80 % d'idle donnent la même bande (78,23–81,27 ms à 5 110
  lignes ; 18,84–19,91 ms à 1 025 lignes sur les 4 runs à ≥ 84 %). À charge plus
  élevée, la latence monte : 22 runs sur 129 dépassent 150 ms à 5 110 lignes, et
  aucun run à moins de 40 % d'idle ne tient la cible. Un run isolé à 79 % d'idle
  atteint 157,13 ms : la variance d'une machine partagée n'est pas un intervalle
  de confiance.

**Conséquence de lecture** : la contention ne peut qu'**augmenter** la latence
observée. Les chiffres publiés sont donc des majorants de la latence sur machine
au repos, et les cibles sont tenues avec les marges indiquées. Aucun chiffre de
cette section n'autorise à relâcher une cible : S8 les a fixées, et un passage
lent sous charge n'est pas un argument contre elles.

### 8.3 Contrôle CI

Le job `lsp-targets` de `.github/workflows/performance.yml` (séparé du job de
latence/linéarité de `check`, qui est inchangé) exécute les deux mesures en
`--release` avec `RAVUM_LSP_LATENCY_OUT=<workspace>/target/lsp/latency.json`
(chemin absolu : un binaire de test a la racine du paquet comme répertoire
courant), puis :

```text
python tools/timings_gate.py --lsp target/lsp/latency.json --lsp-small-p95-ms 50 --lsp-large-p95-ms 150
```

Le gate est **fail-closed** (un export incomplet, `debug`, incohérent ou un
`p95` au-delà de sa cible fait échouer le job) et le job publie l'export en
artefact. Limite explicite : les cibles S8 ont été établies sur la machine de
référence (16 Go) ; un runner GitHub partagé est plus lent, donc un run rouge y
signale « ce runner n'atteint pas la cible produit », jamais une autorisation de
l'assouplir.

### 8.4 Ce que cette section ne prouve pas

- Une seule machine, un seul OS, un seul compilateur (`rustc 1.95.0`), un seul
  corpus synthétique par point ; aucune mesure multi-plateforme.
- La mesure inclut le transport JSON-RPC local (pipes), pas le rendu des
  diagnostics dans un éditeur réel, ni un réseau.
- La machine n'était pas au repos (§8.2) : les chiffres sont des majorants, et
  l'incertitude liée à la charge n'est pas un intervalle de confiance.
- Trois répétitions par point, pas de test statistique : le p95 est le
  nearest-rank d'un échantillon de 20 éditions, donc la deuxième plus grande
  valeur observée.
- Aucun `PROVED`, aucune maturité « production » : statut `TESTED` pour ces
  mesures et les tests cités.

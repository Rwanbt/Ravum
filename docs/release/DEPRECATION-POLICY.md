# Politique de dépréciation — interfaces publiques de `ravum-lang` (critère R10)

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL`.** Date : **2026-11-14**.
>
> Ce document **propose** une politique de dépréciation pour les interfaces
> publiques du langage Core 1.0. Il n'en applique aucune :
>
> - **aucune interface n'est dépréciée à ce jour** (registre §5, vide) ;
> - **aucun mécanisme d'avertissement de dépréciation n'existe dans le code**
>   (§6.2, absence vérifiée) ;
> - **aucune release n'a été publiée, aucun tag n'existe, aucun artefact n'est
>   signé** (§0, §7) : le compteur de version publiée est à zéro, donc le cycle
>   décrit en §3 n'a jamais eu l'occasion de s'appliquer.
>
> Il **ne ferme pas R10**. Le critère
> [`R10`](../roadmap/PRODUCTION-READINESS.md) exige quatre choses en plus d'une
> politique écrite : CLI `--help` = contrat, schémas JSON versionnés, registre de
> codes figé, LSP testé. Ce document décrit l'état réel de ces quatre points
> (§2, §6) ; il ne les change pas, et une politique n'est pas une preuve.
>
> Il ne modifie ni `spec/language/compatibility.md` (DRAFT), ni
> [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md), ni
> [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md), ni
> [`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md), qu'il cite. Les règles `DEP-*`
> ci-dessous sont **des propositions**, pas des obligations en vigueur.

## 0. Faits vérifiés avant d'écrire des règles

Une politique n'est pas une observation : elle dit ce qu'il faudrait faire. Les
faits ci-dessous datent du **2026-11-14** et sont liés à la branche
`goal/consolidation-2` (tête `214bf5e` au contrôle initial). Ils sont tous
reproductibles par la commande ou le fichier cité.

| Fait | Valeur observée | Comment / source |
|---|---|---|
| Branche et tête | `goal/consolidation-2`, `214bf5e` ; au contrôle initial, aucun fichier suivi modifié et un fichier non suivi | `git status`, `git log --oneline -5` ; arbre non stable, voir §0.1 |
| Tags | **aucun** (`git tag --list` vide ; `git describe --tags` → `fatal: No names found`) | commande du 2026-11-14 |
| Releases publiées | **aucune** ; `AGENTS.md` rappelle « sans tag ni release » ; `D3-D4-D9-POLICIES.md` §0 a observé `[]` via l'API GitHub le 2026-11-11 | [`AGENTS.md`](../../AGENTS.md), [`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md) |
| Signature | mécanisme qualifié, **aucune clé de projet, aucun `allowed_signers`** | [`SIGNATURES.md`](../qualification/SIGNATURES.md) §1 ; recherche `Get-ChildItem -Recurse -Filter allowed_signers` : aucun fichier |
| Registre des codes | **108 codes** enregistrés, 126 littéraux émis, 0 non enregistré, 0 jamais émis, 0 `W-*` hors DIAG-1.6 | `python tools/registry_audit.py` → exit **0** |
| Schéma JSON | `schema_version: 1` | `crates/ravum-lang-cli/src/json.rs:16` |
| Contrat JSON exercé | **6 scénarios, 0 violation, 1 tension** (`doctor` émet des chemins absolus) | `python tools/json_contract.py` → exit **0**, `json-contract: PASS` |
| Drapeaux CLI | **22 drapeaux documentés acceptés, 0 rejeté** | `pwsh -File scripts/cli-flag-audit.ps1` → exit **0** |
| Version du binaire | `ravum-lang 0.1.0 (edition core-1.0-draft.1)` | `target\release\ravum-lang.exe --version` ; crate `0.1.0` (`Cargo.toml:30`) |
| Aide par commande | `ravum-lang check --help` est une **erreur d'usage (exit 2)** ; l'aide se demande par `help check` | commande du 2026-11-14 |
| Manifeste d'installation | `schema_version: 1`, 16 champs écrits par les deux installateurs | `scripts/install.ps1:232`, `scripts/install.sh:190` ; exemple réel dans [`INSTALL.md`](INSTALL.md) §4 |
| Corpus de conformance | **483** fichiers `case.json` | `Get-ChildItem conformance\cases -Recurse -Filter case.json` |
| Mécanisme de dépréciation | **absent** : 0 occurrence de `deprecat` dans les fichiers `*.rs` du dépôt ; aucune commande `migrate` dans `--help` | recherche de texte ; `ravum-lang --help` |
| CHANGELOG de projet | **absent** (aucun `CHANGELOG*` hors dépendances npm sous `tools/vscode-ravum/node_modules/`) ; Q-COMPAT-1 le recommande | recherche de fichiers ; `spec/language/compatibility.md` §9 |
| Spécification de compatibilité | **DRAFT, non ratifiée** ; `COMPAT-3.6`, Q-COMPAT-1 et Q-COMPAT-5 sont ouvertes | [`compatibility.md`](../../spec/language/compatibility.md) |
| Liste des avertissements | **fermée** à deux codes | [`diagnostics.md`](../../spec/language/diagnostics.md) DIAG-1.6 |

## 0.1 Environnement des vérifications et stabilité de l'arbre

**Environnement.** Windows 11 x86-64 (`Microsoft Windows NT 10.0.26100.0`),
PowerShell 7.7.0, Python 3.13.7, `rustc 1.95.0 (59807616e 2026-04-14)`. Les
mesures d'interface ont été faites avec `target\release\ravum-lang.exe`
(`ravum-lang 0.1.0 (edition core-1.0-draft.1)`, SHA-256
`25987c3bac881557dcfdae1cfd8571ead8dadfaa19619de470b549ea0e8f8879`) et, pour
`tools/json_contract.py`, avec `target\debug\ravum-lang.exe` (SHA-256
`101e0f0ea1603034b754682c3f7c29acdf9b3b720c9fd5a5335afc9f7bbaa845`), que le
script préfère par construction. Ces empreintes ne sont **pas** des empreintes de
release ([`PROVENANCE.md`](PROVENANCE.md)) et ne valent que pour ces binaires.

**Stabilité de l'arbre.** D'autres agents travaillent dans le même répertoire :
des fichiers non suivis et une modification de `README.md` sont apparus après le
contrôle initial, sans rapport avec cette politique. Les faits du tableau §0 sont
donc datés et liés à cette session ; ils ne décrivent pas un état figé du dépôt.
De même, les numéros de ligne cités ont été relevés le 2026-11-14 : un fichier
modifié par un autre agent peut les décaler, c'est le contenu cité qui fait foi.

## 1. Périmètre : les sept interfaces publiques

Sont couvertes par cette politique **uniquement** les interfaces ci-dessous.
Chacune est définie ailleurs : la politique ne réécrit pas le contrat, elle dit
comment il peut changer.

| Interface | Où le contrat est écrit | Version | Ce qui est réellement tenu aujourd'hui | Preuve outillée |
|---|---|---|---|---|
| Commandes et drapeaux CLI | `crates/ravum-lang-cli/src/args.rs` (chaînes `USAGE`), [`crates/ravum-lang-cli/README.md`](../../crates/ravum-lang-cli/README.md) | binaire `0.1.0` | chaque drapeau de la ligne `USAGE` est accepté par son parseur ; **pas** de stabilité promise des options (COMPAT-5.5) | `scripts/cli-flag-audit.ps1` |
| Codes de sortie | [`diagnostics.md`](../../spec/language/diagnostics.md) §8, CLI README ; constantes `crates/ravum-lang-cli/src/util.rs:11-14` (`EXIT_OK/FAIL/USAGE/TOOL`) | `0/1/2/3` | `run` propage le code du programme (DIAG-8.1) ; `--json` ne change jamais le code (DIAG-8.4) | `tools/json_contract.py` (DIAG-8.4), `crates/ravum-lang-cli/tests/exit_codes.rs` |
| Sortie JSON de `check` | [`diagnostics.md`](../../spec/language/diagnostics.md) §7, CLI README | `schema_version: 1` | enveloppe et champs obligatoires conformes aux MUST DIAG-7.1…7.5 | `tools/json_contract.py` |
| Registre des codes de diagnostic | [`diagnostics.md`](../../spec/language/diagnostics.md) §3 | 108 codes | cohérence bidirectionnelle registre ↔ code émis ; immuabilité du **sens** non outillée | `tools/registry_audit.py` |
| Protocole LSP | `crates/ravum-lang-lsp`, [`LSP.md`](../qualification/LSP.md) | LSP 3.17 annoncé par `--help` ; `positionEncoding: utf-16` | capacités annoncées à l'`initialize` ; méthode non implémentée refusée en `-32601` | tests LSP + [`VSCODE.md`](../qualification/VSCODE.md) |
| Manifeste `ravum-install.json` | [`INSTALL.md`](INSTALL.md) §4, `scripts/install.ps1`, `scripts/install.sh` | `schema_version: 1` | champs écrits à l'installation, cohérence `sha256`/version/commit vérifiée en CI | `qualification-evidence.yml` (installateur Windows) |
| Sous-ensemble du langage `.rv` | `spec/language/*.md`, édition `core-1.0` | `core-1.0-draft.1` (**DRAFT**) | **aucune stabilité** : COMPAT-2.1 autorise tout changement pendant le draft | corpus de conformance (483 cas), [`CONFORMANCE-BUILD-CONTRACT.md`](../qualification/CONFORMANCE-BUILD-CONTRACT.md) |

**Hors périmètre, explicitement** (aucune garantie n'est impliquée par ce
document) :

- la **plateforme Rust historique** (`ravum-cli`, `ravum-daemon`,
  `ravum-protocol`, `ravum-workspace-coordinator`, …) et ses propres objets
  `schema_version` (1, 2, …) : c'est une autre ligne du projet, avec ses contrats
  internes ; une politique de dépréciation pour elle n'est pas écrite ici ;
- les **API de bibliothèque** des crates `ravum-lang-*` : interface de
  développement interne, pas surface publique ;
- le **Rust généré** (`#![forbid(unsafe_code)]`), le cache `.ravum/`, les
  **messages** texte et les notes/aides (DIAG-6.6, DIAG-9.4 : hors contrat) ;
- l'**ABI** : il n'y en a aucune ([`abi.md`](../../spec/language/abi.md),
  `compatibility.md` §1) ;
- l'extension VS Code `tools/vscode-ravum` : outil de qualification, pas un
  contrat publié.

## 2. Règles de versionnement

### 2.1 Ce qui est garanti avant Core 1.0 (DRAFT)

**Presque rien n'est garanti, et c'est écrit.** `compatibility.md` est `DRAFT —
CANDIDATE, non ratifiée` et sa table §1 dit du langage : « draft : peut changer
sans préavis ». COMPAT-2.1 (MUST) précise qu'une règle peut changer pendant le
draft, à condition de journaliser le changement avec `draft.N+1` et la liste des
programmes affectés. Le binaire reste en `0.x` tant que les ADR ne sont pas
ratifiés et que l'édition est `core-1.0-draft.N`
([`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md) P3.6, point 7).

Ce qui est **tenu aujourd'hui**, ce n'est donc pas une promesse de stabilité mais
un ensemble de **constats outillés** :

1. cohérence bidirectionnelle du registre des codes (§2.3) ;
2. MUST du schéma JSON §7 et des codes de sortie §8 sur trois commandes (§2.2) ;
3. identifiants de version déclarés et identiques partout : crate `0.1.0`,
   édition `core-1.0`, spécification `core-1.0-draft.1`, JSON
   `schema_version: 1`, lockfile `lockfile_version = 1`, manifeste
   `schema_version: 1` ;
4. format de la ligne `--version`, **contrat de fait** : les deux installateurs
   la parsent — PowerShell avec `^ravum-lang\s+(\S+)`
   (`scripts/install.ps1:192-199`), shell avec
   `sed 's/^ravum-lang \([^ ]*\).*/\1/p'` (`scripts/install.sh:172-176`) — et
   échouent sinon ; un changement de ce préfixe casse l'installation, pas
   seulement l'affichage.

**DEP-2.1 (proposé).** Un changement qui modifie l'un de ces identifiants, ou le
format de `--version`, est traité comme une **rupture d'interface** et suit le
cycle §3, quelle que soit la phase. Pendant le draft, cela veut dire : RFC (pour
le langage) ou note de décision (pour le reste), revue humaine, et mise à jour
coordonnée de la spec, du README CLI, des outils et des tests.

**DEP-2.2 (proposé).** Aucune stabilisation n'est déclarée par ce document : il
ne transforme pas `core-1.0-draft.1` en version stable et ne crée aucune
obligation de compatibilité là où COMPAT-2.1 en refuse encore.

### 2.2 Quand un schéma JSON change de version

La règle est déjà normative : DIAG-9.3 (MUST), COMPAT-5.1 et COMPAT-5.3 (MUST).
Elle est reformulée ici pour être applicable sans ambiguïté.

**Incrémente `schema_version` (rupture) :** retirer, renommer, retyper ou changer
le **sens** d'un champ obligatoire ; changer l'ensemble de valeurs d'un champ
énuméré (`status`, `target.kind`, `command`) ; changer l'unité d'un span
(`start_col`/`end_col` sont en unités UTF-16, §7.3) ; changer la signification
d'`exit_code` dans l'enveloppe.

**N'incrémente pas `schema_version` (additif) :**

- ajouter un champ **optionnel** — précédent réel : `timings`, ajouté sans bump
  et verrouillé par `crates/ravum-lang-cli/tests/timings_contract.rs` ;
- ajouter un **code de diagnostic** (DIAG-9.2, DIAG-9.5) ;
- ajouter une clé au résumé — le résumé observé contient déjà beaucoup plus que
  les deux clés `errors`/`warnings` du §7.2, et `tools/json_contract.py` n'exige
  que leur présence ; c'est un ajout toléré au sens DIAG-7.3 ;
- ajouter un champ au **rapport de conformance** : `CONFORMANCE-BUILD-CONTRACT.md`
  §4 documente des champs additifs sans bump, avec le prix explicite (« un
  lecteur qui les ignore lit le même `cases`/`summary` »).

**DEP-2.3 (proposé).** Un bump de `schema_version` est une modification de
contrat : il touche, dans le même changement, le §7 de
[`diagnostics.md`](../../spec/language/diagnostics.md), le §JSON de
[`crates/ravum-lang-cli/README.md`](../../crates/ravum-lang-cli/README.md),
`tools/json_contract.py` (qui épingle `schema_version == 1`),
`crates/ravum-lang-cli/src/json.rs`, les tests, et une note de migration.

**DEP-2.4 (proposé).** Le producteur n'émet **jamais** un `schema_version` qu'il
ne documente pas, et un consommateur qui ne connaît que `1` doit **échouer
explicitement** s'il reçoit une version supérieure inconnue, plutôt que
l'interpréter partiellement. (Aujourd'hui, DIAG-7.3 impose d'ignorer les champs
inconnus, mais ne dit rien d'une **version** inconnue : c'est le trou que cette
règle comble ; elle ne modifie pas la spec.)

**État réel des champs (mesuré, 2026-11-14).** L'enveloppe émise par
`check --json` porte toujours : `schema_version`, `tool`, `tool_version`,
`spec_version`, `command`, `status`, `exit_code`, `success`, `target`, `summary`,
`diagnostics` ; et, selon la commande et les options, `profile`,
`dependencies_digest`, `lockfile_stale` et `timings` — ces quatre champs sont
`Option` et omis quand ils sont absents (`json.rs:32-41`). Dans l'échantillon
`check` d'un fichier isolé, `profile` et `dependencies_digest` sont présents,
`lockfile_stale` et `timings` absents. Chaque diagnostic porte `code`,
`severity`, `phase`, `message`, `file`, `line`, `column`, `end_line`,
`end_column`, `primary`, `labels`, `notes`. Trois écarts documentaires en
découlent, à trancher (§8.4) :

- `success` est **émis et documenté** (CLI README §JSON, `json.rs:30`) mais
  **absent du §7.2 de la spec** et du jeu de champs requis par
  `tools/json_contract.py` : sa suppression ne serait détectée par aucun outil ;
- `feature_id`, `rule_id`, `help`, `related` sont **déclarés optionnels** par le
  §7.2 mais **jamais émis** par `crates/ravum-lang-cli/src/json.rs` (aucune
  occurrence dans les sources du CLI) : un consommateur ne peut pas compter sur
  eux ;
- le §7.2 énumère neuf valeurs de `command`, alors que seules **trois**
  commandes acceptent `--json` (`check`, `test`, `doctor`, confirmé par
  l'aide) : la liste du champ est plus large que la réalité des options.

### 2.3 Quand un code de diagnostic peut être ajouté, retiré, ou réutilisé

**Ajout.** Un code s'ajoute **d'abord** dans le §3 de
[`diagnostics.md`](../../spec/language/diagnostics.md), **ensuite** dans le code,
et `python tools/registry_audit.py` doit rester vert. L'audit vérifie les deux
sens : un code émis et absent du §3 est une violation (DIAG-1.1), et un code
déclaré qu'aucune source n'émet est **aussi** une violation. Conséquence
concrète : **on ne peut pas « réserver » un code** aujourd'hui — un code
enregistré mais jamais émis fait échouer la porte. Ajouter un code est compatible
(DIAG-9.2, DIAG-9.5, COMPAT-2.5).

**Retrait.** **Interdit** en Core 1.0 : DIAG-9.1 (MUST) dit qu'un code est
immuable, jamais réutilisé, jamais supprimé — « au plus marqué `deprecated`,
jamais dans Core 1.0 ». Le seul chemin réel est une **nouvelle édition majeure**
(COMPAT-2.3), et même là la politique proposée refuse la réutilisation.

**Réutilisation.** Jamais : DIAG-9.1 pour les codes, COMPAT-2.9 pour
`rule_id`/`feature_id` (les renommer est une rupture). Un code retiré puis
réaffecté à un autre défaut rendrait tout historique ininterprétable.

**Avertissements.** La liste est **fermée** (DIAG-1.6 : `W-RES-UNUSED-IMPORT`,
`W-OWN-UNUSED-VARIABLE`). Ajouter un `W-*` n'est donc **pas** un ajout de code
ordinaire : c'est une modification normative, réservée à l'humain (§8.2). C'est
la contrainte qui rend le §3.3 non trivial.

**DEP-2.5 (proposé).** Le registre est **append-only** : une ligne du §3 ne
disparaît jamais, même après une édition majeure ; au plus est-elle annotée
« deprecated since … » par une décision humaine (modification de la spec,
§8.9). Le gel exigé par R10 est donc un gel de la **liste** ; l'immuabilité du
**sens** reste une obligation de revue (DIAG-9.1), pas une vérification machine —
l'audit lirait un code dont le sens a changé sans broncher.

### 2.4 Compatibilité LSP

- **Transport et encodage** : JSON-RPC 2.0 sur `stdio`, *base protocol* LSP
  (`Content-Length`) ; `positionEncoding: utf-16`
  (`crates/ravum-lang-lsp/src/server.rs:283`) ; les colonnes sont comptées en
  unités UTF-16, pas en octets.
- **Capacités** : annoncées à l'`initialize`
  (`crates/ravum-lang-lsp/src/server.rs:263-284`) — `textDocumentSync`
  incrémental (`change: 2`), hover, définition, références, renommage avec
  `prepareProvider`, complétion, symboles de document, symboles d'espace de
  travail, formatage. `serverInfo.version` vaut la version du crate.
- **Additif** : ajouter une capacité ou une méthode est compatible ; un client
  doit traiter une capacité absente comme « fonction indisponible », et une
  méthode non implémentée arrive en `-32601 method not found`
  (`server.rs:49`), jamais en succès simulé ([`LSP.md`](../qualification/LSP.md) §2).
- **Rupture** : retirer une capacité, passer `change` de `2` à `1`
  (incrémental → complet), changer l'encodage de position, changer la forme d'une
  réponse existante, ou remplacer un `null` documenté (« refus plutôt que
  devinette », [`LSP.md`](../qualification/LSP.md) §3, point 5) par une valeur
  inventée.
- **Avertissement impossible aujourd'hui** : le serveur n'envoie **aucun**
  `window/showMessage` (aucune occurrence dans `crates/ravum-lang-lsp/src`) ; le
  seul signal disponible est l'annonce de capacité et la documentation.

**DEP-2.6 (proposé).** Une capacité ne peut être retirée qu'après le cycle §3 et
seulement si une version publiée du serveur a déjà annoncé le remplacement ;
tant qu'aucun canal d'avertissement LSP n'existe, le retrait reste bloqué par
construction (§3.3).

### 2.5 Manifeste d'installation `ravum-install.json`

Écrit à la racine du préfixe, UTF-8 sans BOM, par `scripts/install.ps1` et
`scripts/install.sh`. `schema_version: 1`, 16 champs (`schema_version`, `name`,
`version`, `package_version`, `git_commit`, `git_dirty`, `installed_at_utc`,
`sha256`, `rustc`, `platform`, `arch`, `profile`, `locked`, `binary`, `prefix`,
`installer`), exemple réel dans [`INSTALL.md`](INSTALL.md) §4.

**DEP-2.7 (proposé).**

1. Les 16 champs actuels sont **écrits inconditionnellement** : les retirer,
   les renommer ou changer leur sens incrémente `schema_version`.
2. Ajouter un champ est additif ; un lecteur **ignore** un champ inconnu et ne
   doit pas exiger un champ qu'il ne connaît pas.
3. Les champs d'identité (`name`, `version`, `sha256`, `git_commit`,
   `git_dirty`) ne sont jamais détournés : `git_dirty: true` signifie « ce
   binaire ne correspond pas exactement au commit enregistré » et interdit de le
   présenter comme artefact de release ([`INSTALL.md`](INSTALL.md) §4,
   [`PROVENANCE.md`](PROVENANCE.md)).
4. **Aucun avertissement de dépréciation de champ n'est possible dans le
   manifeste** : il n'est pas signé ([`SIGNATURES.md`](../qualification/SIGNATURES.md)),
   il n'existe pas de canal de message, et sa seule évolution non cassante est
   additive. Un champ déprécié reste donc écrit jusqu'au bump.

**Constat CI** : les contrôles d'installateur vérifient `sha256`, `package_version`,
`git_commit`, `git_dirty`, `locked`, `rustc`, puis le cycle
install/uninstall/reinstall (`.github/workflows/qualification-evidence.yml`) ;
**aucune** étape n'asserte le `schema_version` du manifeste ni ne refuse une
version inconnue (`.github/workflows/linux-qual.yml:261` vérifie seulement que
le fichier existe).

### 2.6 Sous-ensemble du langage `.rv`

Tout changement qui **accepte**, **refuse** ou **change le sens** d'un programme
est un changement de langage. Il suit quatre obligations :

1. **RFC obligatoire** : une proposition d'ingénierie au format
   [`docs/rfc/README.md`](../rfc/README.md) — identifiant `RFC-<SUJET>`, statut
   `PROPOSED` par défaut, **seul le mainteneur** passe une RFC à `ACCEPTED`,
   `REJECTED`, `SUPERSEDED` ou `DEFERRED` ; un agent ne clôt jamais une RFC ;
2. **journal de spec** : `draft.N+1` avec la liste des programmes affectés
   (COMPAT-2.1) ;
3. **cas de conformance** : la modification laisse une trace exécutable dans
   `conformance/cases/**` (le contrat `check ⇒ build` est
   [`CONFORMANCE-BUILD-CONTRACT.md`](../qualification/CONFORMANCE-BUILD-CONTRACT.md)) ;
4. **migration** : tout programme accepté qui devient refusé exige un patch de
   source proposé, revu et testé (COMPAT-6.1, COMPAT-6.2) — jamais une
   réécriture silencieuse.

**Précédents observés** (deux changements documentés de la surface acceptée,
cités ici pour leur trace de migration ; la liste n'est pas présentée comme
exhaustive) :

| RFC | Changement | Trace de migration |
|---|---|---|
| [`RFC-D-COERCE-1-MATRIX`](../rfc/RFC-D-COERCE-1-MATRIX.md) | durcissement `String → &str` : `f(owned)` refusé là où `&str` est attendu ; texte normatif TYP-8.5/TYP-13.1 | `examples/stats` migré de `parse_f64(trimmed)` à `parse_f64(trimmed.as_str())`, sans changer les sorties attendues (`CONFORMANCE-BUILD-CONTRACT.md` §6.3) |
| [`RFC-D13-STATICITY`](../rfc/RFC-D13-STATICITY.md) | rend `return a[1]` (projection d'agrégat) accepté | cas de corpus + tests `crates/ravum-lang-codegen/tests/backend_audit.rs` ; §11 de `CONFORMANCE-BUILD-CONTRACT.md` |

Ces deux changements sont `PROPOSED` et n'ont pas suivi de cycle de dépréciation :
**aucun cycle n'existait**, et aucune version publiée n'a été cassée puisqu'il n'y
en a aucune. C'est exactement le trou que cette politique comble pour l'avenir —
pas une régularisation du passé.

**DEP-2.8 (proposé).** Pendant le draft (aucune version publiée), un changement de
langage peut être livré avec RFC + journal `draft.N+1` + migration de corpus +
approbation humaine. **Après la première version publiée**, un changement qui
refuse un programme accepté par cette version devient une rupture d'édition
majeure (COMPAT-2.3) et suit le cycle §3, sans exception.

## 3. Cycle de dépréciation

Le cycle a six étapes. Une dépréciation ne saute aucune étape ; une étape non
satisfaite **bloque** la suivante (fail-closed, comme le reste du projet).

### 3.1 Proposition

Pour le langage : RFC (obligatoire, §2.6). Pour le CLI, le JSON, le LSP ou le
manifeste : une note de décision datée, citant l'élément, le remplacement, les
consommateurs connus et la preuve que le remplacement fonctionne. Aucun agent ne
décide seul (§3.7).

### 3.2 Annonce

L'annonce doit être **écrite et datée** dans un document versionné, et reprise
dans les notes de version quand il en existera. Or **il n'existe aucun CHANGELOG
de projet** (Q-COMPAT-1 le recommande ; `RELEASE-CHECKLIST.md` §1 garde la case
« `CHANGELOG`/notes de version à jour » non cochée). Tant que ce support n'existe
pas, aucune dépréciation ne peut être annoncée selon cette politique : **l'étape
2 est bloquée** (§8.3).

### 3.3 Avertissement outillé

Un avertissement outillé est un signal que l'utilisateur reçoit **sans lire la
documentation** : un diagnostic, un champ JSON, un message d'outil. Aujourd'hui,
**aucun des trois n'existe pour une dépréciation** :

| Canal | État réel | Pourquoi |
|---|---|---|
| Diagnostic `W-*` | **impossible sans changement normatif** | la liste des avertissements est fermée (DIAG-1.6) ; ajouter `W-…` est une modification de spec réservée à l'humain |
| Champ JSON `deprecated` | **non implémenté** | proposé par Q-COMPAT-5 (`compatibility.md` §9) ; `crates/ravum-lang-cli/src/json.rs` n'émet aucun champ de ce type ; ajout additif possible **après** décision |
| Message d'outil (stderr) | **non implémenté** | aucun code ne mentionne `deprecat` (fichiers `*.rs` du dépôt, 0 occurrence) ; les messages texte sont hors contrat (DIAG-6.6) mais restent un canal lisible |
| `window/showMessage` LSP | **non implémenté** | aucune occurrence dans `crates/ravum-lang-lsp/src` |

**DEP-3.1 (proposé).** Une dépréciation **ne peut pas revendiquer un
avertissement outillé** tant que §8.2 n'a pas choisi un canal. Une dépréciation
qui *exige* un avertissement est donc **bloquée**, et doit être écrite comme
telle : `NOT_AVAILABLE`, jamais « averti ».

**DEP-3.2 (proposé).** Le canal retenu ne doit pas affaiblir une garantie
existante : un `W-*` de dépréciation ne peut pas remplacer une erreur (DIAG-1.5),
et un champ `deprecated: true` ne peut pas changer le verdict d'un programme.

### 3.4 Délai minimal

Le délai se compte **en versions publiées**, pas en jours : aucune cadence de
release n'est décidée (D3) et
[`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §8 exclut
volontairement les estimations de durée. **Aucune durée calendaire n'est promise
ni estimée ici.**

**DEP-3.3 (proposé).** Pour le CLI, le JSON, le LSP et le manifeste :
**une version mineure publiée** entre l'annonce et la première version qui
n'accepte plus l'élément, puis **une version mineure publiée supplémentaire**
entre l'avertissement et le retrait effectif. Une version publiée se définit
comme un tag annoté + artefact + manifeste de provenance + signature
(P3.6/P3.7 de [`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md)) ; une **release
candidate interne non publiée ne compte pas**.

**Conséquence à assumer** : **le compteur est à zéro.** Aucun tag, aucune
release, aucune signature (§0). Le délai minimal n'a donc jamais couru et
**aucun élément ne peut être déprécié au sens « release » aujourd'hui**. Pendant
le draft, ce qui existe est une **révision** `draft.N+1` (COMPAT-2.1) : plus
rapide, mais ce n'est pas une dépréciation et cela ne doit pas être présenté
comme telle.

### 3.5 Migration

- La migration est un **patch de source** proposé, revu et testé ; une IA peut
  proposer le correctif, pas modifier le contrat (COMPAT-6.1).
- Procédure imposée (COMPAT-6.2) : `check --json` sur l'ancienne édition →
  liste des diagnostics de rupture → patch → `check`/`test` sur la nouvelle
  édition → comparaison des résultats observables → revue humaine.
- **Aucun outil de migration automatique n'existe** (`ravum-lang migrate` n'est
  pas une commande — confirmé par `ravum-lang --help` ; COMPAT-6.4 le dit).
- Pour le langage, la migration inclut le **corpus** : un cas migré cite la règle
  qui l'exige et aucune sortie attendue n'est modifiée sans justification
  (`CONFORMANCE-BUILD-CONTRACT.md` §6.3 est l'exemple à suivre).
- Pour le JSON, le consommateur migre **avant** le bump ; l'ancien champ reste
  émis jusqu'au bump (DIAG-9.3).
- Pour le manifeste, un champ déprécié reste écrit ; le lecteur tolère son
  absence future.
- Pour le LSP, le client migre vers la capacité de remplacement avant son
  retrait.

### 3.6 Suppression

| Interface | Suppression possible ? | Conditions |
|---|---|---|
| Code de diagnostic | **non** en Core 1.0 (DIAG-9.1) | nouvelle édition majeure seulement ; le code reste documenté `deprecated`, jamais réutilisé |
| Fonctionnalité du langage | oui, en édition majeure (COMPAT-2.3) | `core-1.x` reste supportée pendant une migration documentée |
| Drapeau CLI | oui, après le cycle | pendant le draft, COMPAT-5.5 autorise un changement d'options sans cycle ; après publication, §3.4 s'applique |
| Champ JSON obligatoire | oui, avec **bump** de `schema_version` | §2.2, DEP-2.3 |
| Champ JSON optionnel | oui, avec bump si documenté comme contractuel | `success` est dans ce cas tant que §8.4 n'est pas tranché |
| Capacité LSP | oui, après le cycle | §2.4, DEP-2.6 |
| Champ du manifeste | oui, avec bump | §2.5, DEP-2.7 |

**DEP-3.4 (proposé).** Une suppression n'est jamais silencieuse : elle apparaît
dans le CHANGELOG, dans le registre §5 (statut `REMOVED`, ligne conservée) et dans
les notes de version, avec la version qui la réalise et le remplacement.

### 3.7 Qui décide

| Acte | Agent | Responsable / mainteneur |
|---|---|---|
| Rédiger la proposition/RFC, mesurer, vérifier | oui | — |
| Ajouter un code de diagnostic (registre + émission + audit vert) | oui, tant que la spec DRAFT le permet | notification ; l'agent ne ratifie rien |
| Ouvrir la liste des avertissements (DIAG-1.6) | **non** | oui, décision normative |
| Choisir le canal d'avertissement (§8.2) | **non** | oui |
| Marquer un code/une ligne `deprecated` dans la spec | **non** | oui |
| Déclarer une dépréciation effective | **non** | oui |
| Ratifier une RFC | **non** (règle 2 de `docs/rfc/README.md`) | oui |
| Publier une version, taguer, signer | **non** | oui (D3/D9) |

## 4. Règles par interface et par cycle

Lecture : « annonce » = étape 3.2, « avertissement » = étape 3.3, « délai » =
étape 3.4 (en versions publiées), « migration » = étape 3.5, « retrait » = §3.6.

| Interface | Annonce | Avertissement outillé | Délai minimal | Migration | Retrait | Décideur |
|---|---|---|---|---|---|---|
| Drapeau CLI | note de décision + CHANGELOG | **aucun canal aujourd'hui** (stderr possible, non implémenté) | 1 version mineure publiée avant refus, 1 de plus avant retrait | script de l'utilisateur ; le drapeau de remplacement existe et est testé | après le cycle | humain |
| Commande CLI | note de décision + CHANGELOG | aucun | idem | idem | après le cycle | humain |
| Code de sortie `0/1/2/3` | CHANGELOG | aucun | **non applicable** : signification contractuelle (COMPAT-5.4) | consommateur | **jamais** pour une signification existante | humain |
| Champ JSON obligatoire | CHANGELOG + note de migration | champ `deprecated` optionnel **si** §8.2 le choisit | bump seulement, pas de retrait dans `schema_version: 1` | consommateur avant bump | au bump, avec note | humain |
| Champ JSON optionnel | CHANGELOG | idem | idem | consommateur | au bump | humain |
| Code de diagnostic | §3 de la spec + audit vert ; **ajout** seulement | aucun (`W-*` fermé) | ajout : aucun délai ; retrait : jamais en Core 1.0 | remplacement documenté dans le §3 | **jamais** en Core 1.0 (DIAG-9.1) | humain pour toute modification de la spec |
| Capacité LSP | CHANGELOG + doc LSP | aucun (`showMessage` absent) | 1 version publiée du serveur annonçant le remplacement | client | après le cycle | humain |
| Champ du manifeste | CHANGELOG + `INSTALL.md` | aucun | 1 version publiée avant arrêt d'écriture | lecteur tolérant | au bump | humain |
| Fonctionnalité / règle du langage | **RFC obligatoire** + journal `draft.N+1` | aucun (`W-*` fermé) | draft : immédiat avec RFC + migration ; publié : édition majeure (COMPAT-2.3) | patch de source + migration de corpus (COMPAT-6.1/6.2) | édition majeure, `core-1.x` supportée pendant la migration | humain (RFC `ACCEPTED`) |

Ces règles portent une contradiction qu'il faut lire ensemble : le délai est
exprimé en versions publiées, et **aucune version n'a été publiée**. Ces règles
sont donc prospectives ; elles ne décrivent aucun événement passé.

## 5. Registre de dépréciations

Registre unique, tenu dans ce document. Une ligne est ajoutée à l'étape 3.2
(annonce) avec sa preuve, et **n'est jamais supprimée** : une dépréciation
terminée passe en `REMOVED` en conservant la ligne (DEP-3.4). Aucune ligne ne
s'ajoute sans décision humaine (§3.7).

**État au 2026-11-14 : vide — 0 dépréciation enregistrée.** Ce tableau est vide
parce qu'aucune interface n'est dépréciée, pas parce qu'aucune ne le mériterait :
la liste des points ouverts est en §6.2 et les décisions en §8.

| ID | Interface | Élément | Annonce (date, version) | Remplacement | Avertissement | Retrait prévu | Statut | Décision / preuve |
|---|---|---|---|---|---|---|---|---|
| _(aucune)_ | — | — | — | — | — | — | — | — |

## 6. Ce qui est déjà outillé, et ce qui manque

### 6.1 Outils existants (exécutés le 2026-11-14)

| Outil | Commande | Câblage | Ce qu'il prouve réellement | Ce qu'il ne prouve pas |
|---|---|---|---|---|
| Audit du registre | `python tools/registry_audit.py` → exit 0 | `.github/workflows/gate-contracts.yml:34` | 108 codes enregistrés, 0 non enregistré, 0 jamais émis, 0 `W-*` hors DIAG-1.6 | le **sens** d'un code n'a pas changé ; un code construit dynamiquement échapperait au scan de littéraux (`crates/**/*.rs`) |
| Contrat JSON | `python tools/json_contract.py` → exit 0 | `.github/workflows/linux-qual.yml:218-219` | les MUST §7/§8 sur **6 scénarios** (`check` propre/erreur/warning/manifeste, `test`, `doctor`) : 0 violation | `build`, `run`, `lint`, `doc`, `fmt`, `init` (non couverts) ; la tension `doctor`/DIAG-7.5 reste ouverte |
| Audit des drapeaux | `pwsh -File scripts/cli-flag-audit.ps1` → exit 0 | `.github/workflows/linux-qual.yml:214` | **22/22** drapeaux de la ligne `USAGE` sont acceptés par le parseur | que l'invocation réussit : pendant l'audit, `watch --release` et `watch --run` sont `PASS` alors que le build échoue (`E-MOD-MANIFEST`) ; le critère est « pas rejeté comme option inconnue » |
| Tests LSP protocole | `cargo test -p ravum-lang-lsp`, `cargo test -p ravum-lang-cli --test lsp_client` (cités par `LSP.md` §5) | `.github/workflows/linux-qual.yml:74` (`cargo test --workspace --all-targets`) | framing, positions, changements incrémentaux, oracle froid/chaud, cache de parse (`parse_cache.rs`) | l'intégration IDE, couverte séparément |
| Session VS Code | `node tools/vscode-ravum/test/runTest.js` (`VSCODE.md` §2) | `.github/workflows/vscode-e2e.yml` | 10 assertions d'API `vscode` sur une session réelle, 10 PASS (auto-déclaré par `VSCODE.md`) | rien sur les méthodes non implémentées ; voir la contradiction §6.2, point 10 |
| Tests de contrats CLI | `cargo test -p ravum-lang-cli` | `.github/workflows/linux-qual.yml:74` | `schema_version`, codes de sortie (`exit_codes.rs`), additivité de `timings` (`timings_contract.rs`), contrat du lanceur de tests (`test_runner_contract.rs`) | les champs non testés (`success`) |
| Contrat `check ⇒ build` | `ravum-conformance run --verify-build` | `.github/workflows/linux-qual.yml` (job `conformance`) | aucune divergence connue sur le corpus mesuré, à un commit et une toolchain | l'absence de formes inconnues ; c'est un filet de non-régression pour le langage |
| Intégrité documentaire | `python3 tools/validate_repo_docs.py` | `.github/workflows/docs-check.yml:20` | 42 ADR `PROPOSED`, liens du README racine | **pas** les liens des autres documents : un lien mort dans ce fichier ne serait pas détecté par la CI |

### 6.2 Ce qui manque (chaque absence est vérifiée)

1. **Aucun mécanisme d'avertissement de dépréciation** : recherche de `deprecat` sur tous les fichiers `*.rs` du dépôt → 0 occurrence ; `json.rs` n'a aucun champ `deprecated` ; la liste `W-*` est fermée (DIAG-1.6). Conséquence : §3.3 est `NOT_AVAILABLE`.
2. **Aucun CHANGELOG de projet** : aucun `CHANGELOG*` hors `tools/vscode-ravum/node_modules/` ; `RELEASE-CHECKLIST.md` §1 le garde non coché ; Q-COMPAT-1 le dit absent. Conséquence : §3.2 est bloquée.
3. **Aucun outil de migration** : `ravum-lang migrate` n'existe pas (COMPAT-6.4 ; `--help` ne le liste pas).
4. **Aucune garde de version en CI** (COMPAT-5.5 est un SHOULD) : la CI exécute `--version` pour vérifier que le binaire répond (`.github/workflows/linux-qual.yml:262-263`, « grep -q ravum »), pas pour échouer si la version majeure change.
5. **Aucun schéma JSON machine-lisible** : le contrat est de la prose (§7) plus un validateur Python. Le seul `*.schema.json` trouvé dans l'arbre appartient à une dépendance npm de l'extension VS Code (`tools/vscode-ravum/node_modules/vscode-languageserver-protocol/metaModel.schema.json`).
6. **`success` hors contrat outillé** : émis (`json.rs:30`) et documenté (CLI README) mais absent de `diagnostics.md` §7.2 et de `json_contract.py` ; aucune suppression ne serait détectée (§2.2).
7. **Trois commandes seulement sous contrat JSON** : `json_contract.py` couvre `check`, `test`, `doctor` ; le §7.2 de la spec énumère neuf valeurs de `command`.
8. **Tension `doctor` / DIAG-7.5** : `doctor --json` émet des chemins absolus (`cache_dir`, `cwd`, `executable`, `package_root`) que DIAG-7.5 interdit aux champs dépendants de la machine ; `json_contract.py` la classe en **tension** et demande une décision humaine.
9. **`check --help` est une erreur d'usage (exit 2)** : l'aide par commande se demande par `help check`. Un script qui suit la convention `cmd --help` obtient un code 2 ; c'est observé, pas documenté comme une règle.
10. **Contradiction documentaire LSP** : [`LSP.md`](../qualification/LSP.md) §4 écrit « intégration éditeur réelle : **non testée dans VS Code** », alors que [`VSCODE.md`](../qualification/VSCODE.md) documente une session VS Code réelle à 10/10 PASS. Les deux ne peuvent pas être vrais ensemble ; ce document ne tranche pas (§8.6).
11. **Registre des dépréciations non machine-vérifié** : le tableau §5 n'est lu par aucun outil ; une ligne ajoutée à la main ne casserait rien si elle était fausse.
12. **Pas d'inventaire des consommateurs** : aucune liste de qui dépend de quel drapeau/champ ; une annonce ne peut donc pas nommer les consommateurs touchés, seulement les décrire.
13. **Aucune assertion CI sur le `schema_version` du manifeste d'installation** : le fichier est vérifié présent (`linux-qual.yml:261`) et ses champs contrôlés (`qualification-evidence.yml`), pas sa version de schéma.

## 7. Ce que cette politique ne garantit pas

- **Aucune release, aucun tag, aucune signature n'existe** (§0). Le cycle §3
  n'a jamais tourné ; « délai minimal » est un compteur à zéro, pas un engagement
  calendaire.
- **Elle ne ferme pas R10** : elle fournit la pièce « politique de dépréciation
  écrite » ; le critère exige aussi que CLI/JSON/registre/LSP soient stables, et
  l'état de ces quatre points est décrit en §2 et §6, pas amélioré.
- **Elle ne ratifie rien** : `compatibility.md` reste `DRAFT` ; les règles
  `DEP-*` sont `PROPOSED` jusqu'à approbation humaine ; un agent ne peut ni les
  accepter ni les appliquer.
- **Elle ne promet pas que rien ne cassera** : pendant le draft, COMPAT-2.1
  autorise tout changement journalisé. La politique est un **processus**, pas une
  garantie de stabilité.
- **Elle ne couvre pas la plateforme historique** (`ravum-*`) ni ses objets
  `schema_version` : aucune garantie n'y est impliquée.
- **Elle ne prouve aucun sens** : le gel du registre est une cohérence de liste ;
  l'audit ne détecte pas un code dont la signification a changé.
- **Elle ne produit aucune mesure de temps** : aucune cadence, aucune date, aucun
  délai calendaire — les estimations de durée sont volontairement absentes du
  dépôt ([`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §8).
- **Elle ne remplace pas** [`PROVENANCE.md`](PROVENANCE.md),
  [`RELEASE-CHECKLIST.md`](RELEASE-CHECKLIST.md),
  [`KEY-CUSTODY.md`](KEY-CUSTODY.md),
  [`SIGNATURES.md`](../qualification/SIGNATURES.md), ni
  [`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md).

## 8. Décisions humaines requises

1. **Ratifier ou rejeter cette politique** (`PROPOSED` → `ACCEPTED`/`REJECTED`
   par le responsable), et dire à quelle date elle entre en vigueur.
2. **Choisir le canal d'avertissement** : (a) documentation seule, conformément à
   COMPAT-3.6 — aucun avertissement outillé ; (b) ouvrir DIAG-1.6 pour créer un
   `W-*` de dépréciation — modification normative ; (c) ajouter un champ JSON
   optionnel `deprecated` — additif, proposé par Q-COMPAT-5, non implémenté.
   Sans ce choix, §3.3 reste `NOT_AVAILABLE`.
3. **Créer le support d'annonce** : un `CHANGELOG` de projet (Q-COMPAT-1) ou une
   règle équivalente disant où une dépréciation est annoncée. Sans lui, §3.2 est
   bloquée.
4. **Trancher `success`** : l'ajouter au §7.2 de la spec et à
   `tools/json_contract.py` (il devient contractuel), ou le documenter comme
   toléré et non garanti. Aujourd'hui, il est documenté mais non testé.
5. **Trancher la tension `doctor`/DIAG-7.5** : documenter l'exception « `doctor`
   rapporte l'environnement » ou retirer les chemins absolus du résumé.
6. **Réconcilier `LSP.md` §4 et `VSCODE.md`** (l'un dit « non testé dans VS
   Code », l'autre documente 10/10 PASS).
7. **Décider le sort de `check --help`** : convention acceptée (et documentée)
   ou à implémenter.
8. **Décider la cadence de release (D3)** qui donne un sens à « une version
   mineure publiée » : sans publication, aucun délai §3.4 ne peut courir.
9. **Décider si le registre §3 de la spec reçoit une colonne `deprecated`**
   (modification de spec, humaine) ou si la dépréciation des codes reste
   purement documentaire.
10. **D9** : la signature des artefacts n'existe pas ; quand elle existera, un
    champ de manifeste déprécié devra suivre le cycle signé (P9.1), ce qui n'est
    pas décrit ici.

## 9. Sources

Tous les chemins ont été vérifiés présents dans l'arbre au 2026-11-14.

**Interfaces et contrats**

- [`README.md`](../../README.md) — état du projet, commandes, LSP.
- [`AGENTS.md`](../../AGENTS.md) — mandat, « sans tag ni release », interdiction d'affaiblir un contrat.
- [`crates/ravum-lang-cli/README.md`](../../crates/ravum-lang-cli/README.md) — commandes, codes de sortie, schéma JSON, `schema_version: 1`.
- [`crates/ravum-lang-cli/src/args.rs`](../../crates/ravum-lang-cli/src/args.rs) — chaînes `USAGE` (source de l'audit des drapeaux).
- [`crates/ravum-lang-cli/src/json.rs`](../../crates/ravum-lang-cli/src/json.rs) — `SCHEMA_VERSION = 1`, structure de l'enveloppe.
- [`crates/ravum-lang-lsp/src/server.rs`](../../crates/ravum-lang-lsp/src/server.rs) — capacités `initialize`, `positionEncoding`, `-32601`.
- [`spec/language/diagnostics.md`](../../spec/language/diagnostics.md) — §3 registre, §7 JSON, §8 codes de sortie, §9 stabilité (DIAG-1.x, 7.x, 8.x, 9.x).
- [`spec/language/compatibility.md`](../../spec/language/compatibility.md) — COMPAT-1…7, Q-COMPAT-1 et Q-COMPAT-5.
- [`spec/language/abi.md`](../../spec/language/abi.md) — absence d'ABI.
- [`spec/language/features.md`](../../spec/language/features.md), [`spec/language/unsupported.md`](../../spec/language/unsupported.md) — surfaces supportées/refusées.

**Release, installation, provenance**

- [`INSTALL.md`](INSTALL.md) — manifeste `ravum-install.json`, `schema_version: 1`, champs.
- [`PROVENANCE.md`](PROVENANCE.md) — `git_dirty`, absence de signature.
- [`RELEASE-CHECKLIST.md`](RELEASE-CHECKLIST.md) — case CHANGELOG non cochée.
- [`D3-D4-D9-POLICIES.md`](D3-D4-D9-POLICIES.md) — fusion/promotion/release, P3.6/P3.7, signature.
- [`KEY-CUSTODY.md`](KEY-CUSTODY.md) — custody de clé (citée par D3-D4-D9).
- [`SIGNATURES.md`](../qualification/SIGNATURES.md) — mécanisme qualifié, clé absente.

**Qualification**

- [`LSP.md`](../qualification/LSP.md) — capacités, refus explicites, cache de parse, mesures.
- [`VSCODE.md`](../qualification/VSCODE.md) — session VS Code réelle (T7).
- [`CONFORMANCE-BUILD-CONTRACT.md`](../qualification/CONFORMANCE-BUILD-CONTRACT.md) — contrat `check ⇒ build`, champs additifs, migration `examples/stats`.
- [`PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) — plateformes réellement exercées (macOS `NOT_RUN`).

**RFC**

- [`docs/rfc/README.md`](../rfc/README.md) — règles RFC, statut par défaut, clôture humaine.
- [`RFC-D-COERCE-1-MATRIX.md`](../rfc/RFC-D-COERCE-1-MATRIX.md) — précédent de durcissement avec migration.
- [`RFC-D13-STATICITY.md`](../rfc/RFC-D13-STATICITY.md) — précédent d'acceptation nouvelle.

**Outils et CI**

- [`tools/registry_audit.py`](../../tools/registry_audit.py) — audit bidirectionnel du registre.
- [`tools/json_contract.py`](../../tools/json_contract.py) — validation §7/§8 sur le binaire réel.
- [`tools/features_coverage.py`](../../tools/features_coverage.py) — traçabilité règle → cas (porte `--strict`).
- [`tools/validate_repo_docs.py`](../../tools/validate_repo_docs.py) — intégrité documentaire (README seulement).
- [`scripts/cli-flag-audit.ps1`](../../scripts/cli-flag-audit.ps1) — drapeaux documentés vs parseur.
- [`scripts/install.ps1`](../../scripts/install.ps1), [`scripts/install.sh`](../../scripts/install.sh) — manifeste et parsing de `--version`.
- [`linux-qual.yml`](../../.github/workflows/linux-qual.yml) — audits, contrat JSON, installateur.
- [`gate-contracts.yml`](../../.github/workflows/gate-contracts.yml) — registre et traçabilité.
- [`qualification-evidence.yml`](../../.github/workflows/qualification-evidence.yml) — contrat d'installateur.
- [`docs-check.yml`](../../.github/workflows/docs-check.yml) — contrôle documentaire.
- [`conformance/cases/`](../../conformance/cases/) — corpus (483 cas au 2026-11-14).

**Critère**

- [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) — R10 et état `PARTIAL`.
- [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) — registre canonique des gates (fait foi).

# MVP-0c — rapport consolidé (jalon du 9 novembre 2026)

Statut : **rapport de jalon, pas une clôture de goal**. Les lots L0 à L4 ont produit
du code, des tests et des mesures vérifiées ; L2 (implémentation du groupement), la
campagne S8 avec vrais agents, le branchement CLI final et l'intégration GitHub
restent ouverts. Aucune fusion, aucun ADR accepté, aucune licence, aucune release.
Les 42 ADR restent `PROPOSED`.

Branche de travail : `feat/mvp0c-hardening` (empilée sur `feat/mvp0b-hardening`,
elle-même empilée sur `feat/3-mvp0a-continuous-loop`).
PR de travail : **#82** (draft, base `feat/mvp0b-hardening`). Suivi : **#81**.

## 1. État initial et final des deux dépôts

| | Initial (audit L0, 2026-11-09 ~00:35) | À ce jalon |
|---|---|---|
| Code | `feat/mvp0b-hardening` @ `ea7200c`, arbre propre, PR #78/#80 `MERGEABLE` | `feat/mvp0c-hardening` @ `5f36475` (21 commits, 162 fichiers, +57 607/−227 au-dessus de `ea7200c`), PR #82 draft |
| `dev` | `7841102` docs-only (aucun crate, workflow documentaire `docs-check.yml`) | inchangé (aucune fusion) |
| Vault | `master` propre, 8 sessions Ravum | sessions MVP-0c L0 + L1 + L2 + L3 + L4 + seconde vague, `master` vérifié |
| Tests | 347/0 (54 suites) | **495/0 (78 suites)**, exit 0, local |
| ADR/RFC | 42 ADR + 2 RFC `PROPOSED` | inchangé |
| Issues | 78 ouvertes | 78 ouvertes + #81 (suivi MVP-0c) |

## 2. Branches et SHA

- `main` : inchangée (local = `origin/main` + 1 commit local préexistant non poussé).
- `dev` : `7841102` — inchangée.
- `feat/3-mvp0a-continuous-loop` : `6b37ad3` (tête PR #78).
- `feat/mvp0b-hardening` : `ea7200c` (tête PR #80).
- `feat/mvp0c-hardening` : `5f36475` (tête PR #82 au moment de ce rapport).

## 3. PR et issues

- **#78** : ouverte, `MERGEABLE`, 186 fichiers, +25 528/−6, 22 commits ; audit
  confirme 0 secret, 0 binaire, 0 fichier supprimé, aucun fichier changé > 500 Ko.
- **#80** : ouverte, `MERGEABLE`, 304 fichiers, +128 220/−304, 54 commits, empilée
  sur #78 ; retarget vers `dev` nécessaire après fusion de #78.
- **#82** : draft, 162 fichiers, +57 607/−227, 21 commits, base
  `feat/mvp0b-hardening` ; ne doit pas être fusionnée avant la décision sur #78/#80.
- **#81** : suivi d'exécution MVP-0c (six lots, gates, non-claims).

## 4. Subagents réellement utilisés (8 teammates durables, périmètres disjoints)

| Worker | Rôle dans MVP-0c | Livrables principaux |
|---|---|---|
| `git-txn` | audit PR (#78/#80) puis robustesse publication Git | `MVP-0c-pr-audit.md`, `git_publication_crash.rs` (15 tests) |
| `incremental` | L1 inter-crates puis branchement daemon | `ravum-unit-graph`, fixture multicrate, daemon externs, `externs_integration.rs` (7 tests) |
| `collab-4` | L2 phase 1, conflits multi-agents, classification transitoire | `grouping`, `grouping-multiagent`, correctif retry vérification |
| `collab-2-3` | L3 statuts sémantiques puis adaptateur oracle L1 | protocole v2, `ravum-collab-oracle` (14 tests) |
| `impact-ci` | L4 CI par impact, puis correctif selftest dev-deps | selftest 56 cas/583 assertions, gardes CI |
| `browser-e2e` | L4 E2E navigateur et trou de couverture | `MVP-0c-e2e.md`, preuve 10/10 local+CI |
| `audio-probe` | L4 stress DSP puis correctif crash CI | `dsp_stress.rs` (9 tests), correctif livelock/watchdog |
| `s8-bench` | S8 réel : protocole, pilote, procédure | `benchmarks/s8-mvp0c/**`, `MVP-0c-s8.md` |

Le Lead a réalisé : audit L0, plan, audit critique des livraisons, corrections
transverses (runner de tests, evidence opt-in, encodage `impact.ps1`), intégration,
vérifications indépendantes, rapports et vault.

## 5. Modifications intégrées (21 commits)

- **L0** : audit vérifié des deux dépôts, suite 347/0 reproduite, plan des six lots,
  audit structuré des PR.
- **L1** : crate `ravum-unit-graph` (modèle `cargo metadata --no-deps`, identités
  `content/context/interface/externs/verdict`, compilation topologique
  `--extern`, fallback Cargo, cache JSON, statuts sans `PROVED`), fixture
  `fixtures/multicrate`, extension daemon schéma 1→2 (externs contenu-adressés).
- **L1.2** : `Daemon::analyze_with_externs` (identité + cache incluant les externs,
  artefacts content-addressed, fermeture transitive par snapshot).
- **L2** : mesure du groupement de patches, puis mesure des conflits multi-agents.
- **L3** : protocole v2 (niveaux d'évaluation versionnés), `SemanticOracle`
  injectable, correctif AUTO_SAFE, adaptateur L1 `ravum-collab-oracle`, robustesse
  publication Git sous crash.
- **L4** : CI par impact durcie, E2E revalidés, stress DSP, reclassification des
  échecs transitoires de vérification.
- **S8** : protocole pré-enregistré, pilote scripté mesuré, procédure de campagne
  réelle avec statistiques d'usage du harnais.
- **Outillage** : `tools/agent-usage/usage.mjs`, `scripts/run-tests.ps1` corrigé,
  regeneration d'évidence opt-in, `scripts/impact.ps1` UTF-8 BOM.

## 6. Analyse inter-crates obtenue (L1)

Cas central réel (preuve `evidence/mvp0c-cross-crate-2026-11-09.json`) : changement
de signature `core::scale(f64)` → `core::scale(f64, f64)`.

- `core` : `TARGETED_OK` (rustc invoqué).
- `api` : **`ERROR` avec sources inchangées** (`api_content_id_unchanged: true`,
  `api_hit: false`), diagnostic réel `E0061` (×2).
- `consumer`/`application` : revalidés par l'oracle Cargo, `valid=false`, aucun
  verdict antérieur réutilisé.
- Ajout d'interface compatible : `core`, `api` et `consumer` revalidés réellement
  (3 rustc, `hit=false`), `valid=true` — propagation transitive démontrée.
- **Déterminisme mesuré** : deux compilations identiques → même SHA-256 `.rmeta`
  pour `core`, `api`, `consumer`.
- 16 cas négatifs du mandat couverts + HIT + déterminisme (18 tests).
- Limites : interface_id = SHA-256 du `.rmeta`, donc **dépendant du chemin absolu**
  du workspace temporaire (conservateur, non portable entre checkouts). Expérience
  Lead : `rustc --remap-path-prefix=<racine>=/virtual` produit des `.rmeta`
  identiques dans deux répertoires — piste d'amélioration mesurée, non implémentée.

## 7. Daemon : externs contenu-adressés (L1.2)

- Externs validés (nom, SHA-256, taille, unicité), inclus dans `snapshot_id` et dans
  la clé de cache ; chemin sans externs identique octet pour octet (v1).
- Mesure (`--suite cross-crate`, 3 rounds, machine chargée) : isolé froid 237 ms
  (1 rustc) ; chaîné froid 681 ms (3 rustc) ; **hits 8–9 ms** ; extern changé sans
  changement de sources ⇒ **miss réel 228 ms**, nouveau `snapshot_id`, diagnostic
  `E0061` réel ; aucun HIT périmé. Aucun gain de vitesse revendiqué (la chaîne
  coûte 3 compilations + artefacts + fermeture).
- Limite : un extern fourni par un tiers sans sidecar ne fournit pas ses
  dépendances transitives ⇒ diagnostic réel, jamais d'interface inventée.

## 8. Mesures de compilation et comparaison

### 8.1 Groupement de patches (L2 phase 1, `evidence/mvp0c-grouping-2026-11-09.json`)

6 éditions compatibles, 5 répétitions, ordre alterné :

| Mode | Temps total p50 | Analyses | rustc |
|---|---:|---:|---:|
| Séquentiel | 1 159 ms | 6 | 6 |
| Groupé w=0 | **204 ms** | 1 | 1 |
| Groupé w=25 | 214 ms | 1 | 1 |
| Groupé w=50 | 240 ms | 1 | 1 |
| Groupé w=100 | 308 ms | 1 | 1 |
| Incompatible + reprise | 1 597 ms | 9 | 9 |

Gain apparié : −74 % à −83 % ; 5 invocations rustc économisées. Lot incompatible :
refus 184 ms puis reprise 1 596 ms ≈ 1,4× le séquentiel. Disque d'état : 1,24 Mo
(groupé) vs 2,51 Mo (séquentiel).

### 8.2 Rentabilité conditionnelle (L2.2, `mvp0c-grouping-multiagent`)

Décisions réelles du binaire `ravum-collab` (5 répétitions, 20/20 conformes) :
fichiers distincts ⇒ AUTO_SAFE ×3 ; symboles distincts d'un même fichier ⇒
AUTO_SAFE ×5 ; même fonction ⇒ AUTO_SAFE, CONFLICT, CONFLICT ; dépendance non
déclarée ⇒ AUTO_SAFE, MANUAL_REQUIRED, AUTO_SAFE.

Modèle calibré (coûts mesurés phase 1) : seuil de bascule `p*` = 27,2 % (N=3, w=0),
40,8 % (N=4), 54,4 % (N=5), 68,0 % (N=6). Avec le mix mesuré (50 % de lots
conflictuels, corpus volontairement chargé), le groupement **n'est pas rentable**
(−33,7 % à −48,6 % selon la fenêtre). Verdict : pas d'activation par défaut.

### 8.3 S8 (pilote, `evidence/mvp0c-s8-pilot-2026-11-09.json`)

9 campagnes scriptées (3 répétitions × A/B/C), 12/12 tests verts, 0 changement perdu.

| Stratégie | Temps total p50 (run initial) | Verdict |
|---|---:|---|
| A — copies par agent + intégration | **2 650 ms** | plus rapide |
| B — répertoire partagé + détection | 3 062 ms | **le plus lent** |
| C — séquentiel | 2 914 ms | intermédiaire |

Le pilote **confirme** que le répertoire partagé n'est pas plus rapide et **infirme**
le classement C < A du MVP-0b (sans coût Git, A passe devant C). Écarts 9–15 % avec
chevauchement des dispersions ; build = 84–90 % du temps. Tokens et supervision
**non mesurés** au pilote ; la procédure de campagne réelle (sessions DSH → agents,
tokens via `tools/agent-usage/usage.mjs`, budget borné) est documentée et testée sur
le chemin `live prepare`.

## 9. Collaboration et vérification (L3/L4)

- **Protocole v2** : `COORDINATOR_SCHEMA_VERSION = 2`, `EvaluationLevel`
  (`TEXTUALLY_APPLICABLE`, `SEMANTICALLY_CHECKED`, `TESTED`, `PROVED` borné,
  `REJECTED`, `MANUAL_REQUIRED`), journal v2 sur-ensemble avec v1 relu sans perte.
  Sous v2, **aucun `AUTO_SAFE` sur la seule applicabilité textuelle** (prouvé par
  4 tests + smoke CLI). 10/10 cas négatifs `fixtures/collab5`.
- **Adaptateur L1** `ravum-collab-oracle` : `INTERFACE_BREAK` bloquant quand un
  dépendant inchangé échoue après un changement de dépendance ; toute incertitude
  ⇒ `Unavailable`, jamais `Clear` ; bout en bout v2 ⇒ `REBASE_REVALIDATE` /
  `TEXTUALLY_APPLICABLE`, jamais `AUTO_SAFE`. Coût mesuré : médiane 5 079 ms,
  max 6 981 ms sur la fixture (6 fallbacks Cargo dominent, pas rustc).
- **Publication Git** : 15 tests de crash **réels** (processus tué aux étapes
  `pre-commit`, `commit-msg`, `cas prepared`, `cas committed`, `post-commit`),
  reprise `recover()` des deux côtés du point de commit, vérrou de maintenance,
  permissions Windows, fichiers générés préservés octet pour octet, double
  publication refusée. **Aucun défaut de sûreté découvert** ; 2 observations
  non bloquantes documentées (étiquette `ConcurrentWriter`, extensions de
  `GENERATED_PATTERNS`).
- **Vérification** : reclassification des échecs rustc **sans E-code** ⇒ retry
  borné unique puis `MANUAL_REQUIRED` (jamais `REJECT`) ; `E-code` ⇒ `REJECT`
  conservé ; `exit_code` + extrait stderr ≤ 2 Kio journalisés. Prouvé par 3
  scénarios de faux rustc piloté par `RAVUM_RUSTC`.
- **E2E navigateur** : 10/10 local et CI (artifact `browser-e2e-evidence`,
  Chrome 156 headless), **aucun skip**. Trou de couverture prouvé : `mvp-impact.yml`
  n'exécute aucun navigateur ; règle de déclenchement proposée, non appliquée.
- **Stress DSP** : 9 tests (10 000 swaps, 2 instances, stop/start ×50, buffers
  1..4096, destruction pendant fermeture, arrêt pendant swap, 20 modules ×3,
  4 000 activations concurrentes) — 0 allocation/désallocation sur le thread de
  rendu, **0 bloc mixé**. Défaut CI trouvé et corrigé : le test était en livelock
  (deux publieurs lockstep) et un watchdog mur-à-mur de 45 s appelait `abort()`
  (`STATUS_STACK_BUFFER_OVERRUN`) ; correctif = phases opposées déterminées,
  watchdog de progression 30 s, `StopOnDrop`, modules `no_std` réduits pour le
  budget (`prepare` 31,2 s → 3,6 s). 24 tests dsp verts (dont 15 existants).

## 10. Tests réussis / échoués / non exécutés

- **Réussis** : 495 tests, 78 suites, exit 0 (local, `--test-threads=1`,
  `CARGO_BUILD_JOBS=2`). Répartition vérifiée : daemon 115 (dont 7 externs + 2 wire),
  coordinator 137 (dont 15 crash + 3 transitoires), cli 51 (dont 8 nouveaux),
  ravum-batch 19, ravum-collab-oracle 14, unit-graph 21 (dont 2 rootapp),
  dsp-harness 24 (dont 9 stress), bench 5, reste du workspace inchangé.
- **Échoués** : 0 dans la campagne finale locale. **CI** : deux échecs réels
  observés sur `c86d1f0` (watchdog du stress DSP ; selftest impact périmé par les
  dev-deps DSP), tous deux corrigés ; la CI de `5f36475` est vérifiée séparément
  dans le message de jalon (voir §12).
- **Non exécutés / bloqués** : prouveur externe (Kani absent, Linux/WSL2 requis) ;
  CI par impact sur une vraie PR vers `dev` (bloquée : `dev` sans crate) ; campagne
  S8 avec vrais agents (procédure prête, non exécutée) ; hôte CLAP/VST3 réel (hors
  dépôt) ; ASIO (licence SDK) ; soak multi-heures ; `-C opt-level=0` (décision).

## 11. CPU / RAM / disque (mesures existantes, machine partagée)

| Mesure | Valeur observée |
|---|---|
| Daemon au repos | WorkingSet 4,78 Mo, 0,016 s CPU, 4 threads |
| Groupement (L2.1) | RSS pic harnais 7,6–8,4 Mo ; disque d'état 1,24–5,33 Mo selon mode |
| Multi-agents (L2.2) | RAM libre 2,4–2,8 Go ; charge 11–49 % |
| Oracle L1 | médiane 5 079 ms, max 6 981 ms par proposition ; 6 rustc + 6 `cargo check` + 2 metadata |
| Stress DSP | RSS +0,54 Mio sur 60 chargements ; 22,1 s pour la campagne d'évidence |
| CI impact | jobs 2 min 12 s / 3 min 32 s ; E2E +25–65 s quand déclenché |

La sonde WMI/CIM est tombée en panne par intermittence pendant la session ; les
mesures de charge ont été refaites via Node (`os.freemem`, `os.cpus`) et le harnais
S8 a été rendu indépendant de WMI. Aucune valeur n'est inventée quand la mesure
échoue ; les limites sont déclarées.

## 12. État CI (à la rédaction)

- `58220f5` : produit 42538867282 et impact 42538867273 **verts** (410 tests).
- `05350c7` : produit 42541321576 et impact 42541321622 **verts** (449 tests,
  E2E inclus).
- `c86d1f0` : **rouge** — deux défauts réels et distincts : watchdog mur-à-mur du
  stress DSP (`0xC0000409`) ; selftest impact périmé par les dev-deps DSP ajoutées.
- `5f36475` : correctifs poussés (livelock DSP + watchdog de progression ; selftest
  56 cas/583 assertions + garde CI) ; **CI produit 42545647860 et impact
  42545647923 : succès vérifiés**, après les 458 tests locaux / 0 échec.

## 13. Limites et garanties

- Aucun compilateur Ravum, aucune sémantique propre, aucun `PROVED` général, aucune
  garantie hard-RT, aucun XRUN officiel, aucune supériorité multi-agents revendiquée.
- L1 = fiabilité (l'analyse chaînée coûte plus cher que l'analyse isolée) ;
  L2 = mesures et seuils, **pas encore d'implémentation** du groupement.
- L'adaptateur oracle et le daemon ne sont **pas branchés** dans `ravum-cli` ; les
  tests d'intégration sont au niveau bibliothèque/binaire `ravum-collab`.
- S8 = agents scriptés déterministes ; tokens et supervision non mesurés.
- Identités `.rmeta` dépendantes du chemin (portabilité à améliorer) ; `MergeCheck`
  non étendu (exit code/stderr dans le résumé et la raison seulement).

## 14. Dette technique résiduelle

1. Groupement : pré-vol de conflits, snapshot de lot atomique avec CAS, repli,
   télémétrie `p`, interactions externs — non implémentés (design + seuils prêts).
2. CLI : commande de vérification inter-crates et injection `--oracle` non câblées.
3. S8 : campagne réelle avec subagents DSH non exécutée.
4. CI : règle E2E dans `mvp-impact.yml` non appliquée ; PR vers `dev` non exerçable.
5. Formatage : `cargo fmt --check` révèle ~305 fichiers non formatés par rustfmt
   1.95 (dérive de version) ; aucun gate fmt en CI. Commit de formatage dédié à
   décider, sans mélange avec les changements fonctionnels.
6. Protocole JSON-lines du daemon sans champ `externs` (API Rust seulement).
7. Prouveur externe (Kani) bloqué sur Windows ; harnais Linux/WSL2 à faire.
8. WMI/CIM instable localement ; `taskkill /T` pathologique sous charge (contourné
   hors dépôt pour un run E2E local ; CI non affectée).

## 15. Prochaines priorités

1. **Décision humaine #78/#80**, puis retarget #80 → `dev`, puis #82 → `dev` et
   re-vérification du diff réel (la CI par impact sur `dev` n'est pas exerçable
   avant).
2. Implémenter le groupement L2 conditionnel (fenêtre 50 ms, N max 8, pré-vol de
   conflits sans rustc, repli conservateur, télémétrie) et mesurer le gain réel
   multi-agents.
3. Brancher `ravum-unit-graph`/`ravum-collab-oracle` dans `ravum-cli`
   (`ravum unit-check`, `--oracle unit-graph`, `--protocol v2`).
4. Exécuter la campagne S8 réelle (27 sessions DSH, budget borné) avec tokens et
   supervision mesurés ; conserver le verdict même défavorable.
5. Formatage global + gate fmt (commit dédié) ; règle E2E dans la CI par impact.
6. Prouveur externe sur Linux/WSL2 si l'environnement le permet ; sinon documenter.

## 16. Décisions nécessitant approbation humaine

1. Fusion de #78 vers `dev`, puis retarget/fusion de #80, puis de #82.
2. Licence du code (aucune choisie) et gouvernance de l'outillage IA tiers commité
   (hooks `.claude/settings.json` à chemin absolu, skills sans notice).
3. Ratification éventuelle des ADR/RFC (aucune acceptation automatique).
4. Adoption éventuelle de `-C opt-level=0` (gain mesuré, code généré différent).
5. Budget et exécution de la campagne S8 réelle (tokens réels, temps de supervision).
6. ASIO (licence SDK Steinberg) et hôte CLAP/VST3 (périmètre hors dépôt).

## 17. Preuves et sessions Obsidian

Preuves versionnées : `evidence/mvp0c-{cross-crate,cross-crate-daemon,grouping,grouping-multiagent,verification-transient,impact,impact-dev-dep,collab,collab-oracle,publication,e2e,verification-retry,dsp-stress,dsp-stress-ci,s8-pilot}-2026-11-09.json`, plus `evidence/mvp0c-pr-audit.json`, `evidence/mvp0c-batch-2026-11-09.json`, `evidence/mvp0c-batch-e2e-2026-11-09.json`, `evidence/mvp0c-impact-e2e-2026-11-09.json`, `evidence/mvp0c-integration-audit-2026-11-09.json` et `evidence/mvp0c-s8-live-{A1,B1,C1,A2,B2,C2,A3,B3,C3}-2026-11-09.json`.

Sessions vault :
[L0 audit](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-09-mvp0c-l0-audit-and-integration.md),
[L1 cross-crate](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-09-mvp0c-cross-crate-incremental.md),
[L2 performance](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-09-mvp0c-build-performance.md),
[L3 sémantique](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-09-mvp0c-shared-workspace-semantics.md),
[L4 qualification](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-09-mvp0c-qualification.md).

## 18. Reproduction

```powershell
cd D:\App\Ravum
$env:CARGO_BUILD_JOBS=2
cargo test --workspace --offline -- --test-threads=1      # 458/0, 69 suites
cargo test -p ravum-unit-graph -- --test-threads=1         # 19/0 (dont cas central)
cargo test -p ravum-daemon -- --test-threads=1             # 113/0 (dont 7 externs)
cargo test -p ravum-workspace-coordinator -- --test-threads=1  # 137/0
cargo test -p ravum-collab-oracle -- --test-threads=1      # 14/0
cargo test -p ravum-dsp-harness -- --test-threads=1        # 24/0 (dont 9 stress)
node benchmarks/grouping-multiagent/run.mjs --reps 5       # valid=true, 20/20
node tools/impact/selftest.mjs                             # 56 cas / 583 assertions
node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3     # OK
```

Machine : Windows 11 Pro 26100, i7-13620H (10 cœurs/16 threads), 15,7 Go RAM,
rustc/cargo 1.95.0, Node 26.13.0, Python 3.13.7. Machine partagée avec d'autres
agents : les durées incluent la charge ambiante, déclarée dans chaque preuve.

## 19. Mise à jour du 9 novembre — seconde intégration (CLI, batch, S8 réel)

Cette section s'ajoute au rapport sans réécrire les résultats précédents ; elle
documente les lots livrés après le premier jalon.

### 19.1 Nouveaux lots livrés et vérifiés

| Lot | Contenu | Vérification |
|---|---|---|
| **L1.3** | externs contenu-adressés dans le **protocole JSON-lines** du daemon (stdio + TCP) : `WireExtern`/`WireAnalyzeRequest`, legacy inchangé, invalides refusés | daemon **115** tests (2 wire), stdio+TCP réels, `extern` changé ⇒ miss réel, `E0061` |
| **L1.4** | `content_id` du root package n'agrège plus les sources des membres (attribution par manifeste le plus proche) | unit-graph **21** tests (2 nouveaux), reproduction avant/après, CLI vérifié |
| **L2.3** | crate `ravum-batch` : collecteur borné (fenêtre 50 ms/max 100, N 6/max 8), pré-vol de conflits sans rustc, repli N+1 sans perte, télémétrie | **19** tests, preuve `mvp0c-batch` (30→5 analyses ; mixte p≈50 % sans perte) |
| **L4.9** | E2E navigateur déclenchés par la CI par impact (`browser_e2e_required`, étapes conditionnelles, artefact) | selftest **59 cas/655 assertions**, E2E local 10/10 en 24,1 s, fail-closed testé |
| **L4.10** | flake auto-GC (`runs >= 1` immédiat) → attente bornée 50 ms/10 s, assertions conservées | 5/5 sous charge + 5/5 au repos, 7/7 fichier |
| **L4.11** | preuves de test écrites uniquement si `RAVUM_WRITE_EVIDENCE=1` (arbre propre après tests) | avant/après `git status`, opt-in vérifié |
| **L5.1** | CLI : `ravum unit-check` (graphe réel, chaîne `--extern`, codes 0/1/2/3, workspace inchangé) et `collab-cycle --protocol v2 --oracle unit-graph` | cli **47** tests, signatures cassées ⇒ exit 1 + `E0308`, jamais `AUTO_SAFE` |

### 19.2 Campagne S8 réelle (27 subagents DSH, 9 campagnes, tokens mesurés)

Protocole pré-enregistré, ordre `A1 B1 C1 · C2 B2 A2 · A3 B3 C3`, même modèle,
mêmes tâches, 3/3 agents attribués par marqueur sur chaque campagne.

| Stratégie | Médiane (n=3) | Min–max | Tokens traités (médiane/campagne) | Tests |
|---|---:|---:|---:|---|
| A — copies + intégration | **111 110 ms** | 100 664–208 694 | ≈ 3,83 M | 9/9 campagnes 12/12, 0 perte |
| B — répertoire partagé + garde | 237 674 ms | 126 224–286 275 | ≈ 3,89 M | 9/9 campagnes 12/12, 0 perte |
| C — séquentiel | 528 481 ms | 269 594–611 003 | ≈ 3,13 M | 9/9 campagnes 12/12, 0 perte |

Verdict : **A < B < C** ; le workspace partagé **n'est pas plus rapide** que les
copies privées (+114 % médian), C est le plus lent par construction (tours
sérialisés + supervision). Tokens du même ordre entre stratégies ; aucun coût
monétaire exposé, aucun chiffre inventé. Détail et limites :
[`MVP-0c-s8.md`](MVP-0c-s8.md) §9, preuves `evidence/mvp0c-s8-live-*`.

### 19.3 Tests, CI et limites

- Suite locale finale : **495/0 (78 suites)**, exit 0, après tous les lots et le câblage CLI du collecteur.
- CI sur le commit d'intégration `51a1bae` : produit **42550250213 succès**,
  impact **42550250227 succès** (incluant la nouvelle règle E2E) ; le commit
  documentaire suivant déclenche un nouveau run, vérifié séparément.
- Limites : S8 n=3, une fixture, un modèle, machine partagée, supervision du Lead
  incluse dans le temps ; le pré-vol du collecteur est textuel (conflit sémantique
  imprévu ⇒ repli) ; `unit-check` copie le workspace (coût IO) ; E2E impact peut
  doubler les E2E quand `mvp.yml` tourne aussi (coût ~25–65 s, documenté) ;
  identités `.rmeta` dépendantes du chemin ; aucun `PROVED`.
- Reste ouvert (inchangé) : décision humaine #78/#80/#82, CI par impact sur une
  vraie PR vers `dev`, Kani (Linux/WSL2), ASIO/CLAP, implémentation du groupement
  conditionnel en production (le collecteur existe mais n'est pas encore branché au
  daemon/CLI). Le formatage global et le gate `fmt` ont été corrigés depuis (§20.5).

## 20. Mise à jour du 9 novembre — gate L2 franchi, audit final et E2E CI vérifiés

### 20.1 Gate L2 de bout en bout (avec vrai `cargo test`)

`fixtures/grouping` porte désormais **12 tests** (5 unitaires + 7 d'intégration
sous éditions cumulées). Protocole : 5 modes, 5 répétitions en ordre alterné,
`CARGO_TARGET_DIR` neuf par mode/répétition, `CARGO_BUILD_JOBS=2`, aucun réseau.
Modes : séquentiel (6 `cargo test` cumulatifs), groupé (1 lot), collecteur réel
`ravum-batch` (fenêtres 0 et 50 ms), incompatible + repli.

| Mode | cold p50 | warm p50 | total p50 | `cargo test` | tests |
|---|---:|---:|---:|---:|---|
| Séquentiel | 2 512 ms | 1 290 ms | **8 960 ms** | 6 | 12/12 |
| Groupé | 2 520 ms | 271 ms | **2 765 ms** | 2 | 12/12 |
| Collecteur w=0 | 2 492 ms | 276 ms | **2 777 ms** | 2 | 12/12 |
| Collecteur w=50 | 2 582 ms | 270 ms | **2 893 ms** | 2 | 12/12 |
| Incompatible + repli | 534 ms | 1 303 ms | **9 861 ms** | 8 | 6 acceptés / 1 rejeté |

- **Gain : 6 195 ms (−69,1 %)** jusqu'à tests verts ; **12/12 tests et digest
  SHA-256 du contenu final identiques** entre séquentiel, groupé et collecteur
  (`cddae95b87c1…`). Le gain vient de l'élimination des 5 recompilations chaudes
  (cold équivalent), pas du build initial.
- **Ré-exécution indépendante du Lead** (5 reps) : séquentiel **8 563 ms** vs groupé
  **2 659 ms**, gain **5 904 ms (−68,9 %)** ; collecteur w=0 2 861 ms, w=50
  2 627 ms, incompatible 9 317 ms, `valid=true` — chiffres cohérents avec la preuve
  du worker (variance de quelques % sur machine partagée).
- **Point d'équilibre réel** : 6 195 ms d'attente tolérable avant neutralité ; les
  fenêtres mesurées (0/50 ms) sont ~100× en dessous. Fenêtre 50 ms : +116 ms p50
  (~2 % du gain).
- **Limite structurelle documentée** : si l'adaptateur déclare des chemins de
  fichier communs, le pré-vol file-granulaire isole chaque proposition et le gain
  tombe à zéro ; l'adaptateur L2.4 déclare des symboles distincts et s'appuie sur le
  `cargo test` de l'analyseur. Un lot invalide coûte ≈ le séquentiel (repli sans
  gain, cohérent avec `p*`). Le collecteur n'est pas encore branché au produit.

### 20.2 Audit final d'intégration

[`MVP-0c-integration-checklist.md`](MVP-0c-integration-checklist.md) : verdict
**`READY_FOR_HUMAN_REVIEW`**, aucun bloquant technique. 202 fichiers (+71 322/−273),
0 suppression/renommage/binaire, 0 secret sur 71 305 lignes, 76 liens relatifs
valides, 33/33 preuves JSON valides, aucun test supprimé/désactivé, **0 conflit**
simulé entre les 3 PR, séquence #78 → #80 → #82 documentée avec arbres attendus.
Constats non bloquants : chemins personnels dans 14 preuves, protection `dev` sans
required status check, écart de comptage 491/492 **expliqué** (la CI exécute la
suite workspace de 491 tests **plus** un test filtré par une démo `migration v1→v2`,
`9 filtered out`), nom de preuve d'audit initial corrigé dans §17.

### 20.3 Règle E2E vérifiée sur la CI réelle

Runs `42550250227` (`51a1bae`) et `42551014585` (`24d92b1`) : étapes **Setup Node,
Browser E2E, Upload** toutes `success`, artefact `browser-e2e-evidence` (155 290 o),
rapport **10/10**, 44 vérifications. **`workflow_dispatch` réel** : run
[42551717472](https://github.com/Rwanbt/Ravum/actions/runs/42551717472),
`base=origin/dev`, succès, décision `browser=true`, E2E 21 s, artefact 10/10.
Décisions locales 4/4 conformes ; **coût observé +26 à 29 s** (bas de la fourchette
25–65 s), run dominé par la suite FULL (216–233 s). Les runs observés sont FULL
(diff vs `origin/dev`) : FULL + E2E prouvés, chemin sélectif UI non observé.

### 20.4 Blocages et dette confirmés

- **Kani / prouveur externe** : toujours bloqué — `wsl -l -v` ne répond pas (>120 s)
  sur cette machine, aucun prouveur externe exécuté ; documenté, aucun faux vert.
- **CI par impact sur `dev`** : toujours bloquée tant que `dev` ne contient pas les
  crates (décision humaine sur #78).
- Le collecteur `ravum-batch` est désormais **câblé au CLI** (`ravum batch-check`, 4 tests) ; l'intégration directe au daemon n'est pas faite (non requise par le gate L2) ; aucun `PROVED`.
- La duplication possible des E2E entre `mvp.yml` et `mvp-impact.yml` reste un coût
  accepté (~25–65 s), documenté, sans faux vert possible.
- La couverture features/targets/options de compilation du graphe inter-crates reste
  partielle (limite connue de `ravum-unit-graph`).

### 20.5 Dette technique corrigée (9 novembre)

- **Formatage** : le workspace a été formaté avec **rustfmt 1.95** (52 fichiers,
  +1 337/−409) ; la toolchain est épinglée par `rust-toolchain.toml` (`1.95.0`) et la
  CI installe `dtolnay/rust-toolchain@1.95.0` (au lieu de `stable`, qui exécutait
  rustc 1.99.1). Un step **`cargo fmt --all -- --check`** échoue désormais si le
  formatage dérive. Vérifications : `cargo fmt --all -- --check` exit 0 sur la
  toolchain épinglée, suite complète **495/0/78** après formatage et ajout des 4 tests `batch-check`.
- **Chemins personnels** : 63 preuves JSON ont été anonymisées (124 chemins
  utilisateur, 739 chemins de dépôt, 124 chemins temporaires) ; plus aucune
  occurrence du nom d'utilisateur dans les fichiers suivis. Manifeste :
  `evidence/mvp0c-evidence-sanitization-2026-11-09.json` ; aucun résultat, chiffre
  ou conclusion modifié (seuls les chemins d'environnement changent).
- **Collecteur câblé au CLI** : `ravum batch-check` regroupe les propositions via `ravum-batch`, applique les éditions sur une copie et vérifie chaque lot avec un vrai `cargo test --offline` (repli N+1). 4 tests d'intégration, démonstration `evidence/mvp0c-batch-cli-2026-11-09.json` (2 propositions, même fichier, granularité symboles → 1 lot, 1 analyse, 2 acceptées, `no_loss=true`). L'intégration directe au daemon n'est pas faite (non requise par le gate L2).
- **Simulation de fusion re-jouée** sur les bases actuelles (`dev` `045b285`, tête
  #78 `550847e`, tête #80 `45d8434`, tête #82 `e9f5aab`) : `git merge-tree` renvoie
  **0 conflit** pour les trois paires ; arbres simulés `975fecb9…` (dev+#78),
  `dbb0a69d…` (#78+#80), `5617d996…` (#80+#82). Ces arbres changeront encore avec
  les prochains commits : les rejouer au moment de la fusion avec les commandes de
  la checklist.

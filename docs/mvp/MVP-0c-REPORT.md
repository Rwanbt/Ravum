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
| Tests | 347/0 (54 suites) | **458/0 (69 suites)**, exit 0, local |
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

- **Réussis** : 458 tests, 69 suites, exit 0 (local, `--test-threads=1`,
  `CARGO_BUILD_JOBS=2`). Répartition : daemon 113 (dont 7 externs), coordinator 137
  (dont 15 crash + 3 transitoires), collab-oracle 14, unit-graph 19, dsp-harness 24
  (dont 9 stress), bench 5, cli 33, reste du workspace inchangé.
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

Preuves versionnées : `evidence/mvp0c-{pr-audit,cross-crate,cross-crate-daemon,grouping,grouping-multiagent,verification-transient,impact,impact-dev-dep,collab,collab-oracle,publication,e2e,verification-retry,dsp-stress,dsp-stress-ci,s8-pilot}-2026-11-09.json`.

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

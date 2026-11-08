# MVP-0b / L0 — Audit initial et baseline reproductible

> **Statut** : rapport d'audit, pas un certificat. Les chiffres sont datés, mesurés sur
> la machine de référence locale, et ne sont pas portables. Aucun statut de ticket, d'ADR
> ou de release n'est modifié par ce document.
>
> Date : 2026-11-08. Base de code auditée : `6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8`
> (HEAD de `feat/3-mvp0a-continuous-loop`, l'arbre de travail était propre).

## 1. Méthode

1. Lecture de `README.md`, `AGENTS.md`, `docs/architecture/STATUS.md`,
   `docs/architecture/PROJECT_INVARIANTS.md`, `docs/roadmap/MVP.md`,
   `docs/mvp/*.md` (M0 à M5), `docs/reviews/2026-11-08-consolidation.md` (extraits),
   `docs/rfc/RFC-SHARED-WORKSPACE-MULTIAGENT.md` (structure et sections de décision).
2. Vérification de l'état Git réel : branches locales et distantes, SHA, `git status`.
3. Vérification de la PR #78 et des issues ouvertes via `gh`.
4. Exécution de la suite complète et reproduction des démonstrations M1, COLLAB-1, M2,
   M3, M4.
5. Inventaire du code, des tests, des ADR, des preuves.
6. Consignation des défaillances observées **sans les corriger silencieusement**.

## 2. État des dépôts au démarrage

### 2.1 Dépôt de code — `Rwanbt/Ravum`

| Élément | Valeur observée |
|---|---|
| Checkout | `D:\App\Ravum` |
| Branche | `feat/3-mvp0a-continuous-loop`, arbre propre, HEAD `6b37ad3` |
| `origin/dev` et `origin/main` | `78411024e2665dafce31b029786ef56d400bdab2` (identiques au moment de l'audit) |
| Avance de la branche sur `dev` | **22 commits** |
| Branche de travail MVP-0b créée depuis ce HEAD | `feat/mvp0b-hardening` |
| Pull request | [#78](https://github.com/Rwanbt/Ravum/pull/78) → base `dev`, ouverte, `MERGEABLE`, 20 commits dans la PR |
| CI du dernier commit vérifié | verte (run `42438407246`), 16 étapes — non re-exécutée par cet audit |
| Issues ouvertes | 77 (protection de branches), 79 (RFC shared workspace), 34 (licence), plus la série RAV-026…RAV-054 |
| ADR | **42 fichiers**, tous `PROPOSED` |
| RFC | 2 (`README` + `RFC-SHARED-WORKSPACE-MULTIAGENT`, `PROPOSED`) |
| Licence | aucune déclarée (décision ouverte) |

Un commit d'outillage IA (`16b1ff2`, « install ai-native dev-stack ») est le premier de la
branche et se trouve donc dans la PR #78 bien qu'absent de `dev`/`main`. Aucune
réécriture d'historique n'est autorisée : la stratégie retenue en L5 sera de clarifier
les groupes de commits et de ne plus mêler outillage et produit dans les PR futures.

### 2.2 Vault Obsidian — `Rwanbt/obsidian-dev-brain`

| Élément | Valeur observée |
|---|---|
| Checkout | `D:\Documents\Obsidian\IA_Dev_Brain` |
| Branche | `master`, propre, HEAD `b2f54b2` (« chore(vault): auto-sync … ») |
| Contrat | `_system/VAULT-WRITING-CONTRACT.md` en `schema_version: 4`, auto-save autorisée avec gardes |
| Projet | `projects/ravum/` (`INDEX.md`, `AGENTS.md`, `BOARD.*` générés) |
| Dernière session | `projects/ravum/operations/sessions/2026-11-08-mvp0a-m0-m4-cycles.md` |
| `.git/maintenance.lock` | absent au moment de l'audit |

## 3. Machine, toolchain, conditions de mesure

| Élément | Valeur |
|---|---|
| OS | Windows 11 Professionnel 10.0.26100 |
| CPU | 13th Gen Intel Core i7-13620H, 10 cœurs / 16 threads |
| RAM | 15,71 Go visibles ; **2,2 Go libres au démarrage** (charge externe : autres agents, services) |
| Disque | `D:` 198 Go libres ; `C:` 6,1 Go libres |
| `rustc` / `cargo` | 1.95.0 (59807616e 2026-04-14) / 1.95.0 (2d3ce0bd6 2026-03-21) |
| Node | v26.13.0 |
| Git | 2.51.1.windows.1 |
| Outils disponibles | `cargo-nextest`, `sccache` (non utilisés dans cette campagne), `rust-analyzer` **shim présent mais composant non installé** |
| Charge CPU au démarrage | 78 % (`Win32_Processor.LoadPercentage`), d'autres agents écrivaient dans le dépôt |
| Configuration de build | `[profile.dev] debug = 1`, `incremental = false` |

Conséquence méthodologique : toutes les mesures de latence de ce cycle doivent être lues
avec la calibration de charge, et une comparaison entre deux campagnes exige des
conditions documentées. Les chiffres historiques M1 (189/519/746 ms) ont été obtenus avec
un cache d'artefacts désactivé ; ils ne sont pas comparables à des runs à cache chaud.

## 4. Baseline de tests

Commande exécutée depuis la racine :

```powershell
cargo test --workspace --all-targets
```

| Résultat | Valeur |
|---|---|
| Tests réussis | **139** |
| Échecs | **0** |
| Durée observée | **46,52 s** (`EXIT=0`) |
| Suites exécutées | 24 binaires de test (`ravum`, `m1_patch_loop`, `collab_flow`, `m2_proof_gate`, `block_host`, `proof_cache`, `artifact_cache`, `e2e`, `git_writer`, `journal_durability`, …) |

Sortie brute conservée : `evidence/mvp0b-l0-baseline.txt`.

Ce résultat **reproduit** le chiffre historique rapporté (139/139). Il ne prouve pas la
correction du futur compilateur Ravum, ni une absence universelle de corruption mémoire.

## 5. Reproduction des démonstrations déclarées

| Démonstration | Commande | Résultat observé | Verdict |
|---|---|---|---|
| M1 hot patch | `scripts/demo-m1.ps1` | processus hôte vivant, `activated`, check 378 ms / build 72 ms / load 6 ms / total **462 ms** | reproduit |
| COLLAB-1 | `scripts/demo-collab.ps1` | 6 propositions, `AUTO_SAFE=2 CONFLICT=1 REJECT=1 MANUAL_REQUIRED=1 REBASE_REVALIDATE=1`, journal écrit, base Git temporaire | reproduit |
| M2 preuve bornée | `ravum prove positive.rs --claim finite --inputs 0,1,2,3` | `status: proved`, `authorizes_activation: true`, domaine = 4 entrées déclarées | reproduit |
| M2 contre-exemple | `ravum prove negative.rs --claim non-negative --inputs 0,1,2,3` | `status: tested`, `counterexample: f(2) = -1 < 0`, **exit 1 attendu** | reproduit |
| M3 DSP | `ravum dsp iir_v1.rs --next iir_v2.rs --blocks 400 --json` | 0 allocation, 0 dépassement, `max_sample_delta` 0,1079 après swap | reproduit (voir `evidence/mvp0b-dsp-iir.json`) |
| M4 full-stack | `scripts/demo-m4.ps1` | health/contrat, CRUD complet, 401 sans jeton, 400 `title_empty`, migration v1→v2 vérifiée | reproduit |

### Défaillance observée pendant l'audit

Le premier `cargo run -q --bin ravum -- prove …` a échoué avec
`exit code 0xc0000142 (STATUS_DLL_INIT_FAILED)` pendant que d'autres agents compilaient et
liaient le même binaire. Le même appel, immédiatement relancé, a réussi. Ce n'est pas un
échec de test masqué : c'est une instabilité de l'environnement de build concurrent
(linker qui remplace l'exécutable pendant son chargement) et elle est corrigée par la
sérialisation des campagnes. Elle est conservée ici pour la traçabilité.

## 6. Inventaire réel

| Élément | Valeur |
|---|---|
| Crate workspace (base) | 11 crates + 2 applications (`ravum-demo-host`, `ravum-todo`) |
| Code Rust | ≈ 17 400 lignes dans `crates/` et `apps/` |
| Documentation | 76 fichiers Markdown dans `docs/` |
| ADR | 42 (tous `PROPOSED`) |
| Preuves versionnées | 12 fichiers dans `evidence/` (M1, M2, M3, M4) |
| Fixtures | `calculator/`, `collab/`, `dsp/`, `latency/`, `negative/`, `proof/` |

## 7. Réel contre documenté

| Domaine | Implémenté et testé | Seulement documenté / absent |
|---|---|---|
| Boucle snapshot → analyse → build → patch → activation | oui (M1, tests + démo) | migration d'état, ABI plus large que `f64 -> f64` |
| Publication sans verrou | oui (`ArcSlot`), mais vulnérable au dernier `Arc` sur l'audio (**risque de l'audit**) | — |
| Écrivain Git unique | oui (COLLAB-1, `commit --only` + contrôle a posteriori) | **transactionnalité réelle contre un écrivain concurrent : absente (risque de l'audit)** |
| Preuves bornées | oui (domaine fini énuméré, ledger append-only, cache invalidé) | prouveur externe (Kani) non installé → `Unsupported` |
| DSP de bloc | oui (ABI v2, état possédé par l'hôte, 0 alloc mesurée) | crossfade, périphérique audio, priorité RT, WCET |
| Full-stack | oui (UI statique + API + SQLite + migrations + 10 E2E socket/base) | **aucun E2E navigateur**, aucun TLS, une connexion SQLite |
| CLI | oui (`check`, `build`, `patch`, `run`, `watch`, `status`, `bench`, `prove`, `artifacts`, `dsp`) | `bench` mesure la boucle, pas le temps jusqu'au livrable |
| Daemon / incrémentalité persistante | **absent** : `rustc` est relancé à froid à chaque édition | cible P0-A non atteinte |
| CI sélective par impact | **absente** : un seul workflow produit non sélectif | L4-E |
| Shared workspace multi-agents | prototype COLLAB-1 + RFC `PROPOSED` | COLLAB-2/3/4 absents, benchmark S8 non exécuté |
| Vérification/sécurité FFI | `unsafe` localisés, revue partielle | outillage Miri/ASan non exécuté |

## 8. Risques identifiés et ordre de traitement

| # | Risque | Sévérité | Traitement dans MVP-0b |
|---|---|---|---|
| R1 | Destruction du dernier `Arc` d'un module retiré sur le thread audio (allocation + envoi canal dans un `Drop`) | **critique** | **corrigé en L1-A** (`docs/mvp/MVP-0b-rt-lifecycle.md`) |
| R2 | Publication Git non transactionnelle : `commit --only --no-verify`, concurrence détectée *après* publication, hooks contournés | **critique** | L1-B |
| R3 | `rustc` à froid à chaque édition → P0-A et P0-I non tenues | haute | L2 |
| R4 | Résultats périmés / buffers incomplets non couverts par un moteur persistant | haute | L2 |
| R5 | Aucune mesure de productivité S8 worktrees vs workspace partagé | haute | L3 |
| R6 | E2E navigateur absent, JS de l'UI non validé | haute | L4-A |
| R7 | Discontinuités DSP mesurées (0,108 / 0,657) non traitées | moyenne | L4-B |
| R8 | CI non sélective, risque de lenteur et de couverture inégale | moyenne | L4-E |
| R9 | Licence non choisie, protection de branches non configurée | moyenne | L5 (décision humaine) |
| R10 | Machine chargée, RAM libre faible : risque d'OOM/contention sur les campagnes parallèles | moyen | limitation des workers, mesure de charge |

## 9. Dépendances et dette

- Dépendances externes ajoutées par MVP-0a : `serde`, `serde_json`, `sha2`,
  `libloading`, `rusqlite` (SQLite embarqué, isolé derrière `ravum-store`).
- Aucune API `rustc_private`, aucun `RUSTC_BOOTSTRAP` : vérifié dans
  `ravum-rust-adapter` (variables ambiantes neutralisées).
- Dette assumée et documentée : `ArcSlot::load_owning` reste exposé pour les threads de
  contrôle ; `--no-verify` sera retiré en L1-B ; `[profile.dev] incremental = false` a été
  posé pour la mesure de latence et devra être réévalué avec le daemon L2.

## 10. Verdict du gate L0

- Baseline **reproducible** : 139/139 tests, 46,52 s, commande et environnement documentés.
- Démonstrations M1, COLLAB-1, M2, M3, M4 **reproduites**.
- Limitations, risques et écarts réel/documenté **identifiés** ci-dessus.
- Aucun test existant n'a été supprimé, désactivé ou contourné.

**Gate L0 : franchi.** Les deux correctifs critiques R1 et R2 sont les priorités du lot L1 ;
R1 est traité en L1-A, R2 en L1-B. La cible P0-A reste **non atteinte** et ne pourra pas
être revendiquée sans le lot L2.

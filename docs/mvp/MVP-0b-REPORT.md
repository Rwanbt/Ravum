# MVP-0b — Rapport de cycle (durcissement, incrémentalité, collaboration, sauvegardes)

> **Rapport de cycle, pas un certificat.** Chaque chiffre vient d'une exécution réelle
> consignée dans `evidence/` ou dans les rapports de lot. Les revendications sont limitées
> au périmètre démontré. Aucun ADR n'est accepté : les 42 ADR et la RFC shared workspace
> restent `PROPOSED`. Aucune release, aucune fusion automatique, aucune garantie hard-RT,
> aucune preuve générale de sûreté.
>
> Date : 8 novembre 2026. Langue : français. Branche : `feat/mvp0b-hardening`, empilée sur
> la PR #78 (MVP-0a).

## 1. État initial exact des deux dépôts

| Dépôt | État au démarrage (vérifié) |
|---|---|
| **Rwanum code** (`D:\App\Ravum`) | branche `feat/3-mvp0a-continuous-loop`, HEAD `6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8`, arbre **propre** ; `origin/dev` = `origin/main` = `78411024e2665dafce31b029786ef56d400bdab2` ; avance de 22 commits sur `dev` |
| PR | [#78](https://github.com/Rwanbt/Ravum/pull/78) ouverte, base `dev`, `MERGEABLE`, non fusionnée, 20 commits |
| Issues | #77 (protection de branches), #79 (RFC shared workspace), #34 (licence), série RAV-026…RAV-054 ; toutes ouvertes |
| ADR / RFC | 42 ADR `PROPOSED`, 2 RFC `PROPOSED` |
| Licence | aucune déclarée |
| **Vault** (`D:\Documents\Obsidian\IA_Dev_Brain`) | `master`, HEAD `b2f54b2`, propre hors `LOG.md` préexistant, contrat v4 lu et vérifié, pas de `.git/maintenance.lock` |
| Machine | Windows 11 Pro 26100, i7-13620H 10c/16t, 15,71 Go RAM, 2,2 Go libres au départ, charge CPU 78 %, `rustc`/`cargo` 1.95.0, Node 26.13.0, Git 2.51.1 |
| Tests | **139 réussis, 0 échec, 46,52 s** (reproduit exactement le chiffre historique) |

## 2. État final et SHA

| Élément | Valeur |
|---|---|
| Branche de travail | `feat/mvp0b-hardening`, poussée, **15 commits** au-dessus de `6b37ad3` |
| Dernier commit de code | `6d5b1b7` (ce rapport est ajouté par le commit suivant de la même branche) |
| PR MVP-0b | **[#80](https://github.com/Rwanbt/Ravum/pull/80)**, base `feat/3-mvp0a-continuous-loop`, `MERGEABLE`, 15 commits, 208 fichiers, +60 333/−282 |
| PR #78 | inchangée, non fusionnée, non réécrite |
| CI du dernier commit | [run 42487917654](https://github.com/Rwanbt/Ravum/actions/runs/42487917654) — **succès**, 18 étapes dont l'E2E navigateur |
| Tests workspace | **275 réussis, 0 échec**, 74 binaires de test, 109,3 s |
| ADR / RFC / licence | inchangés : 42 `PROPOSED`, RFC `PROPOSED`, aucune licence publiée |
| Vault | 2 sessions publiées et vérifiées (voir §25) |

## 3. Subagents réellement utilisés

Six teammates durables, chacun avec un périmètre d'écriture exclusif et une mission
vérifiable ; le Lead a contrôlé et intégré chaque livraison (relecture + ré-exécution des
tests), plutôt que de se fier aux rapports.

| Agent | Mission | Livrable principal | Vérification Lead |
|---|---|---|---|
| `git-txn` (Worker C) | L1-B Git transactionnel | `git_writer.rs` réécrit, `path_policy.rs`, `publication_journal.rs`, 16 tests | 47 tests re-exécutés, revue du CAS et des hooks |
| `incremental` (Worker D) | L2 daemon persistant | crate `ravum-daemon`, 3 campagnes de mesures, docs | 50 tests, ré-exécution de la suite invariants 14/14 |
| `browser-e2e` (Worker F) | L4-A E2E navigateur | harnais CDP 10 scénarios, `demo-m4-browser.ps1`, étape CI, 3 bugs produit corrigés | 10/10 re-exécuté par le Lead, **vert en CI** |
| `collab-2-3` (Worker G) | L3 COLLAB-2/3 | verifier injecté, `semantics.rs`, `records.rs`, 4 corpus de tests, démo COLLAB-2 | 87 tests re-exécutés, démo re-exécutée |
| `s8-bench` (Worker E) | S8 benchmark | harnais `benchmarks/s8`, 15 campagnes, preuves JSON | protocole `ok`, verdict relu |
| `impact-ci` (Worker I) | L4-E CI sélective | `tools/impact`, workflow `mvp-impact.yml`, selftest 48 cas | selftest re-exécuté, correction du FULL légitime |
| `collab-4` (Worker J) | L3.4 collaboration ↔ daemon | `ravum collab-cycle`, graphe d'unités fail-closed, 36 tests CLI, preuve JSON | 36 tests re-exécutés, démo re-jouée |
| `audio-probe` (Worker K) | L4-C audio réel | sonde `waveOut` silencieuse + mesures device | à intégrer (round 2) |

Le Lead a réalisé lui-même : L0 (audit/baseline), L1-A (cycle de vie temps réel DSP),
L4-B (crossfade), L4-C/L4-D (statuts honnêtes), L5.3 (licence), l'intégration CLI du
daemon, la consolidation documentaire, la protection de branches, les PR et les deux
sauvegardes Obsidian.

## 4. Tickets et exigences traités

| Ticket/exigence | Traitement | Statut |
|---|---|---|
| #77 protection `main`/`dev` | protection appliquée par API, commentaire de vérification publié | **fait** (décision humaine restante : durcir `dev` avec checks) |
| #34 licence | analyse + recommandation + outil d'audit, **aucune licence publiée** | **préparé**, décision humaine |
| #79 RFC shared workspace | contexte nourri par COLLAB-2/3 et S8 ; RFC **non ratifiée** | **PROPOSED inchangé** |
| P0-A (fluidité) | daemon persistant + `ravum check --daemon` mesurés | **partiel** : contenu identique <1 ms, code modifié 165–208 ms (cible non atteinte) |
| P0-C (sécurité mémoire) | risque de destruction sur callback supprimé, garde non possédante testée | **corrigé et mesuré** |
| P0-D (vérification honnête) | statuts préservés, prouveur absent signalé, test instable corrigé | **maintenu** |
| P0-F (temps réel) | crossfade + 0 alloc/free callback + statut audio honnête | **partiel** : pas d'hôte réel, aucune qualification RT |
| P0-I (temps jusqu'au livrable) | S8 mesuré | **mesuré, non démontré supérieur** (workspace partagé plus lent) |

## 5. Commits (15, sur `feat/mvp0b-hardening`)

1. `f01d7b7` — fix(dsp): never destroy a retired module on the audio callback
2. `86f90b4` — docs(mvp0b): audit the real state and record the reproducible L0 baseline
3. `44573a4` — feat(dsp): crossfade block swaps without adding work to the callback
4. `245db0b` — feat(collab): make Git publication transactional and hook-respecting
5. `7026f21` — feat(daemon): persistent incremental analysis engine with honest measurements
6. `41260bd` — fix(proof): isolate test ledgers; record verification and audio-host status
7. `a094c2d` — docs(licence): third-party inventory, options and implicit-decision audit
8. `25d9856` — feat(cli): route `ravum check --daemon` through the persistent engine
9. `c2c2ce9` — feat(m4): real browser E2E via CDP, and fix the two UI bugs it found
10. `da3e6ad` — bench(s8): worktrees vs shared workspace vs sequential, negative result
11. `f58c32b` — ci(impact): conservative fail-closed impact selection
12. `50d1273` — fix(adapter): parse the direct rustc JSON diagnostics
13. `902b912` — fix(daemon): isolate cache test directories for parallel tests
14. `167281b` — feat(collab): COLLAB-2/3 with symbol-aware merges and real verification
15. `6d5b1b7` — docs(mvp0b): consolidate the verified state, status and roadmap

## 6. PR créées ou mises à jour

- **PR #80 créée** : « feat(mvp0b): temps réel DSP, Git transactionnel, daemon incrémental
  et collaboration vérifiée », base `feat/3-mvp0a-continuous-loop` (empilée), `MERGEABLE`.
  Elle sera retargetée vers `dev` après fusion de #78, **décision du responsable**.
- **PR #78 inchangée** : aucun commit réécrit, aucun force push, aucun commentaire supprimé.
- Une seule branche poussée (`feat/mvp0b-hardening`), fast-forward à chaque push.
- Commentaires publiés : #77 (configuration de protection vérifiée), #34 et #79 (liens vers
  analyses/livrables, voir §25).

## 7. Correctifs DSP (L1-A + L4-B)

### 7.1 Cycle de vie temps réel (risque critique R1)

Défaut confirmé par lecture du code puis par un test **déterministe** : `process_rt`
prenait un `Arc` possédant ; si l'écrivain relâchait ses références entre le chargement et
le relâchement du callback, le `Drop` du module s'exécutait sur le thread audio
(libération mémoire + envoi de bibliothèque dans le canal de reclamation = allocation et
verrou dans un `Drop`).

Correction : garde de lecture non possédante (`ReadGuard`, deux atomiques, aucun
changement du compteur fort, `!Send`), `process_rt`/`enter_rt` l'utilisent, l'API
possédante est renommée `load_owning` et documentée « contrôle uniquement » ; le thread de
reclamation est créé avant tout `Drop` possible. Harnais étendu aux **désallocations**.

Mesure : **199 517 blocs** de callback pendant 12 publications → **0 allocation,
0 désallocation** sur le thread audio ; 4 lecteurs + 1 écrivain ; bloc long ; file de
200 bibliothèques ; tempête de 40 publications (141,5 ms) sans fuite. 5 tests adversariaux
+ 3 tests unitaires de slot.

### 7.2 Crossfade de remplacement (L4-B)

Fondu de sortie borné, opt-in (`--crossfade-frames`), état du module sortant conservé
**par thread** (un premier essai qui réécrivait cet état à chaque bloc **aggravait** le
clic IIR : 0,112), identité de fondu par epoch process-wide, buffer de pile, aucun état
partagé mutable.

| Session (400 blocs × 256 frames, 48 kHz) | Sans fondu | Avec fondu 256 | Référence |
|---|---:|---:|---:|
| IIR coefficient 0,1 → 0,5 | 0,1079 | **0,0294** | 0,0288 |
| Gain 0,5 → 2,0 | 0,6572 | **0,0576** | 0,0288 |

0 allocation et 0 désallocation sur le thread audio pendant les fondus. Le gain reste à
2× la référence : plancher d'une rampe atteignant un gain de 2,0, pas un clic résiduel.

## 8. Correctifs Git transactionnels (L1-B)

Remplacement de `git commit --only --no-verify` + contrôle du parent *après* coup par :

1. `prepare()` : index isolé (`GIT_INDEX_FILE`) semé depuis HEAD, blobs par
   `hash-object -w`, entrées `update-index --cacheinfo/--force-remove`, `write-tree`,
   hooks exécutés explicitement (`prepare-commit-msg`, `pre-commit`, `commit-msg` avec
   relecture du message, `reference-transaction` honoré nativement), `commit-tree
   -p expected_old`. Aucune référence, aucun index réel, aucun working tree touché.
2. `publish()` : re-validation HEAD + disque **immédiatement avant** le point de commit,
   puis un seul `git update-ref <ref> <new> <old>` (CAS atomique). Refus explicite si la
   référence a bougé, ref intacte. Réconciliation ensuite des seuls chemins publiés ; un
   fichier divergent est laissé intact avec avertissement, jamais écrasé.
3. Journal durable append-only + fsync (`INTENT` avant swap, `OUTCOME` après), idempotence
   par `publication_id`, `recover()` pour les intentions interrompues.
4. Politique de chemins (`.git`, absolu, `..`, `:` refusés ; générés/lockfiles protégés).
5. `--no-verify` supprimé ; hooks réellement exécutés (fallback `sh.exe` Git mesuré sous
   Windows).

16 tests négatifs couvrent les 12 scénarios du mandat. Incident CI réel trouvé et corrigé
à la racine : la démo COLLAB-1 partageait un journal de publication persistant dans
`target/` (cache CI) et refusait un re-run avec `AlreadyPublished` → journal scopé dans
l'espace de travail jetable de la démo, l'idempotence du writer étant conservée (3 runs
consécutifs verts).

## 9. Architecture incrémentale obtenue (L2)

Crate `crates/ravum-daemon` (lib + serveur JSON-lines + harnais de mesure), uniquement sur
la CLI publique `rustc` (aucune API `rustc_private`) :

- snapshots immuables adressés par contenu (source + contexte + toolchain), buffers non
  sauvegardés supportés, fichiers utilisateur jamais modifiés ;
- unité de compilation stable par contexte pour réutiliser l'état `-C incremental` ;
- cache de verdicts L1 mémoire borné + L2 disque auto-digéré (entrée altérée = `miss`,
  jamais une mauvaise réponse) ;
- révisions monotones : une réponse périmée est rejetée **avant** tout travail ; une
  révision plus récente **tue réellement** le `rustc` en vol (35 ms observés) ;
- admission bornée et priorisée (interactive d'abord), budgets et timeout durs ;
- rapport JSON structuré : diagnostics, timings par étape, hit/miss.
- intégration : `ravum check <fichier> --daemon` (1<sup>er</sup> run 182 ms miss,
  2<sup>e</sup> run **0 ms** hit disque, buffer cassé → 2 erreurs + exit 1).

Mesures honnêtes (10 échantillons/cellule, ordre tournant, diagnostics identiques à froid
obligatoires) : contenu identique 139–153 ms → **0 ms** ; modification réelle d'une petite
unité : **aucun gain**, overhead 5–8 ms par miss ; `-C incremental` rentable à partir de
~1 000 lignes (−12/−24 %) et **−30/−36 % au-delà de ~3 200 lignes** ; 14/14 contrôles
négatifs et de bord passent (invalidation inter-modules, péremption, annulation,
corruption, redémarrage, budgets, timeout, JSON, fichiers utilisateur intacts).

## 10. Fonctionnalités collaboratives obtenues (L3)

- **COLLAB-1 conservé** : décisions historiques identiques
  (`AUTO_SAFE=2 CONFLICT=1 REJECT=1 MANUAL_REQUIRED=1 REBASE_REVALIDATE=1`), test de
  non-régression dédié, vérification sémantique **opt-in**.
- **COLLAB-2** : deux agents modifient deux fonctions différentes du même fichier → deux
  `AUTO_SAFE`, fusion, **compilation réelle** du snapshot fusionné (et exécution des tests
  avec `--verify-tests`), puis publication Git conservant les deux changements.
- **COLLAB-3** : 10 cas sémantiques testés avec verdicts explicites : même fonction →
  `CONFLICT` ; signature publique → `MANUAL_REQUIRED` ; appelant incompatible → `REJECT`
  avec `E0061` ; contrat affaibli → `MANUAL_REQUIRED` ; dépendance non déclarée →
  `MANUAL_REQUIRED` ; import ambigu → `REJECT` avec `E0659` ; ABI → `MANUAL_REQUIRED` ;
  lockfile → `MANUAL_REQUIRED` ; régression fonctionnelle compilable →
  `MANUAL_REQUIRED` sans tests, `REJECT` avec `--verify-tests` ; snapshot périmé →
  `REBASE_REVALIDATE`. Chaque refus laisse snapshot et arbre Git inchangés.
- Corpus de 10 scénarios comparant 4 stratégies de fusion : la couche retenue
  (symboles + lignes réellement modifiées + graphe de dépendances + compilation) est la
  seule qui refuse le cas « même fonction, lignes disjointes » sans compiler, avec zéro
  fusion incorrecte ; 2 faux conflits assumés et documentés.
- API de passage L2/COLLAB-4 : `AcceptedChange` (snapshots avant/après, chemins, symboles,
  décision, séquence de journal, record de vérification avec identifiants de preuve
  déterministes) ; trailers `Ravum-Evidence`/`Ravum-Check-Evidence`.

## 11. Résultats du benchmark S8

Protocole pré-enregistré, 6 éditions sur 3 agents scriptés déterministes (pas des LLM),
5 répétitions par scénario, ordre alterné, caches froids identiques, 15 campagnes,
validation `ok: true`.

| Métrique (médiane, n=5) | A — worktrees | B — workspace partagé | C — séquentiel |
|---|---:|---:|---:|
| Temps total jusqu'à tests verts | **5 660,6 ms** | **6 677,6 ms** | **4 785,6 ms** |
| Attente cumulée des agents | 2 053,5 ms | **7 750,8 ms** | 0,14 ms |
| Conflits / temps de résolution | 1 / 433,6 ms | 1 / 0 ms | 0 / 0 |
| Merges / publications / commits | 3 | 1 | 6 |
| Changements acceptés perdus | 0 | 0 | **1 (écrasement silencieux, 5/5)** |
| Réussite fonctionnelle stricte | 5/5 | 5/5 | **0/5** |

**Verdict honnête** : le workspace partagé **n'est pas plus rapide** ; c'est le plus lent
des trois et il triple l'attente des agents, car snapshot + arbitrage + publication
transactionnelle coûtent plus que trois merges, tandis qu'une compilation froide domine
(2,4–3,5 s). Ce qu'il achète pour ce surcoût est mesuré : refus explicite sans écriture,
journal durable, publication unique attribuée, **zéro perte**. Le séquentiel est le plus
rapide mais perd silencieusement une édition acceptée. La suppression des worktrees n'est
donc **pas** justifiée par ces mesures ; elle devra l'être par une campagne avec de vrais
agents IA (tokens et supervision **non mesurés ici**).

## 12. Tests réussis

| Commande | Résultat |
|---|---|
| `cargo test --workspace --all-targets` | **275 réussis / 0 échec**, 74 binaires, 109,3 s |
| Baseline L0 avant modifications | 139 réussis / 0 échec, 46,52 s |
| `ravum-patch-host` | 26 (8 unit, 8 block_host, 5 rt_lifecycle, 5 crossfade) |
| `ravum-workspace-coordinator` | 87 (49 unit, 2 collab1, 3 collab2, 10 collab3, 2 stratégies, 16 git_publication, 2 git_writer, 3 journal) |
| `ravum-daemon` | 50 (32 unit, 3 chaînes d'édition, 15 intégration) |
| `ravum-cli` | 36 (15 unitaires du binaire dont 3 `cycle-bench` + 10 `collab-cycle`, 4 intégration COLLAB-4, 17 intégrations existantes préservées) |
| `ravum-proof` | 28 (19 unit, 9 proof_cache) |
| `ravum-rust-adapter` | 8 (dont le nouveau cas rustc-direct) |
| `ravum-todo` | 14 (4 unit + 10 E2E HTTP) |
| E2E navigateur | **10/10** localement (Chromium headless 156) **et en CI windows-latest (Edge)** |
| CI complète | [run 42487917654](https://github.com/Rwanbt/Ravum/actions/runs/42487917654) **succès** |

## 13. Tests échoués (et ce qu'il en est advenu)

Aucun test ne reste en échec. Trois défaillances réelles ont été trouvées et corrigées à
la racine, **pas masquées** :

1. **`ravum-proof`, test instable préexistant** : `timeout_never_authorizes_activation`
   échouait par intermittence car tous les tests partageaient un ledger temporaire
   (chemin keyé sur un compteur valant 0 partout) et donc la même clé de preuve. Corrigé
   par une séquence unique par test ; 6 exécutions consécutives 19/19.
2. **`ravum-daemon`, test instable** : les tests de cache partageaient un répertoire
   temporaire à la seconde près, donc se corrompaient en parallèle. Corrigé ; 6 runs
   parallèles 32/32.
3. **CI rouge (run 42485209061)** : la démo COLLAB-1 rejouée sur un `target/` caché
   trouvait un journal de publication persistant et refusait avec `AlreadyPublished` —
   l'idempotence du writer était correcte, la démo ne l'était pas. Journal scopé dans
   l'espace jetable ; 3 runs consécutifs verts, puis CI verte.

Défaillances transitoires d'environnement, documentées sans les compter comme régressions :
`STATUS_DLL_INIT_FAILED` lors d'un link concurrent, un `link.exe exit 1104` (fichier en
cours d'utilisation) résolu au retry, et 2 échecs de mesure dus à des fichiers non suivis
d'un autre worker (WIP), jamais présents dans un commit.

## 14. Tests non exécutés

- **Hôte audio réel** (CLAP/VST3/WASAPI/ASIO), XRUN, buffer size, priorité RT — non
  exécuté, raisons et plan dans `MVP-0b-audio-host.md`.
- **Prouveur externe** (Kani) : non installé, aucun harness, donc `Unsupported` — plan
  proposé dans `MVP-0b-verification.md`.
- **Soak test RSS / fuite longue** du daemon ; **deux daemons partageant un même
  `work_dir`** ; crash pendant écriture de cache.
- **S8 avec de vrais agents LLM** : agents scriptés seulement, tokens et supervision non
  mesurés.
- **S8 sur un vrai projet multi-crates** ; **fixtures macro-lourdes**.
- **Firefox/WebKit**, mobile, a11y complète, TLS/HTTPS pour l'E2E navigateur.
- **MSRV 1.74** déclarée dans les manifestes : compilée/testée avec `rustc 1.95` seulement.
- **Workflow `mvp-impact.yml`** : validé localement (selftest, PyYAML, simulation) mais
  **pas encore exécuté sur GitHub Actions** (le déclencheur est `pull_request` vers
  `main`/`dev` ; il s'exécutera à la première PR vers `dev`).
- **Benchmark de bout en bout** : désormais **mesuré** (`ravum cycle-bench`, voir
  `MVP-0b-cycle.md`) pour une petite unité et une unité synthétique de 2 313 lignes ;
  restent non mesurés : un vrai crate multi-crates, les macros procédurales, le coût du
  seul link incrémental et la preview dans une interface réelle.

## 15. Mesures M0/M1/MVP-0b comparées

| Mesure | MVP-0a (M1) | MVP-0b (ce cycle) | Lecture |
|---|---|---|---|
| Analyse `rustc` à froid | p50 145 ms, p95 189 ms | 144–201 ms p50 (fixture 230 lignes) | même plancher de démarrage |
| Analyse contenu identique | non couverte (cache d'artefacts de build) | **0 ms p50/p95** (cache de verdicts) | gain majeur, cible <50 ms atteinte **pour ce cas** |
| Analyse après modification réelle | ~145–190 ms | 165–208 ms p50 | **cible <50 ms non atteinte** |
| `-C incremental` persistant | inexistant | −12/−24 % à ~1 100 lignes ; **−30/−36 % à 3 200–7 629 lignes** | gain réel mais dépendant de l'échelle |
| Patch total (analyse+build+link+verify) | p50 525 ms, p95 746 ms (fixture calculator) | **mesuré** : petite unité 316–339 ms p50 / 335–362 ms p95 ; unité 2 313 lignes 524–600 ms p50 / 702–743 ms p95 | cible p95 < 500 ms atteinte sur petite unité, manquée à 2 313 lignes ; build+link à froid dominant |
| Clic de remplacement DSP (IIR / gain) | 0,108 / 0,657 | **0,029 / 0,058** avec fondu | mesure ; pas de seuil psychoacoustique revendiqué |
| Allocations callback | 0 mesurées | **0 allocations et 0 désallocations** (199 517 blocs) | garde temps réel |
| RAM daemon (WorkingSet) | — | 5,6–7,4 Mo pendant la campagne | sobriété du moteur |
| Temps total jusqu'à tests verts, 3 agents | — | A 5 661 ms / B 6 678 ms / C 4 786 ms | S8, voir §11 |
| Jigue OS (période 5,33 ms) | non mesurée | lateness p50 0,37 ms / p95 0,65 ms / max **2,6 ms** | borne les prétentions RT |
| Cycle complet, contenu **inchangé** (édition→activation) | 85–177 ms (p50) | **8–13 ms** (p50), p95 9–18 ms | ×10 à ×13, cache de verdicts + cache d'artefacts |
| Cycle complet, **édition réelle**, petite unité | 316 ms p50 / 335 ms p95 | 339 ms p50 / 362 ms p95 | MVP-0b ≈ +7 %, build à froid dominant |
| Cycle complet, **édition réelle**, unité 2 313 lignes | 524 ms p50 / 702 ms p95 | 600 ms p50 / 743 ms p95 | cible p95 < 500 ms **manquée** à cette taille |

## 16. CPU et RAM

- Machine de référence : 15,71 Go ; RAM libre observée entre 2,2 et 4,8 Go selon les
  agents concurrents ; charge CPU mesurée entre 21 % et 93 % selon les campagnes.
- Campagne L2 : CPU 30–44 %, 3,4–3,7 Go libres, 329–342 processus, calibration
  `rustc -vV` 65–94 ms ; échelle : 26–42 % ; invariants : 73–80 % (chaque fichier de
  preuve embarque sa charge).
- Daemon : WorkingSet 5,6–7,4 Mo, `peak_active_jobs = 1` (parallélisme imposé à 1),
  répertoire incrémental 3,8–24,6 Mo selon la taille d'unité (croissance disque **non
  collectée**, documentée).
- S8 : RAM pic coordinateur 5,5 Mo ; testeurs 93–142 Mo ; CPU 1,8–2,6 s par campagne ;
  disque ~12,6 Mo par réplique.
- Sonde de jigue : lateness max 2,6 ms sous 52 % de charge — aucune extrapolation hard-RT.

## 17. Résultats E2E navigateur

Harnais CDP sans dépendance npm, vrai Chromium headless, vrai serveur, vraie base SQLite,
**validation du DOM affiché** (pas seulement du HTTP).

- Local : **10/10 scénarios PASS**, 44 vérifications, 9,4–13,1 s, HeadlessChrome/156.
- CI GitHub windows-latest : **étape verte** (Edge détecté, `-RequireBrowser`).
- Défauts produit réels trouvés **par le navigateur** et corrigés :
  1. la suppression depuis l'UI était **impossible** (DELETE sans `Authorization` → 401) ;
  2. serveur Windows : socket accepté en mode non bloquant → `400 malformed_request`
     intermittents avec un vrai client (~1/8) ;
  3. réponse `204` avec `Content-Length` (interdit RFC 7230 §3.3.2) provoquant
     `net::ERR_ABORTED` côté Chromium.
- Ajouts UI nécessaires : édition inline du titre, message DOM pour titre vide, favicon
  `data:`. Aucun test HTTP supprimé ; les 10 tests HTTP existants passent toujours.

## 18. Résultats DSP

Voir §7. Résumé : garde non possédante (0 destruction sur callback), crossfade
0,108→0,029 (IIR) et 0,657→0,058 (gain), 0 allocation/désallocation sur 199 517 blocs et
400 blocs de session, 0 dépassement de période simulée. **Aucun périphérique audio, aucune
priorité temps réel, aucun XRUN, aucune qualification hard-RT.**

## 19. Résultats de vérification

- Vérification bornée M2 conservée : `prove` positif → `proved` sur domaine fini déclaré,
  négatif → `tested` + contre-exemple `f(2) = -1 < 0`, exit 1 attendu, activation refusée.
- 28 tests de preuve verts ; statuts jamais promus ; cache invalidé par mutation.
- **Prouveur externe absent** (vérifié : `kani`/`cargo kani` introuvables) → `Unsupported`,
  aucun certificat, aucune revendication de preuve externe.
- `ravum check` remonte à nouveau les diagnostics structurés (bug de parsing `rustc`
  direct corrigé) : `error[E0308] ...:1:28: mismatched types`.

## 20. Régressions éventuelles

- **Aucune régression de test** : 275/275 verts, CI verte, demos M1/COLLAB-1/M2/M3/M4
  inchangées et exécutées en CI.
- Changements de comportement **assumés et documentés** :
  `BlockHost::process_rt` utilise une garde (payload publié `BlockProgram`) — API interne
  des tests adaptée ; les diagnostics `rustc` sont désormais structurés au lieu d'un
  fallback unique (amélioration, couverte par test) ; la démo COLLAB-1 scope son journal
  de publication (correctif) ; la vérification sémantique COLLAB est **opt-in** par défaut
  pour ne pas changer COLLAB-1.
- Effet de bord neutre : le binaire `ravum-collab` a de nouvelles options (`--verify`,
  `--verify-tests`, `--publication-journal`) ; les options existantes restent valides.
- `Cargo.lock` a évolué (membre daemon + arête coordinator→adapter) ; aucun crate externe
  nouveau n'a été ajouté par MVP-0b.

## 21. Limitations

- Cible P0-A **non atteinte sur code modifié** (analyse 95–262 ms p50 selon la taille) ;
  cycle complet mesuré : p95 335–362 ms sur petite unité (< 500 ms) mais 702–743 ms sur
  l'unité de 2 313 lignes ; le goulot est le **build+link à froid**, non couvert par le
  daemon.
- Daemon : analyse seule (pas de codegen/link), pas de trace de réutilisation par requête,
  état disque non collecté, révisions non persistées, pas de client éditeur/LSP.
- S8 : agents scriptés (pas de LLM), n=5, éditions triviales, fixture locale ; verdict
  **défavorable** au workspace partagé en temps.
- COLLAB-2/3 : pas d'AST complète (heuristiques), pas de CRDT, vérification limitée à une
  racine de crate inférée, 2 faux conflits assumés, `--verify-tests` exécute du code du
  snapshot (opt-in, pas de bac à sable).
- Audio : simulateur seulement ; pas de device, pilote, XRUN, priorité RT ; jigue OS
  mesurée mais non représentative du pire cas.
- Vérification : aucun prouveur externe, aucun `PROVED` général, aucune preuve de sûreté
  mémoire du code natif.
- E2E navigateur : Chromium/Edge uniquement, pas d'a11y, pas de TLS, un onglet.
- CI impact : pas d'analyse sémantique (macros, `cfg`, `build.rs`), parseur TOML restreint,
  non encore exécutée sur GitHub Actions.
- Licence non choisie ; MSRV 1.74 non vérifiée ; protection de branches contournable par
  l'administrateur (`enforce_admins=false`, assumé pour un mainteneur solo).

## 22. Risques restants

| Risque | Sévérité | Mitigation actuelle | Reste à faire |
|---|---|---|---|
| Aucune qualification audio réelle | haute | simulateur + mesure de jigue honnête | harnais WASAPI/CLAP, XRUN, plateforme qualifiée |
| P0-A toujours manquée sur code modifié | haute | daemon + mesure | réduire le plancher de démarrage `rustc` (processus persistant, stratégie adaptative) |
| S8 défavorable sans mesure LLM | moyenne | verdict publié | campagne avec vrais agents, tokens mesurés |
| Faux conflits COLLAB (2/10) | moyenne | fail-closed + documentation | hybride ancres+symboles |
| Croissance disque du daemon | moyenne | bornes mémoire, mesures | GC/compaction |
| Hooks Git = code local de confiance | moyenne | exécution explicite | isolation/timeout si usage non fiable |
| Prouveur absent | moyenne | statuts honnêtes | installer Kani + harness |
| CI impact non éprouvée en CI | faible | selftest 48 cas | première PR vers `dev` |
| `enforce_admins=false` | faible | assumé mainteneur solo | revoir si second mainteneur |

## 23. Décisions nécessitant approbation du responsable

1. **Fusion de la PR #78** (MVP-0a) puis de la **PR #80** (MVP-0b), et ordre de fusion.
   Aucune fusion n'a été faite. Après fusion de #78, #80 doit être retargetée vers `dev`.
2. **Licence** (#34) : option recommandée `MIT OR Apache-2.0` pour l'outillage, licence
   documentaire séparée à décider, DCO à instaurer ; **rien n'est publié**.
3. **Statut de la RFC #79** (shared workspace) : nourrie par COLLAB-2/3 et S8, toujours
   `PROPOSED` ; aucun numéro ADR attribué.
4. **Priorité suivante** : (a) intégration L2/COLLAB-4 et mesure de bout en bout
   édition→preview ; (b) harnais audio réel ; (c) campagne S8 avec vrais agents.
5. **Durcissement de `dev`** avec checks requis, une fois la CI sélective éprouvée.
6. Acceptation éventuelle des ADR touchés (aucun ne l'a été par un agent).

## 24. Prochaines étapes proposées

1. Réduire le coût d'une modification réelle, désormais identifié comme le **build+link à
   froid** (230–435 ms) : cache de builds par contenu étendu, `-C incremental` sur la
   compilation cdylib, lien incrémental mesuré, puis re-mesurer le cycle complet avec
   `ravum cycle-bench`.
2. Intégrer `AcceptedChange` (COLLAB) au daemon : invalidation ciblée par chemins/symboles,
   journal d'activations partagé, identifiants de snapshots communs (COLLAB-4).
3. Harnais audio WASAPI shared-mode silencieux + plugin CLAP minimal, mesures XRUN.
4. Installer Kani dans un environnement dédié et écrire un harness borné versionné.
5. GC/compaction de l'état incrémental ; soak test mémoire ; deux daemons sur un `work_dir`.
6. Hybride ancres syntaxiques + symboles pour supprimer les 2 faux conflits ; campagne S8
   avec de vrais agents IA et comptage de tokens.
7. Première exécution du workflow `mvp-impact.yml` sur une PR vers `dev`.

## 25. Sauvegardes Obsidian publiées (liens vérifiés sur GitHub)

Les deux notes ont été validées par `python _system/tooling/vault.py check --format json`
(vert), commitées avec `docs(ravum): …` et poussées en fast-forward sur `master`, puis
relues sur le dépôt distant :

1. [Session MVP-0b L0–L1 — durcissement temps réel et Git transactionnel](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-08-mvp0b-l0-l1-rt-and-git-hardening.md)
   — commit vault `f50795a` (lecture distante vérifiée).
2. [Session MVP-0b L2–L4 — incrémentalité, collaboration, E2E et qualification](https://github.com/Rwanbt/obsidian-dev-brain/blob/master/projects/ravum/operations/sessions/2026-11-08-mvp0b-incremental-collab-e2e-qualification.md)
   — commit vault `37b22b0` (lecture distante vérifiée ; sommet de `master` au moment de la publication).

## Renvois

- Audit : [MVP-0b-audit.md](MVP-0b-audit.md) · Temps réel : [MVP-0b-rt-lifecycle.md](MVP-0b-rt-lifecycle.md)
- Crossfade : [MVP-0b-dsp-crossfade.md](MVP-0b-dsp-crossfade.md) · Incrémental : [MVP-0b-incremental.md](MVP-0b-incremental.md)
- Collaboration : [MVP-0b-collab-2-3.md](MVP-0b-collab-2-3.md) · S8 : [MVP-0b-s8.md](MVP-0b-s8.md)
- E2E navigateur : [MVP-0b-browser-e2e.md](MVP-0b-browser-e2e.md) · CI impact : [MVP-0b-ci.md](MVP-0b-ci.md)
- Audio réel : [MVP-0b-audio-host.md](MVP-0b-audio-host.md) · Vérification : [MVP-0b-verification.md](MVP-0b-verification.md)
- Licence : [MVP-0b-licence-analysis.md](MVP-0b-licence-analysis.md)
- Statut : [../architecture/STATUS.md](../architecture/STATUS.md) · PR [#80](https://github.com/Rwanbt/Ravum/pull/80) · PR [#78](https://github.com/Rwanbt/Ravum/pull/78)

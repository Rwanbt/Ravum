# MVP-0b — S8 : worktrees Git vs workspace partagé Ravum vs agents séquentiels

> **Statut des preuves.** Ce document contient (1) le **protocole pré-enregistré**,
> rédigé **avant** l'exécution des campagnes, puis (2) les **résultats mesurés**
> et le **verdict honnête**. Les chiffres de la partie résultats proviennent
> exclusivement des fichiers `evidence/mvp0b-s8-*.json` produits par
> `benchmarks/s8/run-s8.mjs`. Aucune valeur n'est estimée ni inventée.
>
> **Les « agents » sont des éditeurs scriptés déterministes, PAS des LLM.**
> Aucun token IA n'est consommé ni mesuré ; le temps de supervision humaine
> n'est pas mesuré. Les seuls coûts mesurés sont mécaniques (wall-clock,
> conflits, écritures, RAM/CPU, cache, disque).

## 1. Question posée

Sur un **même projet**, avec les **mêmes demandes d'édition**, le **même nombre
d'agents (3)**, les **mêmes tests** et la **même machine**, quelle organisation
minimise le temps total jusqu'à `cargo test` vert, le temps d'attente subi par
les agents, les conflits et les pertes de changements acceptés ?

Trois organisations comparées :

| Scénario | Organisation |
|---|---|
| **A** | Un worktree Git par agent, branche séparée par agent (`git worktree add`), puis intégration séquentielle des branches par `git merge --no-ff` réels dans la branche `integration`. |
| **B** | Un seul workspace Git partagé. Les agents **n'écrivent jamais** dans le workspace : ils produisent des `ChangeProposal` JSON (snapshot de base + hashes attendus + diff unifié). Le coordinateur Ravum décide (`AUTO_SAFE`/`CONFLICT`/`MANUAL_REQUIRED`/`REBASE_REVALIDATE`/`REJECT`) puis l'écrivain Git unique `GitWriter` publie transactionnellement en **une** publication (`prepare`/`publish`, index isolé, commit non publié, compare-and-swap de référence, journaux durables). |
| **C** | Contrôle : agents séquentiels sur une branche unique, mêmes éditions appliquées l'une après l'autre, sans aucun contrôle de concurrence. |

## 2. Fixture et corpus (figés avant exécution)

### Fixture

`benchmarks/s8/fixture/` : crate Rust autonome (déclare `[workspace]` vide dans
`Cargo.toml`, donc jamais membre du workspace Ravum).

- 4 modules : `alpha`, `beta`, `gamma`, `delta` ;
- 2 fonctions par module (8 fonctions au total) ;
- 10 tests dans `tests/behavior.rs`.

### Corpus : 6 demandes d'édition, 3 agents (2 tâches chacun)

Chaque tâche est une opération de remplacement de ligne **déterministe**
(1 ligne → 1 ligne), si bien que le nombre de lignes de chaque fichier ne change
jamais. Cela rend les hunks sans contexte applicables de façon exacte même quand
un autre agent a modifié une autre ligne du même fichier.

| Tâche | Agent | Nature | Fichiers | Symbole | Propriété |
|---|---|---|---|---|---|
| `t1` | agent-1 | implémentation | `src/alpha.rs` | `alpha_add` | édition indépendante |
| `t2` | agent-1 | implémentation | `src/gamma.rs` | `gamma_prefix` | même fichier que `t5`, fonction différente |
| `t3` | agent-2 | **changement de signature** | `src/beta.rs`, `tests/behavior.rs` | `beta_pad` | ajoute un paramètre `fill` + met à jour 2 sites d'appel |
| `t4` | agent-2 | implémentation | `src/delta.rs` | `delta_count` | **conflit contrôlé** avec `t6` |
| `t5` | agent-3 | implémentation | `src/gamma.rs` | `gamma_join` | même fichier que `t2`, fonction différente |
| `t6` | agent-3 | implémentation concurrente | `src/delta.rs` | `delta_count` | **conflit contrôlé** avec `t4` (même unité) |

Le corpus couvre donc les quatre cas demandés : fichiers différents, fonctions
différentes d'un même fichier, changement de signature, conflit contrôlé.

**Résultat attendu du conflit (pré-enregistré).** L'ordre d'intégration est
déterministe (`t4` avant `t6`) : le côté déjà intégré gagne.

- **A** : `git merge` de la branche d'agent-3 produit un conflit textuel sur
  `src/delta.rs` ; résolution pré-enregistrée = garder le côté intégré
  (`checkout --ours`), puis commit de merge. `t6` est écarté, `t4` conservé.
- **B** : la seconde proposition reçoit `CONFLICT` (même symbole déjà modifié) et
  n'est pas publiée. Ce refus est un **résultat mesuré**, pas un échec du harnais.
- **C** : `t6` est appliqué après `t4` et l'écrase **silencieusement** (aucun
  contrôle de concurrence) : c'est une perte mesurée, par construction du contrôle.

## 3. Protocole d'exécution

- Chaque répétition travaille sur une **copie éphémère** de la fixture dans le
  dossier temporaire du run (`%TEMP%/ravum-s8-<stamp>/<scénario><rep>/`), avec un
  dépôt Git neuf (`core.autocrlf=false`, aucune hook locale) et **aucun cache
  `target/` partagé** : chaque répétition paie une compilation froide identique.
- Rien n'est écrit dans le dépôt Ravum : seuls les fichiers `evidence/` et
  `benchmarks/s8/results/latest.md` sont produits.
- 3 agents = 3 processus Node concurrents ; chaque agent exécute ses 2 tâches
  séquentiellement. Durée mesurée en wall-clock (`performance.now()`).
- **Ordre alterné** des campagnes : bloc `A,B,C`, puis bloc `C,B,A`, puis
  `A,B,C`, etc., pour répartir la dérive de charge machine entre scénarios.
- **Répétitions : 5 par scénario** (minimum exigé).
- Critère de réussite : `cargo test` du projet fixture **vert** après intégration
  et **aucun changement accepté perdu**.

### Métriques mesurées par répétition

Temps total jusqu'à tests verts (ms) ; phase d'édition des agents (ms) ; phase
d'intégration/publication (ms) ; `cargo test` (ms) ; attente cumulée des agents
(ms) ; nombre de conflits ; temps de résolution des conflits (ms) ; nombre de
merges/publications/commits ; nombre de rebuilds (invocations `cargo`) ; volume
de cache (`target/`, `.git/`, disque de la réplique) ; RAM (pic de WorkingSet
agrégé : coordinateur et testeurs séparément) ; CPU consommé ; charge machine
documentée (RAM libre, `LoadPercentage`, nombre de processus) ; taux de réussite
fonctionnelle.

### Critères de succès pré-enregistrés (évalués par répétition)

- **C1** — `cargo test` vert (10/10) après intégration, dans les trois scénarios.
- **C2** — 0 édition **indépendante** perdue (`t1`, `t2`, `t3`, `t5`, et `t4` en
  A/B ; en C, `t4` est le perdant pré-vu du conflit et compté séparément).
- **C3** — conflit contrôlé `t4`/`t6` **détecté et arbitré sans écriture
  silencieuse** : exigé en A et B ; en C (contrôle sans détection), la perte
  silencieuse est au contraire **mesurée** (attendu : 1).
- **C4** — intégrité du flux B : agents n'ayant pas écrit le workspace avant
  publication ; publication unique effectuée par `GitWriter` avec trailers
  `Ravum-Proposal`/`Ravum-Agent` ; `git status` propre ; un seul commit de
  publication.
- **C5** — intégrité du flux A : 3 worktrees Git réels et 3 commits de merge.

`réussite fonctionnelle stricte = C1 ∧ C2 ∧ C3` pour A et B ; pour C, le harnais
vérifie C1 ∧ C2 ∧ « perte silencieuse = 1 attendue ».

## 4. Reproductibilité

- Binaire coordinateur **épinglé** : construit par le harnais depuis la référence
  Git **`7026f21`** (HEAD au début de la mission S8) via `git archive`, dans un
  `--target-dir` temporaire séparé. Raison : le dépôt est partagé et des agents
  modifient/compilent le crate `ravum-workspace-coordinator` en parallèle ; un
  binaire pris dans `target/debug` de la racine ne serait pas reproductible.
- Outils : Node 26, cargo 1.95.0, git 2.51.1, Windows 11, 10 cœurs / 16 threads.
- Commandes : `pwsh -File scripts/s8.ps1` (équivalent
  `node benchmarks/s8/run-s8.mjs --reps 5 --scenarios A,B,C`).

## 5. Limites déclarées (pré-enregistrées)

1. **Agents scriptés, pas de LLM** : aucune mesure de tokens, de qualité de code
   généré, ni de temps de supervision humaine ; les tâches sont des
   remplacements de lignes figés.
2. **Échantillon restreint** : 5 répétitions par scénario.
3. **Fixture locale petite** : 4 modules, 8 fonctions, 10 tests ; pas de
   dépendances externes, pas de réseau.
4. **Machine partagée** : d'autres agents compilent pendant les campagnes ; la
   charge est mesurée et documentée mais non contrôlée.
5. **Compilation froide par répétition** : chaque répétition recompile la fixture,
   ce qui amortit les différences d'organisation dans le temps total.
6. **Échantillonnage RAM/CPU** : granularité ~200–300 ms ; les pics très courts
   (rustc) peuvent être sous-échantillonnés ; les valeurs sont comparatives.
7. **Portée sémantique** : le coordinateur applique des hunks textuels exacts et
   détecte les conflits par symbole/plage ; il ne fait pas de fusion sémantique
   (limite déjà documentée par COLLAB-1). Le conflit `t4`/`t6` est textuel, pas
   sémantique.

---

# RÉSULTATS (mesurés, ajoutés après exécution)

## 6. Exécution

| Élément | Valeur |
|---|---|
| Date du run (Europe/Paris) | **2026-11-08, départ ~14:44:36, fin ~14:47:06** |
| Commande exacte | `pwsh -NoProfile -File scripts/s8.ps1 -Reps 5` |
| Durée wall-clock du run complet | **150,4 s** (15 campagnes + préparation + vérifications + écriture des preuves) |
| Code de sortie | **0** (`Résultat global : OK (campagnes exploitables et conformes)`) |
| Répétitions | **5 par scénario**, soit 15 campagnes |
| Ordre des campagnes | A1 B1 C1 · C2 B2 A2 · A3 B3 C3 · C4 B4 A4 · A5 B5 C5 |
| Validation du protocole | `ok: true`, `problems: []` (aucun échec C1–C5) |
| Preuves brutes | `evidence/mvp0b-s8-A-2026-11-08.json` (204 Ko), `-B-` (255 Ko), `-C-` (173 Ko), `-summary-` (46 Ko) |
| Rapport généré | `benchmarks/s8/results/latest.md` |

### Environnement machine (mesuré)

- Windows 11 Professionnel, Intel Core i7-13620H, **10 cœurs / 16 threads**, 15,7 Go RAM, 197,8 Go libres.
- Node v26.13.0, cargo 1.95.0, rustc 1.95.0, git 2.51.1.windows.1.
- **Machine partagée** : d'autres agents compilaient pendant le run (charge documentée ci-dessous).

### Binaire coordinateur épinglé (reproductibilité)

Le crate `ravum-workspace-coordinator` était **modifié et recompilé en parallèle**
par un autre agent pendant la mission (HEAD de branche avancé de `7026f21` à
`25d9856`, crate modifié non commité, binaire de `target/debug` changé). Pour un
benchmark reproductible, le harnais n'utilise **pas** ce binaire de travail :

- référence épinglée : **`7026f21`** (HEAD de mission S8), commit `7026f21554c1a5313c69514c90c2b00322dedb1e` ;
- export par `git archive` (sans toucher au worktree ni aux branches) ;
- compilation dans `%TEMP%/ravum-s8-coordinator-cache/<sha>/target` (`cargo build -p ravum-workspace-coordinator --bins --target-dir …`, hors ligne) ;
- binaire SHA-256 `2b845149cdac6896f6e72a9b68cc98e2543c04553bf014851a13e452fe1fd0d6`, build incrémental 370 ms.

État du dépôt au démarrage du run : branche `feat/mvp0b-hardening`, HEAD
`25d9856`, 38 entrées modifiées/non suivies (travail d'autres agents, intact et
non touché par ce harnais). Aucun commit n'a été créé par le benchmark.

### Charge machine (sondes complètes et par répétition)

| Sonde | RAM libre | `LoadPercentage` | Processus |
|---|---|---|---|
| Début de campagne | 3 277 Mo | 49 % | 303 |
| Mi-campagne | 3 128 Mo | 43 % | 305 |
| Fin de campagne | 2 822 Mo | 46 % | 318 |

La charge CPU moyenne **pendant chaque répétition** (mesurée par delta des temps
CPU OS) allait de **49,3 % à 80,5 %** : la machine était chargée et sa charge a
dérivé pendant le run. L'ordre alterné A/B/C répartit cette dérive entre les
scénarios, mais **la variance wall-clock en est fortement augmentée** (IQR de
1,2 à 1,5 s sur le temps total).

## 7. Tableau comparatif A/B/C

Médiane `[min–max]` (IQR), n = 5 répétitions par scénario. Timings en ms
wall-clock.

| Métrique | **A — worktrees** | **B — workspace partagé** | **C — séquentiel** |
|---|---|---|---|
| **Temps total jusqu’à tests verts** | **5 660,56** [4 453,3–6 260,5] (IQR 1 547,8) | **6 677,62** [5 428,7–7 637,8] (IQR 1 246,5) | **4 785,64** [4 256,1–6 221,7] (IQR 1 318,3) |
| Phase d’édition des agents | 957,07 [815,0–1 036,3] (114,9) | 17,51 [15,2–24,1] (1,5) | 2 067,98 [1 617,1–2 938,4] (327,5) |
| Phase d’intégration/publication | 1 243,35 [1 080,9–1 798,0] (565,6) | 2 467,69 [2 082,8–3 092,3] (442,5) | 0 (aucune) |
| `cargo test` final | 3 284,52 [2 425,1–3 397,6] (800,5) | 2 702,45 [2 373,8–3 671,4] (626,0) | 2 767,09 [2 299,0–3 488,5] (580,5) |
| **Attente cumulée des agents** | 2 053,52 [1 877,2–2 844,6] (693,1) | **7 750,82** [6 549,6–9 763,0] (1 294,3) | 0,14 [0,10–0,19] (0,01) |
| Conflits détectés | 1 [1–1] | 1 [1–1] | **0** |
| Temps de résolution des conflits | 433,61 [338,7–659,2] (172,9) | 0 (refus immédiat, sans écriture) | 0 (aucune détection) |
| Merges / publications / commits | 3 merges [3–3] | 1 publication [1–1] | 6 commits [6–6] |
| Rebuilds (invocations cargo) | 1 [1–1] | 1 [1–1] | 1 [1–1] |
| Tests verts (sur 10) | **10/10** | **10/10** | **10/10** |
| **Changements acceptés perdus** | **0** | **0** | **1** (perte silencieuse) |
| Éditions indépendantes perdues | 0 | 0 | 0 |
| Écrasements silencieux | 0 | 0 | **1** |
| RAM pic totale suivie (Mo WorkingSet) | 145,8 [84,0–161,1] | 100,5 [85,8–221,9] | 149,3 [83,3–162,8] |
| RAM pic coordinateur `ravum-collab` (Mo) | 0 | **5,5** [5,5–5,5] | 0 |
| RAM pic testeurs cargo/rustc/test (Mo) | 138,8 [77,0–154,1] | 93,5 [78,8–214,9] | 142,3 [76,3–155,9] |
| CPU total consommé (s) | 2,11 [1,48–2,53] | 2,06 [1,80–2,64] | 2,22 [1,27–2,84] |
| Charge CPU machine pendant la répétition (%) | 73,6 [56,1–80,1] | 61,6 [49,5–72,7] | 60,6 [49,3–80,5] |
| Cache `target/` de la réplique (Mo) | 12,52 | 12,52 | 12,52 |
| Objets `.git/` de la réplique (Mo) | 0,04 | 0,03 | 0,04 |
| Espace disque de la réplique (Mo) | 12,57 | 12,56 | 12,56 |
| **Réussite fonctionnelle stricte** | **5/5** | **5/5** | **0/5** (C3 non satisfait par construction) |

### Échantillons bruts (15 répétitions)

| rep | ordre | total_ms | edit_ms | integr_ms | test_ms | attente_ms | conflits | writes | tests | perdus | RAM_tot_Mo | RAM_test_Mo | RAM_coord_Mo | CPU_s | target_Mo |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A1 | 1 | 4652,29 | 815,01 | 1243,35 | 2485,48 | 2053,52 | 1 | 3 | 10/10 | 0 | 84,0 | 77,0 | 0 | 1,48 | 12,52 |
| B1 | 2 | 6677,62 | 15,23 | 2648,41 | 2702,45 | 8228,32 | 1 | 1 | 10/10 | 0 | 85,8 | 78,8 | 5,5 | 2,06 | 12,52 |
| C1 | 3 | 4785,64 | 2067,98 | 0 | 2556,71 | 0,19 | 0 | 6 | 10/10 | 1 | 149,3 | 142,3 | 0 | 2,84 | 12,52 |
| C2 | 4 | 4483,60 | 1617,10 | 0 | 2767,09 | 0,10 | 0 | 6 | 10/10 | 1 | 162,8 | 155,9 | 0 | 2,00 | 12,52 |
| B2 | 5 | 5796,14 | 18,36 | 2467,69 | 2373,79 | 7750,82 | 1 | 1 | 10/10 | 0 | 221,9 | 214,9 | 5,5 | 2,64 | 12,52 |
| A2 | 6 | 4453,30 | 851,50 | 1080,92 | 2425,13 | 1877,17 | 1 | 3 | 10/10 | 0 | 109,0 | 102,0 | 0 | 2,09 | 12,52 |
| A3 | 7 | 5660,56 | 1036,34 | 1194,52 | 3284,52 | 2052,82 | 1 | 3 | 10/10 | 0 | 154,6 | 147,5 | 0 | 2,53 | 12,52 |
| B3 | 8 | 7042,59 | 16,85 | 2205,88 | 3671,36 | 6934,04 | 1 | 1 | 10/10 | 0 | 100,5 | 93,5 | 5,5 | 2,27 | 12,52 |
| C3 | 9 | 6221,69 | 2938,41 | 0 | 3137,20 | 0,14 | 0 | 6 | 10/10 | 1 | 85,1 | 78,1 | 0 | 2,22 | 12,52 |
| C4 | 10 | 4256,09 | 1843,90 | 0 | 2298,96 | 0,14 | 0 | 6 | 10/10 | 1 | 83,3 | 76,3 | 0 | 1,27 | 12,52 |
| B4 | 11 | 5428,69 | 17,51 | 2082,81 | 2437,02 | 6549,63 | 1 | 1 | 10/10 | 0 | 168,0 | 160,9 | 5,5 | 2,00 | 12,52 |
| A4 | 12 | 6260,46 | 957,07 | 1760,08 | 3397,59 | 2745,91 | 1 | 3 | 10/10 | 0 | 145,8 | 138,8 | 0 | 2,31 | 12,52 |
| A5 | 13 | 6200,09 | 966,38 | 1797,96 | 3285,95 | 2844,64 | 1 | 3 | 10/10 | 0 | 161,1 | 154,1 | 0 | 2,11 | 12,52 |
| B5 | 14 | 7637,82 | 24,10 | 3092,26 | 3063,07 | 9763,00 | 1 | 1 | 10/10 | 0 | 86,0 | 78,9 | 5,5 | 1,80 | 12,52 |
| C5 | 15 | 5801,91 | 2171,38 | 0 | 3488,54 | 0,14 | 0 | 6 | 10/10 | 1 | 161,8 | 154,8 | 0 | 2,58 | 12,52 |

`perdus` = changements acceptés perdus ; `écrasés` (non affiché) = 1 pour
chaque C, 0 pour A/B. Détail complet par répétition dans les fichiers
`evidence/mvp0b-s8-*.json`.

## 8. Vérifications demandées (exécutées réellement)

### 8.1 Scénario A — vrais worktrees Git et vrais merges

Vérifié à chaque répétition (extrait A1, identique en structure pour A1–A5) :

```
worktree <tmp>/A1/repo      HEAD <sha>  branch refs/heads/integration
worktree <tmp>/A1/agent-1   HEAD <sha>  branch refs/heads/agent-1
worktree <tmp>/A1/agent-2   HEAD <sha>  branch refs/heads/agent-2
worktree <tmp>/A1/agent-3   HEAD <sha>  branch refs/heads/agent-3
```

- 3 worktrees réels + 1 worktree d'intégration ; **3 commits de merge**, 6 commits
  d'agents, **9 commits** dans `git rev-list --count base..HEAD` ;
- conflit de merge réel : `src/delta.rs` (`t4` vs `t6`), résolu par la politique
  pré-enregistrée « garder le côté déjà intégré » (`checkout --ours` + commit de
  merge), en 338,7–659,2 ms selon la répétition ;
- marqueurs finaux : `t1,t2,t3,t4,t5` présents, `t6` absent ; 0 changement perdu.

### 8.2 Scénario B — publication réellement effectuée par le `GitWriter`

Vérifié aux **5 répétitions** (structure identique) :

- décisions coordinateur : **5 `AUTO_SAFE` + 1 `CONFLICT`** ; le refus est
  explicite : `symbol "delta_count" is already modified by agent-2/t4 (agent agent-2)` ;
- les agents **n'ont pas touché** le workspace avant publication
  (`git status --porcelain` vide avant l'intégration) ;
- `ravum-collab integrate … --commit` a produit **exactement 1 commit de
  publication** par répétition (aucun commit intermédiaire) :

  | rep | commit de publication | parent (= commit de base) | chemins publiés |
  |---|---|---|---|
  | B1 | `b1354021ca97…` | `2fb645843c0f…` | 5 |
  | B2 | `99d841153c46…` | `0246db36ac45…` | 5 |
  | B3 | `dc119a01515f…` | `b904bb8dd2a0…` | 5 |
  | B4 | `12e6cc30d170…` | `25f4a3c4bd2a…` | 5 |
  | B5 | `0f55f4d8adc3…` | `d59cc75b865a…` | 5 |

- sujet : `chore(collab): integrate 5 proposal(s) touching 5 path(s)` ; auteur
  `ravum-coordinator <coordinator@ravum.invalid>` ;
- chemins publiés : `src/alpha.rs, src/beta.rs, src/delta.rs, src/gamma.rs,
  tests/behavior.rs` ;
- corps du commit avec trailers **`Ravum-Proposal`**, **`Ravum-Agent`** et
  **`Ravum-Publication-Id: collab:agent-1/t1,agent-1/t2,agent-2/t3,agent-2/t4,agent-3/t5`** ;
- journaux : 6 entrées de décisions journalisées + journal de publication avec
  `INTENT` et `OUTCOME/PUBLISHED` (2 enregistrements, mêmes `publication_id` et
  `commit`) ; `git status` propre après publication et après tests ;
- contenu final identique à la référence A/B (`t4` présent, `t6` absent),
  0 changement perdu.

### 8.3 Scénario C — contrôle et perte silencieuse mesurée

- 6 commits séquentiels par répétition (`src/alpha.rs`, `src/gamma.rs`,
  `src/beta.rs`+`tests/behavior.rs`, `src/delta.rs`, puis les deux éditions
  concurrentes de `delta_count`) ;
- `t4` (commit `cafe792…`) puis `t6` écrasent la même ligne : l'état final
  contient `t6` et **plus** `t4` ; **aucun conflit n'est détecté** ;
- mesure identique dans les 5 répétitions : 1 changement accepté perdu,
  1 écrasement silencieux, tests verts (les deux variantes sont
  comportementalement équivalentes — la perte est textuelle et d'intention).

## 9. Verdict honnête

**Sur ce protocole, le workspace partagé avec coordinateur Ravum n'est PAS plus
rapide. Il est le plus lent des trois en temps total jusqu'à tests verts.**

Comparaisons appariées par répétition (mêmes conditions de charge à quelques
minutes d'intervalle) :

- **B vs C** : B plus lent dans **5/5** répétitions, de **+821 à +1 892 ms**
  (médiane **+1 313 ms, soit +27 %** vs C).
- **B vs A** : B plus lent dans **4/5** répétitions (+1 343 à +2 026 ms ; une
  répétition B4 est plus rapide de 831 ms dans un contexte de charge très
  différent, C4/A4 à 49 %/80 % de CPU).

Décomposition mécanique mesurée de l'écart B :

1. **Phase d'édition très rapide (17,5 ms)** : les agents B ne touchent jamais au
   workspace, ils produisent un diff + des hashes en mémoire. C'est ~55× plus
   rapide que l'édition en worktrees (957 ms) et ~118× plus rapide que
   l'édition séquentielle (2 068 ms).
2. **Mais phase d'intégration centralisée plus coûteuse (2 468 ms)** que les
   3 merges Git de A (1 243 ms) : lecture immuable du snapshot complet du
   workspace, décisions séquentielles, publication transactionnelle par le
   `GitWriter` (index isolé, commit non publié, compare-and-swap, réconciliation
   du disque et de l'index, journaux durables).
3. **Et attente des agents beaucoup plus forte (7 751 ms cumulés)** : les agents
   attendent la décision centralisée (6 549–9 763 ms cumulés selon la
   répétition) contre 1 877–2 845 ms d'attente de merge en A et ~0,14 ms en C.

Le workspace partagé **n'est pas non plus supérieur à A sur le temps** : médiane
+18 % vs A, avec un chevauchement important des dispersions (IQR ≈ 1,2–1,5 s) et
une machine dont la charge a dérivé de 49 % à 80 % pendant le run.

**A vs C** : quasi-parité en temps total sur cet échantillon (A plus rapide dans
3/5 des paires, médiane des écarts appariés −30 ms ; le +18 % de la médiane vient
de A4/A5 exécutées à 80 %/78 % de charge CPU). A parallélise l'édition
(957 ms vs 2 068 ms) mais paie 1 243 ms d'intégration et 434 ms de résolution de
conflit, ce qui annule le gain.

**Ce que B achète avec son surcoût mesuré**, et que C ne peut pas offrir :
refus explicite sans écriture (`CONFLICT` en 0 ms, aucun état modifié), journal
durable de chaque décision, **une seule publication transactionnelle** attribuée
par proposition, `git status` propre, **0 changement accepté perdu**. C, le plus
rapide en médiane, **perd 1 édition acceptée sur 5/5 répétitions**, silencieusement.

Conclusion à retenir : ce benchmark **ne démontre pas** que le workspace partagé
est préférable en vitesse ; il mesure le coût de ses garanties. Il démontre en
revanche que l'absence de coordination (C) perd des changements acceptés même
sur 6 éditions triviales et déterministes, et que A (worktrees + merges) est le
seul scénario testé qui combine préservation des changements, détection des
conflits et temps comparable au contrôle séquentiel.

## 10. Limites (confirmées après exécution)

1. **Agents scriptés déterministes, pas des LLM** : aucun token, aucune qualité de
   code généré, aucun temps de supervision humaine mesurés. Les 6 tâches sont
   des remplacements d'une ligne figés à l'avance.
2. **Échantillon restreint** : 5 répétitions, 6 éditions, 10 tests.
3. **Fixture locale petite** : un seul crate Rust sans dépendances ; `cargo test`
   froid par répétition pèse 2,4–3,5 s sur un total de 4,3–7,6 s et **amortit**
   les différences d'organisation.
4. **Machine partagée** : d'autres agents compilaient pendant tout le run
   (49–80 % de charge CPU, 2,8–3,3 Go de RAM libre) ; la variance est grande et
   non contrôlée, ce qui interdit de conclure à des écarts de quelques centaines
   de ms entre A et C.
5. **Compilation froide** : aucun cache `target/` partagé ni incrémental ; les
   trois scénarios paient exactement la même compilation (12,52 Mo de cache).
6. **RAM/CPU échantillonnées à ~200–300 ms** par un processus PowerShell
   persistant ; les pics très courts (rustc, ~0,5–0,7 s) peuvent être manqués
   selon le nombre de rustc parallèles capturés — d'où la dispersion 76–215 Mo
   des « testeurs ». L'échantillonneur refuse les PID réutilisés (protection
   d'identité par heure de démarrage) après deux artefacts détectés et corrigés.
7. **Portée sémantique** : conflit textuel par symbole/plage, pas de fusion
   sémantique ; `t4` et `t6` sont comportementalement équivalents, la perte
   mesurée en C est donc textuelle et d'intention, pas fonctionnelle (les tests
   restent verts).
8. **Binaire B épinglé à `7026f21`** (HEAD de mission) et non au HEAD courant
   `25d9856` : c'est le choix reproductible, documenté ci-dessus ; les deux
   versions du coordinateur ne sont pas comparées.

## 11. Fichiers et commandes

- Harnais : `benchmarks/s8/run-s8.mjs`, `benchmarks/s8/lib/*.mjs`,
  `benchmarks/s8/lib/sampler.ps1`, fixture `benchmarks/s8/fixture/`,
  README `benchmarks/s8/README.md`, rapport généré
  `benchmarks/s8/results/latest.md`.
- Lanceur reproductible : `scripts/s8.ps1` (propage le code de sortie Node).
- Preuves : `evidence/mvp0b-s8-A-2026-11-08.json`,
  `evidence/mvp0b-s8-B-2026-11-08.json`,
  `evidence/mvp0b-s8-C-2026-11-08.json`,
  `evidence/mvp0b-s8-summary-2026-11-08.json`.
- Commandes exactes :

```powershell
# campagne officielle (celle dont les résultats sont ci-dessus)
pwsh -NoProfile -File scripts/s8.ps1 -Reps 5

# équivalent direct
node benchmarks/s8/run-s8.mjs --reps 5 --scenarios A,B,C

# inspection d'une campagne unique (répliques conservées)
node benchmarks/s8/run-s8.mjs --reps 1 --scenarios A,B,C --keep
```

Aucun commit ni changement de branche n'a été effectué par le benchmark dans le
dépôt Ravum ; les commits cités appartiennent aux répliques temporaires.


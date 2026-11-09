# MVP-0c — S8 réel : protocole, pilote scripté et procédure de campagne IA

> **Pré-enregistrement.** La partie « protocole » de ce document a été rédigée
> **avant** l'exécution du pilote. Le harnais enregistre dans la preuve JSON le
> `sha256` de ce fichier au démarrage (`protocol.doc_sha256_at_run`), ce qui
> permet de vérifier que les critères n'ont pas été ajustés après les mesures.
>
> **Pilote = agents scriptés déterministes, PAS des LLM.** Aucun token IA n'est
> mesuré au pilote : ce ne sont pas des mesures inventées, c'est un mode déclaré
> (`tokens.measured = false`). La campagne réelle avec subagents est décrite
> section 8 et réutilise le même harnais (`--mode live`).

## 1. Objectif et hypothèses

Question : sur une même fiabrique Rust réelle, avec 3 agents et 4 tâches
indépendantes, quelles sont les différences mesurables entre **A** (une copie
par agent puis intégration), **B** (répertoire partagé, écritures concurrentes
et détection de conflits) et **C** (séquentiel) — en temps, attentes, conflits,
changements perdus, builds, ressources, et **tokens réels par agent** lors de la
campagne IA ?

Hypothèses pré-enregistrées (à confirmer/infirmer, pas à imposer) :

- **H1** — En B, la phase d'édition est plus courte qu'en A/C (pas de copies,
  pas de sérialisation) mais une détection/résolution de collision ajoute un
  coût visible.
- **H2** — A paie l'intégration (fusion) après coup ; C paie la sérialisation
  pendant l'édition ; le total A vs C dépend du coût fixe de compilation.
- **H3** — Les trois stratégies préservent les 4 éditions (0 changement perdu)
  *si* B détecte bien les collisions ; sans détection, B perdrait une écriture.
- **H4** — En campagne réelle, le temps total et les tokens seront dominés par
  les tours de LLM et la supervision humaine, pas par la mécanique de
  collaboration ; le pilote ne peut donc pas extrapoler les tokens.

## 2. Stratégies comparées (pré-enregistrées)

Les trois stratégies partent de la **même fixture**, reçoivent les **mêmes 4
tâches**, le **même nombre d'agents (3)**, les **mêmes tests** et la **même
machine**.

| Stratégie | Organisation |
|---|---|
| **A** | Chaque agent travaille dans sa **copie privée** de la fixture (3 copies). Après l'édition, le harnais intègre les résultats par **fusion 3-way ligne à ligne en mémoire**, puis écrit une copie d'intégration. Aucun conflit n'est attendu sur ce corpus ; un conflit éventuel est compté et résolu par politique « côté déjà intégré gagne ». |
| **B** | Un **seul répertoire partagé** ; les 3 agents écrivent **en concurrence**. L'éditeur scripté utilise une **garde de version optimiste** (relecture avant écriture, réessai si un autre agent a écrit) : c'est la détection de conflits. Une **fenêtre de collision déterministe** est injectée dans le pilote (agent-2 lit, attend 200 ms, relit) pour exercer réellement la détection. |
| **C** | Un seul répertoire, agents **strictement séquentiels** (aucune concurrence, aucune détection). Contrôle. |

**Contrainte de la mission : aucune commande Git mutante** (le Lead commite).
La stratégie A utilise donc des copies de répertoire et une fusion en mémoire,
et **non** `git worktree`/`git merge` comme dans MVP-0b. Cette différence est
assumée, documentée, et explique une partie des écarts avec MVP-0b.

## 3. Fixture et corpus (pré-enregistrés)

`benchmarks/s8-mvp0c/fixture/` : crate Rust autonome (déclare `[workspace]`
vide, jamais membre du workspace Ravum), **3 modules** (`stats`, `text`,
`dates`), **12 tests** dans `tests/behavior.rs`. Les implémentations livrées
sont **incomplètes** : les tests échouent avant les éditions et passent après
(vérifié par le harnais, qui échoue si la fixture n'est pas rouge au départ).

| Tâche | Agent | Fichier | Fonction | Tests concernés |
|---|---|---|---|---|
| `t1` | agent-1 | `src/stats.rs` | `stats::mean` | 2 |
| `t2` | agent-1 | `src/text.rs` | `text::slugify` | 3 |
| `t3` | agent-2 | `src/stats.rs` | `stats::median` | 2 |
| `t4` | agent-3 | `src/dates.rs` | `dates::parse_iso_date` | 3 |

`t1` et `t3` partagent **le même fichier** mais des fonctions différentes :
c'est le cas qui rend la concurrence de B non triviale et la fusion de A
nécessaire. Les fichiers `Cargo.toml`, `.gitignore`, `src/lib.rs` et
`tests/behavior.rs` sont **protégés** (aucun agent ne doit les modifier).

## 4. Métriques (définitions pré-enregistrées)

Timings en **millisecondes wall-clock** (`performance.now()` / epoch ms).

- **temps total jusqu'à tests verts** : départ des agents → fin de `cargo test`
  vert.
- **phase d'édition** : A/B = durée de la phase concurrente des 3 agents ;
  C = somme des éditions séquentielles.
- **phase d'intégration** : A = fusion 3-way + écriture de la copie
  d'intégration ; B/C = 0.
- **attente des agents** : A = fin d'édition → intégration terminée (par agent) ;
  B = temps bloqué par une collision détectée (relecture + réessai) ;
  C = début de campagne → début de l'agent (attente des prédécesseurs).
- **conflits** : A = hunks en conflit ; B = collisions détectées par la garde de
  version ; C = 0 (aucune détection).
- **temps de résolution** : A = durée de fusion mesurée ; B = durée bloquée en
  réessai ; C = 0.
- **itérations** : nombre total de tentatives d'écriture (réessais inclus).
- **builds** : invocations `cargo` par répétition (1 test final) ; la
  vérification rouge initiale est comptée séparément au niveau campagne.
- **changements perdus** : tâches dont le marqueur attendu est absent du
  résultat final **et** écart au contenu de référence exact ; doit être 0.
- **RAM / CPU / disque** : échantillonneur de processus (WorkingSet, temps CPU,
  taille `target/` et répertoire de réplique).
- **tokens par agent** : *uniquement* en campagne réelle, via
  `tools/agent-usage/usage.mjs` (compteurs enregistrés par le harnais DSH) et
  attribution par marqueur de session ; **jamais estimés**.
- **temps de supervision humaine** : mesuré par le Lead en campagne réelle
  (horodatages), non mesuré au pilote.

## 5. Protocole d'exécution du pilote

- **N = 3 répétitions** par stratégie (minimum exigé), **ordre alterné** :
  `A1 B1 C1 · C2 B2 A2 · A3 B3 C3`.
- Chaque répétition utilise des **copies éphémères** dans le dossier temporaire
  (`%TEMP%/s8-mvp0c-pilot-*`), initialisées depuis la fixture versionnée ;
  **cache `target/` froid identique** pour A/B/C.
- Variables : `CARGO_BUILD_JOBS=2`, `CARGO_NET_OFFLINE=true`, pas de réseau.
- **Bornes** : pilote complet borné à ~15 minutes ; aucun processus résiduel
  (échantillonneur arrêté, agents et cargo attendus, répertoires supprimés sauf
  `--keep`) ; une preuve JSON est écrite même en cas d'échec de protocole ?
  **Non** : en cas d'exception harnais, la preuve n'est pas écrite (échec
  explicite) — seules les campagnes validées produisent la preuve.
- **Critères de succès pré-enregistrés (par répétition)** :
  1. `cargo test` vert après éditions (12/12) ;
  2. contenu final **exactement égal** à la référence calculée hors ligne ;
  3. **0 changement perdu**, **0 fichier protégé modifié**, **0 reçu manquant** ;
  4. fixture **rouge avant** éditions (vérifiée au niveau campagne) ;
  5. en B, la collision injectée est **réellement détectée** (conflits ≥ 1) ;
  6. contrôle positif : un écrasement d'écriture simulé est **détecté** par le
     détecteur du harnais.
- Le harnais échoue (code 1) si un critère n'est pas satisfait.

## 6. Limites pré-enregistrées

1. Agents **scriptés déterministes** : aucune mesure de tokens, de qualité de
   code généré ou de supervision humaine ; les tâches sont des remplacements
   d'extraits figés.
2. Stratégie A par **copies + fusion en mémoire**, pas worktrees Git (contrainte
   « aucune commande git mutante ») ; différence assumée avec MVP-0b.
3. Collaboration de B **injectée** : fenêtre de collision déterministe ; la
   campagne réelle ne l'injecte pas et peut observer 0 conflit.
4. Échantillon restreint : 3 répétitions, 4 tâches, fixture locale.
5. Machine partagée : charge mesurée/documentée, non contrôlée.
6. Compilation froide par répétition : le coût fixe de `cargo test` (compilation
   du crate + binaires de test) amortit les différences d'organisation.
7. Le pilote ne mesure **ni tokens ni supervision humaine** ; il ne peut pas
   servir à extrapoler le coût d'une campagne IA.

## 7. Résultats du pilote (exécutés réellement)

### 7.1 Exécution

| Élément | Valeur mesurée |
|---|---|
| Date | **2026-11-09**, run de 49,6 s wall-clock |
| Commande | `node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3` |
| Campagnes | **9** (3 répétitions × 3 stratégies), ordre alterné `A1 B1 C1 · C2 B2 A2 · A3 B3 C3` |
| Code de sortie | **0** (`Résultat pilote : OK`) |
| Preuve brute | `evidence/mvp0c-s8-pilot-2026-11-09.json` |
| Rapport généré | `benchmarks/s8-mvp0c/results/latest.md` |
| Pré-enregistrement | `sha256` de ce document au démarrage du run = `04252aa26caa7e5331a0e795d1b9194eae802d4371012e74ea846b4bcbfc6886` ; il correspond exactement à la version **sans la section 7** (vérifié : le hash du fichier avant ajout des résultats est identique à celui enregistré par le harnais) |
| Environnement | Windows, Node v26.13.0, cargo 1.95.0, rustc 1.95.0 ; **16 cœurs logiques**, Intel i7-13620H, 15,7 Go RAM |
| Charge machine (sondes Node) | RAM libre 2 845 → 2 779 → 2 921 Mo ; CPU 29,1 % → 33,0 % → 43,4 % (début/milieu/fin) |
| Charge par répétition | CPU machine 69,1 % [59,8–95,2] médiane ; RAM libre minimale ≈ 2,57–2,61 Go ; machine **partagée** avec d'autres agents |

> **Note de reproductibilité (Lead, 2026-11-09).** La preuve JSON versionnée correspond à une seconde exécution de vérification (répétitions indépendantes) ; le tableau ci-dessus rapporte l'exécution initiale du worker. Classement et invariants identiques (A < C < B, 12/12 tests, 0 perte) ; les écarts de temps entre les deux exécutions sont de 5–10 % sur machine partagée. Les deux exécutions sont des mesures réelles, aucune moyenne inventée.

### 7.2 Fixture : vérifications préalables

- Tests **rouges avant éditions** : **7 échecs / 5 succès** sur 12 (noms des échecs
  dans la preuve : `mean_of_simple_values`, `mean_of_negative_values`,
  `median_of_odd_list`, `median_of_even_list`, `slugify_basic_title`,
  `slugify_collapses_separators_and_trims`, `parse_iso_date_accepts_valid_date`) ;
  le harnais aurait échoué si la fixture avait été verte.
- **Contrôle positif de détection de conflit** : vrai (`writer-1` écrasé par
  `writer-2` détecté), en 2,5 ms. Sans ce contrôle, la détection de B ne serait
  pas crédible.
- 9/9 répétitions : `cargo test` **12/12 vert**, contenu final **exactement égal
  à la référence**, **0 changement perdu**, **0 fichier protégé modifié**,
  **0 reçu manquant**.

### 7.3 Tableau comparatif (médiane [min–max] (IQR), n = 3)

| Métrique | **A — copies + intégration** | **B — répertoire partagé** | **C — séquentiel** |
|---|---|---|---|
| **Temps total jusqu’à tests verts** | **2 650,4** [2 563,8–2 920,1] (IQR 178,1) | **3 061,5** [2 846,6–3 083,8] (IQR 118,6) | **2 914,3** [2 849,5–3 144,0] (IQR 147,3) |
| Phase d’édition des agents | 219,1 [203,6–244,1] | 483,2 [468,5–486,7] | 410,9 [396,8–464,8] |
| Phase d’intégration | 24,0 [16,9–24,4] | 0 | 0 |
| `cargo test` final | 2 384,1 [2 320,2–2 629,5] | 2 563,8 [2 366,1–2 582,5] | 2 496,0 [2 442,8–2 671,7] |
| **Attente cumulée des agents** | 188,6 [145,7–189,3] | 306,9 [306,8–321,7] | **448,3** [394,2–455,6] |
| Conflits détectés | 0 | **1** par répétition | 0 |
| Temps de résolution des conflits | 0,4 (bruit de mesure) | 306,9 [306,8–321,7] | 0 |
| Fusions / intégrations | 4 | 0 | 0 |
| Écritures / tentatives | 8 | 5 | 4 |
| Itérations d’agents (tentatives) | 4 | 5 | 4 |
| Compilations `cargo` | 1 | 1 | 1 |
| Tests verts | 12/12 | 12/12 | 12/12 |
| **Changements perdus** | **0** | **0** | **0** |
| RAM agents (RSS auto-déclaré) | 50,7 [49,5–51,7] Mo | 50,3 [50,2–52,4] Mo | 49,7 [49,6–49,7] Mo |
| RAM pic des processus suivis (WorkingSet) | 10,8 Mo (cargo seul capturé) | 149,0 [97,5–156,4] Mo | 10,7 Mo (cargo seul capturé) |
| CPU des processus suivis | 0,06 s | 0,36 s | 0,05 s |
| Charge CPU machine pendant la répétition | 77,2 % [59,8–95,2] | 69,1 % [64,8–78,3] | 70,6 % [67,9–94,4] |
| RAM machine libre minimale | 2 568 [2 460–2 672] Mo | 2 600 [2 598–2 702] Mo | 2 613 [2 552–2 755] Mo |
| Chute RAM machine pendant la répétition | 138 [86–254] Mo | 182 [68–253] Mo | 147 [101–210] Mo |
| Cache `target/` de la réplique | 11,3 Mo | 11,3 Mo | 11,3 Mo |
| Disque de la réplique | 11,31 Mo | 11,30 Mo | 11,30 Mo |
| Réussite fonctionnelle | 3/3 | 3/3 | 3/3 |

Remarque de mesure : la RAM/CPU « processus suivis » ne couvre que les
**processus directs** lancés par le harnais (agents Node, cargo). Les `rustc` et
binaires de test ne sont **pas isolables** : WMI/CIM est tombé en panne sur la
machine pendant la mission (voir §7.6). La RAM des agents est donc mesurée par
leur **RSS auto-déclaré** (~50 Mo par agent Node, stable), et la charge machine
globale par `os.cpus()`/`os.freemem()`.

### 7.4 Échantillons bruts (9 répétitions)

| rep | ordre | total_ms | edit_ms | integr_ms | test_ms | attente_ms | conflits | résol_ms | itérations | tests | perdus | RAM_suivie_Mo | CPU_s |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A1 | 1 | 2563,8 | 203,6 | 16,9 | 2320,2 | 145,7 | 0 | 0,9 | 4 | 12/12 | 0 | 10,8 | 0,016 |
| B1 | 2 | 2846,6 | 468,5 | 0 | 2366,1 | 306,8 | 1 | 306,8 | 5 | 12/12 | 0 | 149,0 | 0,391 |
| C1 | 3 | 2914,3 | 410,9 | 0 | 2496,0 | 448,3 | 0 | 0 | 4 | 12/12 | 0 | 10,6 | 0,047 |
| C2 | 4 | 3144,0 | 464,8 | 0 | 2671,7 | 455,6 | 0 | 0 | 4 | 12/12 | 0 | 10,9 | 0,062 |
| B2 | 5 | 3061,5 | 483,2 | 0 | 2563,8 | 321,7 | 1 | 321,7 | 5 | 12/12 | 0 | 97,5 | 0,250 |
| A2 | 6 | 2920,1 | 244,1 | 24,0 | 2629,5 | 189,3 | 0 | 0,4 | 4 | 12/12 | 0 | 10,8 | 0,188 |
| A3 | 7 | 2650,4 | 219,1 | 24,4 | 2384,1 | 188,6 | 0 | 0,4 | 4 | 12/12 | 0 | 10,8 | 0,062 |
| B3 | 8 | 3083,8 | 486,7 | 0 | 2582,5 | 306,8 | 1 | 306,8 | 5 | 12/12 | 0 | 156,4 | 0,359 |
| C3 | 9 | 2849,5 | 396,8 | 0 | 2442,8 | 394,2 | 0 | 0 | 4 | 12/12 | 0 | 10,7 | 0,031 |

Détail complet (métriques par agent, reçus, sorties cargo, commandes) dans la
preuve JSON.

### 7.5 Tokens et supervision humaine au pilote

- **Tokens : `measured: false`.** Les agents du pilote sont des scripts
  déterministes : aucun LLM n'est appelé, aucune session DSH n'est produite.
  La preuve déclare explicitement `tokens.measured = false` et
  `per_agent = null`. **Aucun token n'est inventé.**
- **Supervision humaine : `measured: false`** au pilote (aucune interaction
  humaine). La campagne réelle la mesurera (§8.6).

### 7.6 Verdict

**Ce que le pilote montre (mesuré, n = 3, fixture locale) :**

1. **B est le plus lent** des trois (médiane 3 061,5 ms) : sa fenêtre de
   collision injectée coûte ~200 ms d'édition + réessai, et sa phase d'édition
   est plus longue que A (483,2 vs 219,1 ms). La détection fonctionne
   (**1 conflit détecté par répétition**, résolu par relecture + réessai en
   ~307 ms, 0 perte).
2. **A est le plus rapide** (médiane 2 650,4 ms), devant C (2 914,3 ms) : gain
   médian ~264 ms (~9 %) vs C. Ce gain vient de l'édition parallèle
   (219 ms vs 411 ms) et d'une intégration très bon marché (24 ms) ; C paie la
   sérialisation (attente cumulée 448 ms).
3. **Les écarts A/C sont petits et se chevauchent partiellement** (A max
   2 920 ms > C min 2 849 ms) : avec n = 3 et une machine partagée dont la
   charge a varié de 60 à 95 %, **on ne peut pas conclure à une supériorité
   générale de A**. La compilation `cargo test` représente 84–90 % du temps
   total et masque les différences d'organisation.
4. **Les trois stratégies préservent les 4 éditions** (0 perdu, contenu exact,
   12/12 tests) : la garde de version de B et la fusion de A sont correctes sur
   ce corpus.

**Comparaison honnête avec MVP-0b** (mêmes principes, corpus et mécanismes
différents — les chiffres ne sont pas directement comparables) :

| Mesure | MVP-0b (scripté) | MVP-0c pilote (scripté) | Explication de l'écart |
|---|---|---|---|
| Corpus | 6 tâches, 4 modules, ~10 tests, 5 reps | 4 tâches, 3 modules, 12 tests, 3 reps | moins de tâches ⇒ éditions plus courtes |
| A | 5 660,6 ms | 2 650,4 ms | MVP-0b A = **vrais worktrees Git + 3 merges** (intégration 1 243 ms) ; MVP-0c A = copies + fusion mémoire (24 ms) ; contrainte « aucune commande git mutante » |
| B | 6 677,6 ms | 3 061,5 ms | MVP-0b B = coordinateur (snapshot complet + journal durable + publication GitWriter transactionnelle, 2 468 ms) ; MVP-0c B = écritures directes + garde optimiste |
| C | 4 785,6 ms | 2 914,3 ms | 4 éditions scriptées vs 6 éditions + commits Git |
| Classement | C < A < B | A < C < B | **B reste le plus lent dans les deux** ; A/C s'inversent parce que A ne paie plus Git |
| Test final | 2 702–3 285 ms | 2 384–2 564 ms | même ordre de grandeur (coût fixe dominant) |
| Part du build dans le total | 40–58 % | **84–90 %** | en pilote, l'organisation pèse moins que dans MVP-0b |
| Tokens | non mesurés | non mesurés | **première mesure réelle possible en MVP-0c** (§8) |
| Supervision humaine | non mesurée | non mesurée | mesurée par le Lead en campagne réelle |

**Conclusion honnête :** le pilote **confirme** MVP-0b sur un point robuste — une
coordination de type B (répertoire partagé + détection) n'est **pas plus rapide**
que les alternatives, et son coût de détection est mesurable. Il **infirme** la
généralité du classement C < A de MVP-0b : sans coût Git dans A, le classement
s'inverse sur ce corpus, et les écarts restent dans le bruit d'une machine
chargée. Le pilote **ne démontre pas** de supériorité générale d'une stratégie ;
il ne peut rien dire des tokens ni du temps de supervision, qui sont l'objet de
la campagne réelle. Toute extrapolation de tokens depuis le pilote serait
inventée et est refusée.

### 7.7 Limites constatées (post-exécution)

Les limites pré-enregistrées (§6) s'appliquent. Constats supplémentaires :

- **WMI/CIM indisponible** pendant la mission (`Get-CimInstance Win32_*` :
  erreur générale / blocage). Le harnais a été rendu indépendant de WMI :
  charge machine via `os.cpus()`/`os.freemem()`, processus **directs** suivis
  par `Get-Process -Id`, RSS des agents auto-déclaré. Les `rustc` ne sont pas
  isolés : la RAM/CPU « processus suivis » **sous-estime** la compilation.
- Trois processus PowerShell orphelins issus des **tentatives interrompues**
  par la panne WMI ont été identifiés et arrêtés ; le cycle de vie de
  l'échantillonneur a été re-testé ensuite (arrêt propre vérifié, aucun
  processus résiduel pour le run officiel).
- Machine partagée : d'autres agents compilaient pendant le run (charge
  machine 60–95 %) ; l'ordre alterné répartit la dérive mais n=3 reste petit.
- Le cache `target/` est froid à chaque répétition (11,3 Mo) : même coût pour
  A/B/C, ce qui amortit les écarts.


## 8. Procédure exacte de la campagne réelle (agents IA)

> Cette section est le mode d'emploi que le Lead doit suivre. Elle ne contient
> aucun chiffre de tokens : les tokens ne sont connus qu'après collecte.

### 8.1 Prérequis

- Fixture versionnée : `benchmarks/s8-mvp0c/fixture/` (rouge avant éditions).
- Harnais : `benchmarks/s8-mvp0c/run.mjs` (Node 26, zéro dépendance npm).
- Outil d'usage : `tools/agent-usage/usage.mjs` (lecture locale des sessions
  DSH, jamais d'appel API).
- Les subagents sont des **teammates DSH** : le Lead les spawne avec
  `spawn_teammate`, chacun dans **sa propre session** (le harnais DSH enregistre
  `data.usage` par message assistant).
- L'agent-command du harnais est **pluggable** : le pilote utilise
  `lib/agent-scripted.mjs`, la campagne réelle utilise les subagents DSH via le
  mode `live` (et peut aussi, pour un autre exécutant, passer
  `--agent-command "<commande {agent} {workspace} {tasks} {strategy} {base} {receipt}>"`).
- **Aucune** commande git mutante n'est exécutée par le harnais ; le Lead
  commite les livrables.

### 8.2 Préparer une campagne (une stratégie × une répétition)

```powershell
node benchmarks/s8-mvp0c/run.mjs --mode live --phase prepare `
  --scenario A --rep 1 --campaign "$env:TEMP\s8-mvp0c-live\A1"
```

`prepare` crée : `base/` (copie de la fixture, tests rouges vérifiés),
`agents/agent-N/` (A uniquement) ou `shared/` (B et C), `receipts/*.base.json`
(hashes de base autorisés), `prompts/agent-N.md` (**prompt exact**), et
`plan.json` (marqueurs, chemins, `started_at_epoch_ms`).

### 8.3 Prompt identique par agent (généré, à transmettre tel quel)

Le prompt généré contient, dans cet ordre :

1. identité : `Tu es S8MVP0C-AGENT-<n> (agent agent-<n>) de la campagne S8
   MVP-0c (scénario <A|B|C>, répétition <r>).` — **le marqueur
   `S8MVP0C-AGENT-<n>` est la clé d'attribution des tokens** ;
2. répertoire de travail absolu et note de stratégie (copie privée en A,
   répertoire partagé avec relecture en B, séquentiel en C) ;
3. les tâches assignées et **seulement** celles-là ;
4. contraintes : ne pas modifier les fichiers protégés, pas de git, pas de
   réseau, **ne pas lancer `cargo`** (le harnais compile, pour borner le coût) ;
5. commande exacte d'écriture du reçu :
   `node <repo>/benchmarks/s8-mvp0c/lib/write-receipt.mjs --agent agent-<n>
   --workspace <dir> --base <dir>/receipts/agent-<n>.base.json
   --receipt <dir>/receipts/agent-<n>.json`
6. réponse finale attendue : `DONE S8MVP0C-AGENT-<n>`.

Le Lead spawne **3 subagents en parallèle** (A et B) ou **1 par 1** (C) :

```
spawn_teammate(
  name: "s8mvp0c-agent-1",
  description: "S8MVP0C-AGENT-1 — campagne S8 MVP-0c <scénario><rep>",
  prompt: "<contenu exact de prompts/agent-1.md>"
)
```

La `description` doit contenir le marqueur : elle devient
`subagent/descriptor.data.label` dans la session et sert d'attribution primaire.
Le premier message utilisateur (le prompt) sert de repli.

### 8.4 Collecter et mesurer

```powershell
# après les 3 réponses DONE
node benchmarks/s8-mvp0c/run.mjs --mode live --phase collect `
  --campaign "$env:TEMP\s8-mvp0c-live\A1" `
  --done-json "$env:TEMP\s8-mvp0c-live\A1\done.json" `
  --usage auto `
  --out "evidence/mvp0c-s8-live-A1-2026-11-09.json"
```

`--done-json` (facultatif mais recommandé pour les attentes) :
`[{"agent":"agent-1","started_epoch_ms":…,"done_epoch_ms":…}, …]` — horodatages
relevés par le Lead au moment de chaque spawn et de chaque réponse `DONE`.

`collect` vérifie : reçus présents/complets, fichiers protégés intacts, écritures
non perdues, `cargo test` vert, puis intègre A si nécessaire. Il écrit la preuve
JSON et sort en code 1 si un critère échoue (y compris tokens non attribués).

### 8.5 Attribution session DSH → agent

1. **Marqueur** : `S8MVP0C-AGENT-<n>` dans la `description` de spawn (et dans le
   prompt). Le harnais lit uniquement l'en-tête de session : événement `session`
   (`delegationDepth`, `origin`, `parentSession`, `createdAt`) et
   `subagent/descriptor.data.label`; **aucun contenu de message n'est conservé**.
2. **Filtre par fenêtre** : `createdAt` compris entre
   `plan.started_at_epoch_ms` et la fin de la collecte, tolérance ±120 s.
   Les sessions racine (Lead) sont exclues (`delegationDepth = 0`).
3. **Appariement** : le libellé du descripteur d'abord, le premier message
   utilisateur en repli. Une session de sous-agent **non attribuée n'est jamais
   répartie au hasard** : elle est listée dans `unmatched_sessions` et la
   campagne est marquée incomplète (`all_agents_matched = false`).
4. **Tokens** : `node tools/agent-usage/usage.mjs --json --workspace D:\App\Ravum`
   fournit par session `inputTokens`, `outputTokens`, `totalTokens`,
   `cacheReadTokens`, `processedTokens`, `assistantMessages`, `wallMs`. Le
   harnais additionne ces champs **tels quels** par agent et conserve la liste
   des sessions attribuées.
   - `processedTokens = inputTokens + outputTokens + cacheReadTokens` (formule
     de `usage.mjs`) ;
   - `cacheReadTokens` est déclaré par l'outil comme sous-ensemble
     d'`inputTokens` ; ne pas réinterpréter ;
   - `wallMs` couvre le premier au dernier événement (inactivité incluse) ;
   - **aucun coût monétaire** n'est exposé par le harnais : ne jamais en
     inventer.
5. **Tokens par tâche** : le harnais DSH ne ventile pas par tâche. Ce qui est
   mesuré est **par agent et par campagne**. Le nombre d'itérations d'un agent
   est approximé par `assistantMessages` de sa session (et `attempts` du reçu).

### 8.6 Temps de supervision humaine (mesuré par le Lead)

Le Lead note avant de spawner : `t_start` (avant le premier `spawn_teammate`) et
`t_end` (après le dernier `DONE`), puis `supervision_ms = t_end - t_start`.
Il peut utiliser `done.json` pour les horodatages par agent. Ce temps inclut les
tours LLM et les vérifications ; il est reporté **tel que mesuré**, sans
estimation.

### 8.7 Plan de campagne borné

- **9 campagnes minimum** : 3 répétitions × 3 stratégies, **ordre alterné**
  `A1 B1 C1 · C2 B2 A2 · A3 B3 C3`.
- Chaque campagne = 3 subagents ⇒ **27 sessions de subagents** au total.
- **Borne de coût** : aucun prix monétaire n'est disponible ; la borne est
  exprimée **en tokens traités et en durée**, fixée par le Lead *avant* la
  campagne (par exemple : budget `processedTokens` total maximal et durée
  maximale). **Critères d'arrêt** : budget dépassé, session de sous-agent non
  attribuable, `cargo test` rouge, fichier protégé modifié, reçu manquant, ou
  plus de 3 itérations de correction par agent.
- Ne pas lancer plus d'une campagne à la fois (machine partagée) ; consigner la
  charge (`probeLoad`) avant/après chaque campagne.

### 8.8 Agrégation et verdict

```powershell
node benchmarks/s8-mvp0c/run.mjs --mode live --phase aggregate `
  --campaigns "$env:TEMP\s8-mvp0c-live\A1,$env:TEMP\s8-mvp0c-live\B1,…" `
  --out "evidence/mvp0c-s8-live-summary-2026-11-09.json"
```

Le Lead ne peut déclarer un verdict que si : **tous les `cargo test` sont verts**,
**aucun changement perdu**, **fichiers protégés intacts**, **les 3 agents de
chaque campagne sont attribués à une session** (tokens non nuls et non
« unmatched »), **la charge machine est consignée**, et **l'ordre alterné est
respecté**. Un chiffre de tokens sans session attribuée doit être traité comme
`UNKNOWN`, jamais comme une mesure.

## 9. Fichiers et commandes

- `benchmarks/s8-mvp0c/fixture/**` — fixture Rust réelle (tests rouges → verts).
- `benchmarks/s8-mvp0c/lib/tasks.mjs` — corpus pré-enregistré.
- `benchmarks/s8-mvp0c/lib/agent-scripted.mjs` — agent-command du pilote
  (éditeur scripté déterministe, garde de version optimiste en B).
- `benchmarks/s8-mvp0c/lib/write-receipt.mjs` — reçu d'agent (utilisé par les
  subagents réels).
- `benchmarks/s8-mvp0c/run.mjs` — harnais (`pilot`, `live prepare/collect/aggregate`).
- `benchmarks/s8-mvp0c/results/latest.md` — résultats bruts générés.
- `scripts/s8-mvp0c.ps1` — lanceur reproductible (propage le code de sortie).
- Preuve pilote : `evidence/mvp0c-s8-pilot-2026-11-09.json`.

```powershell
# pilote (agents scriptés)
pwsh -File scripts/s8-mvp0c.ps1 -Reps 3
# équivalent direct
node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3
```

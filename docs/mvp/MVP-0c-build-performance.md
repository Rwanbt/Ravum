# MVP-0c L2 — performance des modifications réelles : opportunité du groupement de patches

Statut : **MESURÉ** (campagne réelle du 2026-11-09) — pas `PROVED`, pas une
release. Ce document mesure l'opportunité du groupement **avant** toute
implémentation dans le cœur du daemon ; la politique de groupement est simulée
par le harnais (composition de snapshots), le daemon réel n'est pas modifié.

| Élément | Valeur |
|---|---|
| Branche | `feat/mvp0c-hardening`, HEAD au moment des mesures : `11b5e9e` (l'arbre de travail est partagé avec les autres workers : 27 fichiers modifiés/non suivis, ce n'est pas l'identité de la campagne) |
| `rustc` | 1.95.0 (`59807616e`, 2026-04-14), host `x86_64-pc-windows-msvc`, LLVM 22.1.2 |
| Machine | Windows 11 26100, Intel64 Family 6 Model 186, 16 processeurs logiques, 16 868 962 304 o de RAM (~15,7 Go) |
| Preuve JSON | [`evidence/mvp0c-grouping-2026-11-09.json`](../../evidence/mvp0c-grouping-2026-11-09.json) |
| Harnais | `ravum-daemon-bench --suite grouping` (module `grouping.rs` du bench) |
| Fixture | `fixtures/grouping` (479 lignes, 1 fichier, 6 éditions compatibles + 1 incompatible) |

## 1. Question et définition de « build »

Question mesurée : **N éditions compatibles doivent-elles être analysées une par
une (N cycles complets) ou regroupées dans un snapshot cohérent (1 analyse)**,
et quelle fenêtre d'attente de collecte peut-on s'offrir avant que le gain
disparaisse ?

Le daemon MVP-0b/0c est un **moteur d'analyse** : `rustc --emit=metadata`, sans
codegen, sans lien, sans activation. Dans cette campagne, « build » signifie
donc **compilation de métadonnées** (« analyse ») et le nombre de « builds
économisés » est le nombre d'invocations `rustc` évitées. La comparaison
build+link complète relève des campagnes MVP-0b (§5 de `MVP-0b-cycle.md`) et
n'est pas rejouée ici : le daemon ne couvre pas cette étape.

## 2. Protocole (gelé avant exécution)

- **Fixture** `fixtures/grouping` : un module bibliothèque réaliste de
  **479 lignes** (fonctions pures indépendantes, type `Rectangle`, tests
  unitaires), compilé par `rustc --emit=metadata`.
  - 6 éditions **compatibles** (`edit-01`…`edit-06`) qui remplacent le corps
    d'une fonction distincte chacune (addition, multiplication, test de parité,
    factorielle, somme de contrôle, interpolation) : aucun chevauchement de hunk,
    le contenu reste compilable à chaque étape ;
  - 1 édition **incompatible** (`edit-07`) : signature de `scale` changée en
    `scale(x, factor)` alors que `compose` continue de l'appeler avec un
    argument — `rustc` doit produire `E0061`.
  - Contrats vérifiés avant la campagne : chaque `find` est unique dans la base
    (erreur sinon), les 6 étapes cumulatives compilent réellement, le contenu
    avec l'édition incompatible échoue réellement avec `E0061`,
    `cargo check --manifest-path fixtures/grouping/Cargo.toml` passe.
- **6 modes** mesurés, dans un ordre qui tourne d'un cran à chaque répétition :
  1. `sequential` — 6 cycles complets : édition i appliquée cumulativement →
     analyse daemon → vérification ;
  2. `grouped-w000` / `-w025` / `-w050` / `-w100` — fenêtre d'attente simulée
     (`sleep` 0/25/50/100 ms), puis les 6 éditions composées → **1** analyse →
     vérification des 6 postconditions ;
  3. `incompatible` — lot complet + édition incompatible → analyse **refusée**,
     puis reprise individuelle (chaque édition seule sur la base) pour isoler la
     faute, puis re-lot des seules éditions compatibles → accepté.
- **État neuf par mode et par répétition** : chaque mode ouvre un daemon avec un
  répertoire d'état vide (`snapshots`, cache disque, incrémental). Aucun verdict
  ne peut être servi par le cache d'une autre variante ; le harnais vérifie que
  `rustc_runs == analyses` (aucun hit possible).
- **1 répétition de chauffe non enregistrée**, puis **5 répétitions enregistrées**
  (minimum du mandat).
- **Phases mesurées** autour des appels réels : `wait` (fenêtre simulée),
  `compose` (application des éditions aux octets, `find` unique vérifié),
  `analyze` (`Daemon::analyze` de bout en bout), `verify` (postconditions de
  contenu + identité recalculée), `total` (fenêtre du mode).
- **Vérification** : (a) chaque marqueur d'édition appliquée est présent dans les
  octets soumis ; (b) l'identité de contenu recalculée par le harnais
  (`request.identity`) est **égale** au `snapshot_id` rapporté par le daemon ;
  (c) le rapport est `fresh`. Ce n'est pas une preuve sémantique.
- **Incompatible** : le lot doit être refusé, l'édition fautive identifiée par
  son code de diagnostic, le re-lot accepté avec le même contenu final que le
  mode groupé (« aucun résultat incorrect accepté »).
- **Charge machine** documentée avant/après chaque répétition (CPU 11,7–49 %,
  mémoire disponible 1,0–1,9 Go, 323–326 processus) et par calibration
  `rustc -vV` en début et fin de campagne. Machine partagée avec les autres
  workers : charge non contrôlée, mesures mono-exécution.
- **Commandes** :

```powershell
cargo build -p ravum-daemon --bin ravum-daemon-bench
target\debug\ravum-daemon-bench.exe --suite grouping `
  --work-dir target\ravum-daemon-bench `
  --out-grouping evidence/mvp0c-grouping-2026-11-09.json `
  --grouping-reps 5
```

## 3. Résultats mesurés

Campagne : **29,0 s** de mesures (hors compilation du harnais), 1 répétition de
chauffe non enregistrée + 5 répétitions enregistrées.

### 3.1 Temps total par mode (5 répétitions, ordre alterné)

| Mode | total p50 [min–max] | analyses | `rustc_runs` | analyse p50 | attente p50 | vérif. p50 |
|---|---:|---:|---:|---:|---:|---:|
| `sequential` (6 cycles) | **1159 ms** [991–1745] | 6 | 6 | 1151 ms | — | 0 ms |
| `grouped` w=0 ms | **204 ms** [171–234] | 1 | 1 | 203 ms | 0 ms | 0 ms |
| `grouped` w=25 ms | **214 ms** [193–233] | 1 | 1 | 187 ms | 25 ms | 0 ms |
| `grouped` w=50 ms | **240 ms** [221–320] | 1 | 1 | 189 ms | 50 ms | 0 ms |
| `grouped` w=100 ms | **308 ms** [285–339] | 1 | 1 | 206 ms | 100 ms | 0 ms |
| `incompatible` (lot refusé + reprise) | **1597 ms** [1511–2367] | 9 | 9 | 1585 ms | — | 0 ms |

Les phases `compose` et `verify` mesurent < 1 ms (arrondi à 0 dans les p50) :
le coût est dominé par `analyze` (donc par `rustc`). `total` contient l'attente
simulée plus le surcoût de mesure non attribué (`Instant`, allocation, boucle).

### 3.2 Gain apparié par fenêtre (séquentiel − groupé, même répétition)

| Fenêtre | séquentiel p50 | groupé p50 | gain apparié p50 [min–max] | gain relatif p50 | analyses économisées | `rustc` économisés |
|---:|---:|---:|---:|---:|---:|---:|
| 0 ms | 1159 ms | 204 ms | **925 ms** [820–1535] | **83 %** | 5 | 5 |
| 25 ms | 1159 ms | 214 ms | **945 ms** [798–1514] | **82 %** | 5 | 5 |
| 50 ms | 1159 ms | 240 ms | **895 ms** [751–1425] | **80 %** | 5 | 5 |
| 100 ms | 1159 ms | 308 ms | **854 ms** [652–1417] | **74 %** | 5 | 5 |

Lecture : 6 éditions → 1 analyse, soit **5 invocations `rustc` économisées par
lot**. Le gain reste large (74–83 %) même avec une fenêtre de collecte de
100 ms ; la fenêtre coûte approximativement sa durée (w=25 : +10 ms, w=50 :
+36 ms, w=100 : +104 ms par rapport à w=0, bruit de charge inclus).

### 3.3 Point d'équilibre (fenêtre vs gain)

- Fenêtre maximale **mesurée** avec un gain médian positif : **100 ms** (la plus
  grande fenêtre testée ; aucune fenêtre testée n'annule le gain).
- Coût médian d'un cycle d'analyse séquentiel : **191 ms** [163–289].
- Fenêtre d'équilibre **estimée** : `(N−1) × coût_cycle ≈ 5 × 191 ms =
  **955 ms**`. C'est une **ESTIMATION dérivée** des mesures 0–100 ms, pas une
  mesure directe : elle suppose une attente purement latente et un coût
  d'analyse stable ; elle indique seulement l'ordre de grandeur de la fenêtre
  avant que le groupement cesse d'être rentable.

### 3.4 Édition incompatible : refus, latence d'erreur, reprise

| Mesure | p50 [min–max] |
|---|---:|
| Latence du lot refusé (analyse en erreur) | **184 ms** [165–259] (p95 259 ms) |
| Code de diagnostic observé | `E0061` (2 erreurs par analyse) |
| Lot refusé puis reprise individuelle + re-lot | **1596 ms** [1511–2366] (9 analyses) |
| Analyse du re-lot compatible (accepté) | 158 ms [144–215] |
| Incompatible identifié (édition seule) | **5/5** répétitions |
| Aucun résultat incorrect accepté | **oui** (5/5) |
| Contenu final = contenu groupé compatible | **oui** (digest identique) |

La reprise est **volontairement conservatrice et brute** : 1 analyse de lot
refusé + 6 analyses individuelles des compatibles + 1 analyse de l'édition
incompatible + 1 analyse du re-lot = **9 analyses**, soit ~1,4 × le coût du
mode séquentiel de référence (6 analyses). Un lot qui échoue coûte donc plus
cher que ne pas grouper : le groupement n'est rentable que si les lots
échouent rarement, ou si la reprise est plus fine (bissection mesurée,
voir §5).

### 3.5 Ressources et correction

| Ressource (par mode, 5 répétitions) | valeur observée |
|---|---|
| CPU processus harnais (daemon in-process) | groupé : 0–47 ms ; séquentiel : 78–172 ms ; incompatible : 125–249 ms — les `rustc` enfants ne sont pas comptés ici (leur temps est `timings.analysis_ms`) |
| RSS pic du processus (échantillonné toutes les 2 ms) | 7,6–8,4 Mo pour tous les modes (le daemon est in-process ; le pic peut inclure la mémoire résiduelle du mode précédent) |
| Disque de l'état daemon après le mode | groupé : **1 242 962 o** ; séquentiel : **2 513 933 o** ; incompatible : **5 330 062 o** (snapshots immuables + cache + incrémental, répertoires neufs) |
| Mémoire système disponible | 1 056–1 871 Mo |
| Nombre de processus machine | 323–326 |
| Calibration `rustc -vV` (médiane) | début 79 ms, fin 55 ms |
| Charge CPU système | 11,7–49 % pendant les répétitions (45 % au lancement du harnais) |
| Vérifications d'identité | 95 contrôles, **0 échec** |
| Vérifications de marqueurs | 285 contrôles, **0 échec** |
| Digest de contenu identique entre stratégies | **oui** (`74a801600f1c31c9`) |
| Taux de succès correct | **6/6 modes valides**, 5/5 répétitions par mode |

### 3.6 Variabilité brute (tous les totaux par répétition)

| Rép. | Ordre des modes | Totaux mesurés (ms) |
|---:|---|---|
| 1 | sequential, grouped-0, grouped-25, grouped-50, grouped-100, incompatible | 1159 / 234 / 214 / 264 / 287 / 1597 |
| 2 | grouped-0, grouped-25, grouped-50, grouped-100, incompatible, sequential | 172 / 199 / 221 / 308 / 1576 / 1162 |
| 3 | grouped-25, grouped-50, grouped-100, incompatible, sequential, grouped-0 | 233 / 221 / 285 / 1511 / 1116 / 204 |
| 4 | grouped-50, grouped-100, incompatible, sequential, grouped-0, grouped-25 | 320 / 328 / 2112 / 1745 / 210 / 231 |
| 5 | grouped-100, incompatible, sequential, grouped-0, grouped-25, grouped-50 | 339 / 2367 / 991 / 171 / 193 / 240 |

La répétition 4 est un outlier de charge (séquentiel 1745 ms, incompatible
2112 ms) : la machine est partagée, la charge système a varié de 8 % à 49 % pendant la
campagne. Les conclusions portent sur les **compteurs** (analyses, `rustc_runs`,
rejets, identités) et sur des **gains appariés**, pas sur des valeurs absolues.

## 4. Verdict

**Le groupement vaut le coup dans ce périmètre**, avec un résultat net et
reproductible sur les compteurs :

- 6 éditions compatibles : **6 cycles séquentiels (6 analyses, 6 `rustc`)
  remplacés par 1 analyse** — 5 invocations `rustc` et ~925 ms (83 %) de temps
  d'analyse économisés en p50, gain apparié minimum mesuré 820 ms ;
- même avec une fenêtre de collecte de 100 ms, le gain médian reste de
  **854 ms (74 %)** ; la fenêtre mesurée ne renverse jamais le gain, le
  point d'équilibre estimé est de l'ordre de **0,95 s** ;
- le coût de l'état est lui aussi réduit (1 snapshot groupé ≈ 1,2 Mo contre
  ≈ 2,5 Mo pour 6 snapshots séquentiels), au prix d'une perte de granularité
  des diagnostics intermédiaires ;
- **mais** un lot invalide coûte cher : refus en 184 ms puis reprise brute en
  1596 ms (9 analyses) > 1159 ms du séquentiel. Le gain dépend donc de la
  **fréquence des lots dans lesquels une proposition est incompatible**, non
  mesurée multi-agents ici.

## 5. Recommandation de conception (sans implémentation)

1. **Fenêtre par défaut : 50 ms**, bornée (max 100 ms), désactivable
   (`window = 0`) pour le feedback interactif. Justification mesurée : à 50 ms
   le gain médian reste 895 ms (80 %) et le surcoût de latence est ≈ 36 ms par
   rapport à la collecte immédiate ; 100 ms n'apporte rien de mesurable en
   échange.
2. **N max : 8 propositions par lot**, défaut 6. La campagne n'a mesuré que
   N = 6 ; l'économie d'analyses croît en `(N−1) × coût_analyse` mais le risque
   de conflit et le coût de reprise croissent avec N. Ne pas dépasser 8 sans
   mesure supplémentaire (ni 100 ms de fenêtre sans mesure > 100 ms).
3. **Repli obligatoire et conservateur** : si l'analyse du lot échoue,
   (a) ne jamais accepter un résultat partiel ; (b) re-analyser les propositions
   individuellement pour isoler la/les fautive(s) (coût mesuré : N+1 analyses) ;
   (c) re-lot des seules propositions compatibles et vérification des
   postconditions ; (d) journaliser le lot refusé, son code d'erreur et la
   reprise. Un lot ne doit être tenté que si les préconditions textuelles de
   chaque proposition sont validées **avant** la composition (CAS par
   proposition conservé), pour que le refus soit l'exception.
4. **Diagnostics immédiats séparés du lot** : la fenêtre ne doit jamais
   retarder le diagnostic du snapshot courant ; seule la **vérification/build**
   groupé(e) attend la fenêtre (principe MVP-0b §7.2 conservé).
5. **Limiter le périmètre du lot** aux propositions compatibles sur les mêmes
   unités (le graphe L1 doit invalider correctement les dépendants) ; un lot
   mêlant des unités différentes multiplie le coût de reprise et le risque de
   conflit.
6. **Ce qui reste à implémenter** (hors de cette tâche) : file de collecte
   temporisée côté coordinateur/daemon, construction atomique du snapshot de
   lot avec validation de chaque `expected_hash`, politique d'échec/reprise
   ci-dessus, télémétrie (lots tentés/refusés/repris), interactions avec les
   `externs`/`interface_id` de L1, et campagne multi-agents mesurant la
   fréquence réelle des conflits.

## 6. Limites honnêtes

1. **Analyse seule** : `rustc --emit=metadata`, aucun codegen, lien ni
   activation. Les chiffres ne prédisent pas le gain sur un build complet
   (dominé par codegen+link dans MVP-0b) ; ils mesurent la réduction du nombre
   d'analyses.
2. **Fenêtre simulée par `sleep`** : ce n'est pas une vraie collecte
   multi-agents (arrivées concurrentes, annulation, priorité). La latence réelle
   de collecte peut être plus irrégulière que le `sleep` mesuré.
3. **Une seule fixture, un seul crate, N=6** : module mono-fichier de 479 lignes,
   éditions ciblant des corps de fonctions distinctes (pas de hunks
   chevauchants), 5 répétitions. Aucune extrapolation à d'autres tailles, à
   d'autres formes d'édition (signature publique, imports), ni à d'autres N.
4. **Pas de scénario de conflit** : les 6 éditions compatibles ne se disputent
   aucune ligne/symbole ; le cas « deux agents modifient la même fonction » et
   son coût de reprise ne sont pas mesurés ici (COLLAB-1→4 les refuse déjà en
   séquentiel).
5. **Reprise brute** : la reprise individuelle est volontairement naïve
   (N+1 analyses) ; une bissection ou un ordonnancement plus fin pourrait
   réduire ce coût, mais n'est pas mesuré.
6. **Machine partagée** : charge CPU 11–49 %, calibration 55–79 ms, RAM libre
   1,0–1,9 Go, un outlier de charge en répétition 4. Une seule campagne, pas de
   p95/p99 multi-jour ; les gains relatifs appariés sont robustes, les valeurs
   absolues ne le sont pas.
7. **Observabilité partielle** : le CPU processus ne compte pas les `rustc`
   enfants ; le RSS pic est celui du processus harnais (daemon in-process) et
   peut inclure de la mémoire résiduelle des modes précédents ; le disque est
   mesuré après chaque mode sur des répertoires neufs, pas en régime permanent.
8. **Identité/état partagés** : l'arbre de travail est partagé avec les autres
   workers et le HEAD a bougé pendant la campagne (`11b5e9e` enregistré dans la
   preuve, 27 fichiers dirty) ; le binaire du harnais a été reconstruit juste
   avant la campagne.

## 7. Reproduire et vérifier

```powershell
# 1. Fixture autonome (workspace vide déclaré) :
cargo check --manifest-path fixtures/grouping/Cargo.toml

# 2. Compilation du harnais (le cœur du daemon n'est pas modifié) :
cargo build -p ravum-daemon --bin ravum-daemon-bench

# 3. Campagne :
target\debug\ravum-daemon-bench.exe --suite grouping `
  --work-dir target\ravum-daemon-bench `
  --out-grouping evidence/mvp0c-grouping-2026-11-09.json `
  --grouping-reps 5
# lecture : repetitions[] (valeurs brutes), summary.modes, summary.comparison,
# summary.break_even, summary.incompatible, summary.correctness, valid.

# 4. Tests unitaires du harnais (fixture/édition/rotation) :
cargo test -p ravum-daemon --bin ravum-daemon-bench
```

Fichiers de cette livraison (aucun fichier du cœur du daemon modifié) :

- `crates/ravum-daemon/src/bin/ravum-daemon-bench/grouping.rs` (nouveau, mode
  `grouping` : 2 tests unitaires inclus) ;
- `crates/ravum-daemon/src/bin/ravum-daemon-bench/main.rs` (câblage minimal :
  `mod grouping`, drapeaux `--out-grouping`/`--grouping-reps`, suite
  `grouping`) ;
- `fixtures/grouping/{Cargo.toml,src/lib.rs,edits.json}` ;
- `docs/mvp/MVP-0c-build-performance.md` (ce document) ;
- `evidence/mvp0c-grouping-2026-11-09.json` (valeurs brutes d'une exécution
  réelle, `valid: true`).

---

## 8. L2.2 — fréquence des conflits multi-agents et seuil de rentabilité du groupement

Section **ajoutée** après la campagne L2.2 (2026-11-09) ; elle ne modifie aucun
résultat des sections 1–7. Question ouverte de la phase 1 : le groupement gagne
quand les lots sont compatibles, mais un lot invalide coûte 184 ms de refus puis
~1596 ms de reprise — **quelle fréquence de lots conflictuels annule le gain ?**

Preuve : [`evidence/mvp0c-grouping-multiagent-2026-11-09.json`](../../evidence/mvp0c-grouping-multiagent-2026-11-09.json)
(`valid: true`, durée 10,5 s). Harnais : `benchmarks/grouping-multiagent/run.mjs`
(Node 26, zéro dépendance npm, aucune modification de `crates/**`).

### 8.1 Protocole

- **Décisions réelles**, pas inventées : le harnais appelle le binaire
  `ravum-collab` (`base` puis `integrate --verify --json`) ; les propositions
  JSON sont générées depuis la base réelle (`snapshot_id` + hashs obtenus par
  `ravum-collab base`), avec hunks unified-diff exacts ; le coordinateur est en
  mode vérification sémantique `rustc` opt-in, donc les `AUTO_SAFE` reposent sur
  une compilation de métadonnées réelle du snapshot candidat.
- **4 familles de scénarios**, N = 3 à 5 agents, 5 répétitions en ordre alterné
  (l'ordre des scénarios tourne d'un cran par répétition) :
  (a) fichiers distincts (N=3), (b) symboles distincts d'un même fichier (N=5),
  (c) même fonction (N=3), (d) dépendance non déclarée (N=3).
- **Vérifications de contenu** : digest de snapshot recalculé côté harnais avec
  la formule exacte de `ravum-snapshot`, comparé à `snapshot_after` du binaire ;
  chaîne des `accepted_changes` contiguë ; édition acceptée présente dans le
  contenu final, marqueur d'édition refusée absent. **20/20 runs conformes,
  0 changement perdu, décisions identiques sur les 5 répétitions.**
- **Modèle d'économie** : coûts unitaires lus dans la preuve phase 1
  (`summary.break_even.per_cycle_analysis_p50_ms = 191`,
  `summary.incompatible.batch_latency_ms.p50 = 184`,
  `summary.incompatible.recovery_total_ms.p50 = 1596`), fréquence de conflit
  injectée = fréquence mesurée ici. Le modèle est étiqueté **MODÈLE CALIBRÉ** :
  le groupement n'est pas implémenté et n'est donc pas mesuré bout en bout.
- Machine : Windows 11 26100, 16 processeurs logiques, ~15,7 Go RAM, Node
  v26.13.0 ; mémoire libre 2,58 → 2,82 Go ; calibration `rustc -vV` début
  [96, 92, 88] ms, fin [80, 80, 84] ms. Machine partagée avec d'autres workers.
  Provenance Git enregistrée dans la preuve : HEAD `58220f5`, branche
  `feat/mvp0c-hardening`, arbre partagé (fichiers dirty attendus).

### 8.2 Décisions réelles du coordinateur (identiques sur les 5 répétitions)

| Scénario | N | Décisions réelles | Durée du run `ravum-collab` p50 [min–max] |
|---|---:|---|---:|
| `a-distinct-files` | 3 | `AUTO_SAFE`, `AUTO_SAFE`, `AUTO_SAFE` | 559 ms [361–761] |
| `b-distinct-symbols-one-file` | 5 | `AUTO_SAFE` ×5 | 694 ms [551–1073] |
| `c-same-function` | 3 | `AUTO_SAFE`, `CONFLICT`, `CONFLICT` | 233 ms [208–493] |
| `d-undeclared-dependency` | 3 | `AUTO_SAFE`, `MANUAL_REQUIRED`, `AUTO_SAFE` | 383 ms [310–592] |

Motifs réels observés (extraits des décisions JSON) :

- `c-same-function` : « symbol \"shared\" is already modified by
  agent-c1/shared-plus-2 » (conflit de symbole, aucune intégration) ;
- `d-undeclared-dependency` : « undeclared semantic dependency: a changed line
  references symbol \"helper\", whose definition was modified by
  agent-d1/helper » (arbitrage requis, aucun `AUTO_SAFE` sur la proposition) ;
- les `AUTO_SAFE` sont accompagnés de `verification.status = passed` (les
  `accepted_changes` contiennent le record de vérification sémantique).

Les durées ci-dessus mesurent le processus complet (génération de la base +
vérifications `rustc` réelles) et **ne servent pas** de coûts au modèle : le
modèle utilise exclusivement les coûts de la phase 1.

### 8.3 Fréquence de conflits mesurée

- **Lots contenant ≥ 1 proposition non `AUTO_SAFE`** : **50,0 %** (10/20 runs) —
  par construction du corpus, deux familles sur quatre sont conflictuelles.
- **Propositions non `AUTO_SAFE`** : **21,4 %** (9 décisions sur 42).
- Le corpus est **volontairement chargé en conflits** : ce 50 % n'est pas une
  estimation de la fréquence de conflits en production (non mesurée).

### 8.4 MODÈLE d'économie calibré (explicitement étiqueté MODÈLE)

Hypothèses (frozen) : un lot compatible coûte **1 cycle** (191 ms) ; un lot
contenant au moins une proposition non `AUTO_SAFE` est invalide et coûte la
**reprise mesurée** (1596 ms, qui inclut le refus de 184 ms + N+3 analyses à
N=6) ; la fenêtre de collecte est une latence pure ajoutée. Le coût `p` est la
probabilité qu'un lot contienne un conflit.

```text
E_séquentiel(N)       = N × 191 ms
E_groupé(p, N, w)     = w + (1−p) × 191 + p × 1596      (ms)
p*(N, w)              = ((N−1) × 191 − w) / (1596 − 191)   ← seuil de bascule
E_analyses_groupé(p,N)= (1−p) × 1 + p × (N+3)
```

**Seuil de bascule `p*`** (part maximale de lots conflictuels pour laquelle le
groupement reste rentable) :

| N | fenêtre 0 ms | fenêtre 25 ms | fenêtre 50 ms | fenêtre 100 ms |
|---:|---:|---:|---:|---:|
| 3 | 27,2 % | 25,4 % | 23,6 % | 20,1 % |
| 4 | 40,8 % | 39,0 % | 37,2 % | 33,7 % |
| 5 | 54,4 % | 52,6 % | 50,8 % | 47,3 % |
| 6 | 68,0 % | 66,2 % | 64,4 % | 60,9 % |

**Économie par scénario** (p mesuré = 0 ou 1 ; fenêtre 50 ms entre parenthèses) :

| Scénario | N | p mesuré | séquentiel | groupé attendu | gain | analyses attendues (séq → groupé) | rentable ? |
|---|---:|---:|---:|---:|---:|---:|---|
| `a-distinct-files` | 3 | 0 | 573 ms | 191 ms (241) | +382 ms (+332), +66,7 % | 3 → 1 | **oui** |
| `b-distinct-symbols-one-file` | 5 | 0 | 955 ms | 191 ms (241) | +764 ms (+714), +80,0 % | 5 → 1 | **oui** |
| `c-same-function` | 3 | 1 | 573 ms | 1596 ms (1646) | −1023 ms (−1073), −178,5 % | 3 → 6 | **non** |
| `d-undeclared-dependency` | 3 | 1 | 573 ms | 1596 ms (1646) | −1023 ms (−1073), −178,5 % | 3 → 6 | **non** |

**Économie du mélange mesuré** (une famille de scénario = un lot ; total sur
les quatre familles) :

| Fenêtre | séquentiel | groupé attendu | gain | rentable ? |
|---:|---:|---:|---:|---|
| 0 ms | 2674 ms | 3574 ms | **−900 ms (−33,7 %)** | non |
| 25 ms | 2674 ms | 3674 ms | −1000 ms (−37,4 %) | non |
| 50 ms | 2674 ms | 3774 ms | **−1100 ms (−41,1 %)** | non |
| 100 ms | 2674 ms | 3974 ms | −1300 ms (−48,6 %) | non |

Bilan en analyses sur ce mélange : 14 analyses séquentielles contre 14 analyses
groupées attendues — l'économie d'analyses est **annulée** par le coût des
reprises ; les reprises coûtent en outre plus cher qu'une analyse (overhead de
refus + isolement + re-lot).

### 8.5 Verdict (honnête, défavorable au groupement sur ce corpus)

1. **Lots 100 % compatibles** : le groupement reste très rentable, quel que soit
   N et la fenêtre testés (ex. N=5 : +80 % à fenêtre 0, +74,8 % à 50 ms).
2. **Un seul lot conflictuel détruit le gain** : à N=3, un lot sur trois
   conflictuel suffit (seuil 27 %) ; à N=6, il faut rester sous 68 % de lots
   conflictuels (fenêtre 0). Sur le mélange mesuré (50 %), le groupement
   **n'est pas rentable** (−33,7 % à −48,6 % selon la fenêtre).
3. La rentabilité dépend donc **entièrement** de la fréquence de conflits : le
   groupement n'est pas une accélération inconditionnelle. Le seuil dépend de N
   et de la fenêtre ; à N=3 il est très bas (20–27 %), c'est le régime le plus
   fragile.
4. Ce corpus étant délibérément conflictuel, il ne prouve pas que la fréquence
   réelle est de 50 %, ni l'inverse : il fournit le **seuil** auquel comparer une
   fréquence mesurée en production.

### 8.6 Recommandation mise à jour (L2.2)

1. **Ne pas activer le groupement par défaut**. Il doit rester une optimisation
   conditionnelle, sous seuil : `p_lot_attendu < p*(N, fenêtre)` avec une marge
   (par exemple viser < 30 % pour N ≥ 4, valeurs sûres sur toutes les fenêtres
   mesurées ; à N=3 le seuil de 20–27 % rend l'opération hasardeuse).
2. **Fenêtre par défaut 50 ms conservée** (compromis phase 1), avec pour la
   version conditionnelle une préférence pour 0–50 ms : à 100 ms tous les seuils
   baissent de 7 à 10 points relatifs.
3. **N par défaut 6, maximum 8** (comme phase 1), mais uniquement si le seuil est
   respecté ; à N=3–4 le groupement est marginal dès que les conflits ne sont
   pas rares.
4. **Pré-vol de conflits obligatoire** : avant de grouper, vérifier sans `rustc`
   que les propositions sont disjointes (chemins, symboles, plages de lignes) et
   que leurs préconditions CAS portent sur la même base ; les propositions en
   conflit textuel connu ne doivent jamais entrer dans le même lot. Le modèle
   suppose que `p` est la part **résiduelle imprévue** après ce pré-vol.
5. **Repli conservateur inchangé** : lot refusé → jamais de résultat partiel ;
   analyse individuelle N+1 pour isoler, re-lot des compatibles, journalisation ;
   coût de reprise mesuré (~1,4× le séquentiel) à budgéter.
6. **Télémétrie à installer avant activation** : mesurer en continu la part de
   lots où survient une décision non `AUTO_SAFE` et les décisions par
   proposition, afin de calibrer `p` réellement ; sans cette mesure, le seuil
   reste théorique.
7. Ce qui reste à implémenter (hors de cette tâche) : pré-vol de conflits,
   construction atomique du snapshot de lot, politique de repli, télémétrie
   `p`, interactions avec les `externs`/`interface_id` de L1.

### 8.7 Limites de la section 8

- Agents **scriptés déterministes**, pas de LLM : les décisions sont réelles,
  mais les 5 répétitions ne mesurent que la reproductibilité, pas une
  distribution d'arrivées.
- Une petite workspace par scénario, N fixés (3/5/3/3), pas de lot multi-unités,
  pas de macros procédurales, pas de conflit de hunk partiel (les conflits sont
  des conflits de symbole/exactitude).
- Fréquence de conflits 50 % **artefactuelle** (2 familles sur 4 conflictuelles
  par construction) : elle ne représente pas la production.
- Le **groupement n'est pas implémenté** : le modèle suppose un lot atomique et
  une reprise par analyse individuelle ; aucune mesure bout en bout du
  groupement ni de son intégration au coordinateur.
- La reprise du modèle est le coût mesuré à N=6 (constante conservatrice pour
  N<6, non extrapolée au-delà de N=6) ; fenêtres limitées à 0–100 ms.
- Machine partagée avec d'autres workers (charge non contrôlée, mémoire libre
  2,6–2,8 Go, calibration 80–96 ms).

### 8.8 Reproduire la section 8

```powershell
# binaire déjà construit (sinon une seule fois :
#   cargo build -p ravum-workspace-coordinator --bins)
node benchmarks/grouping-multiagent/run.mjs --reps 5
# preuve : evidence/mvp0c-grouping-multiagent-2026-11-09.json
# lecture : scenarios[] (décisions réelles), aggregate (fréquences mesurées),
#           model.breakEvenTable / scenarioEconomics / mixEconomics, valid.
```

Fichiers de la section 8 (aucun fichier de `crates/**` modifié) :
`benchmarks/grouping-multiagent/run.mjs`, `benchmarks/grouping-multiagent/README.md`,
`benchmarks/grouping-multiagent/fixtures/scenario-{a,b,c,d}/…`,
`evidence/mvp0c-grouping-multiagent-2026-11-09.json`, et la présente section.

---

## 9. L4.5 — échec transitoire de la vérification sémantique sous charge

Section **ajoutée** après l'investigation L4.5 (2026-11-09) ; elle ne modifie
aucun résultat des sections 1–8. Preuve :
[`evidence/mvp0c-verification-transient-2026-11-09.json`](../../evidence/mvp0c-verification-transient-2026-11-09.json)
(`valid: true`, investigation 59,4 s) ; script
`benchmarks/grouping-multiagent/investigate-transient.mjs`. Aucun fichier de
`crates/**` modifié : l'analyse du vérificateur est en lecture seule.

### 9.1 Incident observé

Ré-exécution du harnais de la section 8 par le Lead sous charge (6 workers,
RAM libre ~1,1–1,4 Go, calibration `rustc -vV` jusqu'à **441 ms** contre
~80–120 ms à vide) : **19/20 runs conformes**, 1 run non conforme.

- scénario `d-undeclared-dependency`, répétition 4, temps du run complet 3131 ms ;
- décisions obtenues : `AUTO_SAFE` (agent-d1), `MANUAL_REQUIRED` (agent-d2),
  puis **`REJECT`** sur agent-d3 au lieu de `AUTO_SAFE` attendu ;
- motifs : « semantic verification failed: merged snapshot c77b8f105753 does not
  compile with rustc 1.95.0 » puis
  « diagnostic: no-code [error] only metadata stub found for `rlib` dependency
  `std` please provide path to the corresponding .rmeta file with full metadata »
  et « diagnostic: no-code [error] cannot resolve a prelude import » ;
- **aucun E-code**, aucune ligne du contenu candidat citée ; l'échec est
  fail-closed (rien n'est intégré, le harnais marque `valid=false`) ;
- l'échec étant survenu au 3e run alors que les 20 autres passaient, il relève
  d'un état transitoire de la machine et non d'un contenu invalide.

Détail critique : la vérification sémantique exigeait la compilation d'un
contenu candidat **identique** à celui qui avait compilé dans les autres
répétitions et qui compile toujours aujourd'hui. Les deux diagnostics sont des
erreurs du chargeur de métadonnées de `rustc` (chargement de `std`), pas des
erreurs du programme candidat.

### 9.2 Protocole d'investigation contrôlée

- **Scénario d uniquement** (le seul en échec), exécuté par le binaire réel
  `ravum-collab integrate --verify --json` (aucun rebuild) ;
- **3 conditions**, 20 runs réels par condition, 12 compilations `rustc`
  **directes** par condition avec exactement les arguments du vérificateur
  (`--edition 2021 --crate-name ravum_merge_check --crate-type lib
  --emit=metadata --error-format=json --out-dir …`, `RUSTFLAGS`/`RUSTC_WRAPPER`/
  `RUSTC_WORKSPACE_WRAPPER` retirés comme dans le vérificateur) :
  - `rest` : aucune charge injectée (machine toujours partagée, RAM libre
    2,4–2,8 Go) ;
  - `cpu` : 2 processus Node en boucle CPU, bornés à 120 s puis tués ;
  - `memio` : 1 processus Node (192 Mo alloués/touchés + écritures/lectures
    fsync de 4 × 16 Mo en boucle), borné à 120 s puis tué ;
- captures : décisions et raisons complètes, code de sortie et stderr du
  binaire, durée, RAM libre avant/après, calibration `rustc -vV`, succès des
  compilations directes ; le **contenu candidat** exact est haché et archivé.

### 9.3 Résultats contrôlés

| Condition | runs | runs non conformes | probes `rustc` directes | échecs de probe | wall p50 | RAM libre avant |
|---|---:|---:|---:|---:|---:|---:|
| `rest` (sans charge injectée) | 20 | **0** | 12 | **0** | 530 ms | 2,52 Go |
| `cpu` (2 boucles CPU) | 20 | **0** | 12 | **0** | 499 ms | 2,61 Go |
| `memio` (192 Mo + I/O) | 20 | **0** | 12 | **0** | 433 ms | 2,43 Go |

- **60/60 runs conformes**, **0/36 échec de compilation rustc directe** ; les
  5 décisions attendues ont été obtenues partout.
- Compilations de référence du contenu candidat avant la campagne : **3/3
  succès** (exit 0).
- Le sysroot contient bien un `libstd-…rmeta` **complet de 9 633 944 octets**
  (et un `.rlib` de 5 648 528 octets) : au repos, `std` n'est pas un stub —
  l'erreur « only metadata stub found » ne peut donc pas venir d'un contenu
  candidat invalide.
- Échec non reproduit. Taux d'échec mesuré : **0/20 par condition** ; avec 20
  runs, la borne supérieure « règle de trois » à 95 % est ≈ 15 % — ce qui ne
  prouve pas l'absence du phénomène, seulement qu'il est rare dans ces
  conditions.

### 9.4 Cause racine : **UNDETERMINED** (honnête)

Ce qui est établi par les données :

1. Les diagnostics de l'incident ont été émis par `rustc` lui-même
   (`--error-format=json`) ; ils **ne portent aucun E-code** et ne désignent
   aucune ligne du contenu candidat : ce ne sont pas des erreurs de compilation
   du patch, mais des erreurs de chargement des métadonnées du sysroot.
2. Le même contenu candidat compile (3/3 probes de référence) et le sysroot
   contient un `std` complet : ni le contenu ni l'installation au repos ne sont
   en cause.
3. Le vérificateur **ne conserve ni le code de sortie de `rustc` ni son stderr
   brut** : seul le diagnostic parsé est archivé (`Probe { diagnostics, success }`
   dans `crates/ravum-workspace-coordinator/src/bin/common/mod.rs`). L'état
   exact du processus `rustc` pendant l'incident est donc perdu.
4. Les charges contrôlées bornées (CPU, mémoire/I/O) **ne reproduisent pas**
   l'incident ; la charge exacte du run du Lead (6 workers, RAM ~1,1 Go,
   calibration 318–441 ms) est plus sévère que les conditions reproduites.

Hypothèse la plus probable, **non prouvée** : lecture/chargement transitoire des
métadonnées du sysroot dégradé par la pression I/O/OS, transformé à tort en
échec de compilation définitif. **Statut : UNDETERMINED** — déclencheur exact
non capturé, non reproductible dans les conditions contrôlées.

### 9.5 Lacune de classification (établie, indépendante de la cause exacte)

Dans le code actuel (`crates/ravum-workspace-coordinator/src/bin/common/mod.rs`,
`RustcMergeVerifier::metadata_check` via `compile_metadata`) :

- une **erreur de spawn** de `rustc` → `Err` → `VerificationOutcome::Unavailable`
  → `MANUAL_REQUIRED` (correct : fail-closed, non définitif) ;
- un **processus `rustc` qui sort non-zéro**, *quel que soit le diagnostic* →
  `MergeCheckStatus::Failed` → `VerificationOutcome::Failed` → **`REJECT`** ;
- aucun **retry** ; seul le timeout d'exécution des tests est traité en
  `Unavailable`.

Conséquence : un incident transitoire d'infrastructure (sans E-code) est
aujourd'hui indistinguable d'un vrai échec de compilation et devient une
décision **définitive** `REJECT`. Le fail-closed est préservé (rien n'est
intégré), mais la sémantique de décision est fausse : un incident transitoire ne
prouve pas que le patch est invalide.

### 9.6 Recommandation (correctif non appliqué, pour décision du Lead)

1. **Conserver le code de sortie et un extrait borné du stderr brut** dans
   `MergeCheck` (champ d'évidence ; ≤ 2 Ko) : sans cela tout incident futur
   reste `UNDETERMINED` et non auditable.
2. **Retry borné (1 tentative) puis `Unavailable`** au lieu de `Failed` quand
   `rustc` sort non-zéro **sans E-code** (et *a fortiori* sur les motifs
   « only metadata stub found », « cannot resolve a prelude import » ou sans
   aucun diagnostic parsé) → décision `MANUAL_REQUIRED`/`UNKNOWN`, jamais
   `REJECT`. Un **vrai** échec de compilation garde son E-code et reste
   `Failed → REJECT`.
3. **Test à écrire** (non écrit ici) : wrapper `rustc` factice via `RAVUM_RUSTC` —
   (a) 1er appel émet le JSON « only metadata stub found … » + exit 101, 2e
   appel réussit ⇒ après correctif : `MANUAL_REQUIRED`/`AUTO_SAFE` après retry,
   jamais `REJECT` ; (b) wrapper émettant un vrai `E0061` ⇒ `REJECT` conservé ;
   (c) wrapper qui échoue toujours avec le motif transitoire ⇒
   `MANUAL_REQUIRED`, pas `REJECT`. Fichiers probables :
   `crates/ravum-workspace-coordinator/tests/` (nouveau) ; tests existants
   `semantic_mode_*` conservés.
   Risque : un vrai échec sans E-code serait reclassé `MANUAL_REQUIRED` au lieu
   de `REJECT` ; le retry unique + distinction E-code + journalisation bornent
   ce risque, et le fail-closed reste garanti (aucune intégration).

### 9.7 Impact sur l'interprétation L2.2

- La preuve principale L2.2 a été **régénérée après l'investigation** dans une
  fenêtre plus calme : **20/20 runs conformes, `valid=true`**, 13,5 s, RAM libre
  2,0 → 2,4 Go, calibration 100–141 ms (contre 3131 ms pour le run incriminé et
  29,2 s pour la campagne sous charge). Les chiffres des sections 8.2–8.4
  (décisions, fréquence de conflits, modèle) restent valides **en régime
  contrôlé**.
- La campagne L2.2 sous charge a produit un **faux lot conflictuel** : si le
  modèle utilise une fréquence `p` mesurée en régime non contrôlé, un
  `REJECT` transitoire gonfle artificiellement `p` et surestime le coût du
  groupement. Les mesures de fréquence de conflits doivent donc : (a) consigner
  la charge, (b) exclure/retester les runs non déterministes, ou (c) distinguer
  explicitement les incidents transitoires (`UNKNOWN`) des conflits réels.
- Les **seuils `p*`** de la section 8.4 ne sont pas modifiés (ils ne dépendent
  que des coûts mesurés en phase 1, 191/184/1596 ms) ; c'est la variable `p`
  mesurée qui est sensible à la charge.
- Aucune conclusion nouvelle sur la fréquence réelle de conflits en production :
  un incident unique sous charge extrême ne permet pas d'estimer un taux.

### 9.8 Limites de l'investigation

- Charges contrôlées **plus douces** que l'incident (2 processus CPU / 192 Mo
  + I/O bornés vs 6 workers et RAM ~1,1 Go) ; « rest » n'est pas un repos absolu
  (machine partagée, 2,4–2,8 Go libres).
- Un seul scénario (celui de l'incident), N=20 par condition : borne supérieure
  de taux ≈ 15 % à 95 % (règle de trois), pas une preuve d'impossibilité.
- Aucun correctif appliqué (périmètre `crates/**` interdit) ; la proposition de
  test n'est pas exécutée.
- L'analyse de l'incident repose sur la preuve L2.2 `valid=false` archivée dans
  la preuve d'investigation **avant** sa régénération ; le vérificateur ne
  conserve ni exit code ni stderr brut de `rustc`, ce qui borne la forensique.

### 9.9 Reproduire

```powershell
# investigation contrôlée (3 conditions x 20 runs + probes rustc)
node benchmarks/grouping-multiagent/investigate-transient.mjs
# preuve : evidence/mvp0c-verification-transient-2026-11-09.json

# preuve principale L2.2 régénérée en régime plus calme
node benchmarks/grouping-multiagent/run.mjs --reps 5
# preuve : evidence/mvp0c-grouping-multiagent-2026-11-09.json (valid=true)
```

Fichiers ajoutés par la section 9 : `benchmarks/grouping-multiagent/investigate-transient.mjs`,
`evidence/mvp0c-verification-transient-2026-11-09.json`, la présente section ;
`evidence/mvp0c-grouping-multiagent-2026-11-09.json` régénérée (`valid=true`).

---

## 10. L2.4 — gate de bout en bout : groupement + `cargo test` réel

Section **ajoutée** après la campagne L2.4 (2026-11-09) ; elle ne modifie aucun
résultat des sections 1–9. Preuve brute :
[`../evidence/mvp0c-batch-e2e-2026-11-09.json`](../../evidence/mvp0c-batch-e2e-2026-11-09.json)
(`valid: true`, 135,8 s de campagne). Suite de bench :
`ravum-daemon-bench --suite batch-e2e` (module `batch.rs`). HEAD au moment des
mesures : `24d92b1`. Aucun réseau (`cargo test --offline`), `CARGO_BUILD_JOBS=2`.

### 10.1 Question et protocole

Les sections 1–9 mesuraient `rustc --emit=metadata` ; la question du gate L2 est
de savoir si le gain survit à un **vrai build/test Rust**. Protocole gelé :

- **Fixture** `fixtures/grouping` : 6 éditions compatibles (`edits.json`, corps
  de fonctions disjoints) + 1 édition incompatible (signature de `scale`
  cassée). La fixture exécute **12 tests** : 5 unitaires (`src/lib.rs`) +
  7 d'intégration ajoutés (`tests/e2e.rs`) qui restent verts pour la base et
  pour toutes les éditions compatibles cumulées. *Note honnête* : la fixture
  contenait un test unitaire préexistant en échec (`to_kebab_case` produisait
  « mon--texte ») et des assertions exactes incompatibles avec les éditions ;
  ils ont été corrigés dans `fixtures/grouping/**` (périmètre autorisé) pour
  obtenir 12/12 à chaque étape. Les mesures L2.1/L2.2 restent des mesures
  historiques de l'état antérieur de la fixture.
- **Modes**, 5 répétitions, **ordre tourné** d'un cran par répétition :
  1. `sequential` — les 6 éditions appliquées une par une, `cargo test` après
     chaque (6 invocations) ;
  2. `grouped` — les 6 éditions fusionnées, **1** `cargo test` (+ une
     ré-exécution identique pour la mesure « warm ») ;
  3. `collector-w0` / `collector-w50` — les 6 propositions passent par le
     **vrai `BatchCollector`** (`ravum_batch`) avec un `Analyzer` qui écrit le
     contenu fusionné et exécute `cargo test --offline` ; fenêtres 0 et 50 ms ;
  4. `incompatible` — 6 compatibles + l'incompatible dans un lot via le
     collecteur : le lot doit être refusé, puis repli individuel N+1 (8
     analyses), 6 acceptés / 1 rejeté, aucun résultat incorrect accepté.
- **Cold/warm** : chaque mode et répétition démarre avec un
  `CARGO_TARGET_DIR` neuf ; la 1re invocation est **froide**, les suivantes
  **chaudes** (cache incrémental cargo). Le mode groupé et les modes collecteur
  mesurent aussi une ré-exécution identique (warm).
- **Politique de l'adaptateur L2.4** : les 6 éditions sont des hunks disjoints
  du **même** fichier. L'adaptateur déclare `paths = []` et des `symbols`
  distincts ; le pré-vol du collecteur est file-granulaire quand un chemin est
  déclaré, la sécurité réelle étant le `cargo test` de l'analyseur. Une **sonde
  de politique** (sans cargo) mesure le comportement si `paths=["src/lib.rs"]`
  était déclaré : 6 lots isolés (`preflight_isolations=5`, 6 analyses).
- **Mesures** : wall-clock par invocation, passed/failed, nombre d'invocations
  `cargo test`, lignes `Compiling` (proxy des invocations rustc), CPU processus
  harnais (`GetProcessTimes`, hors enfants cargo/rustc), RSS pic du processus
  harnais (working set, échantillonné 5 ms), taille du `CARGO_TARGET_DIR`,
  digests SHA-256 du contenu final, télémétrie du collecteur (latence d'attente
  p50/p95).

### 10.2 Résultats mesurés (p50 sur 5 répétitions, ordre alterné)

| Mode | cold | warm | total | `cargo test` | tests | CPU harnais | RSS pic | disque target |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `sequential` (6 tests) | 2 512 ms | 1 290 ms | **8 960 ms** | 6 | 12/12 | 156 ms | 7,5 Mo | 25,6 Mo |
| `grouped` (1 test) | 2 520 ms | 271 ms | **2 765 ms** | 2 (dont 1 warm) | 12/12 | 47 ms | 7,5 Mo | 17,7 Mo |
| `collector-w0` | 2 492 ms | 276 ms | **2 777 ms** | 2 | 12/12 | 47 ms | 7,5 Mo | 17,7 Mo |
| `collector-w50` | 2 582 ms | 270 ms | **2 893 ms** | 2 | 12/12 | 46 ms | 7,5 Mo | 17,7 Mo |
| `incompatible` (refus + N+1) | 534 ms | 1 303 ms | **9 861 ms** | 8 | 6 acceptés / 1 rejeté | 125 ms | 7,5 Mo | 28,2 Mo |

- **Correction identique prouvée** : les 12 tests passent dans tous les modes,
  et le **digest SHA-256 du contenu final est identique** pour les cinq modes
  (`cddae95b87c1…`) ; le mode incompatible a 2 échecs durs (compilation du lot
  puis de l'item seul), 6 propositions acceptées, 1 rejetée, `no_loss=true`.
- **Gain** : séquentiel `8 960 ms` → groupé `2 765 ms`, soit **−6 195 ms
  (−69,1 %)**. Le collecteur réel retrouve ce gain : `collector-w0`
  `2 777 ms` (**−6 183 ms, −69,0 %**), `collector-w50` `2 893 ms`
  (**−6 067 ms, −67,7 %**, attente p50 = 50 ms). Les builds froids sont
  équivalents (2 492–2 582 ms) : le gain vient de **l'élimination des
  recompilations chaudes** (5 × ~1 290 ms en séquentiel contre ~270 ms en
  groupé), pas du build initial.
- **Point d'équilibre réel** : le gain de 6 195 ms correspond à une attente
  supplémentaire tolérable de **6 195 ms** avant que le groupement ne devienne
  neutre ; les fenêtres mesurées (0 et 50 ms) sont ~100× en-dessous. Un lot
  incompatible, lui, coûte **9 861 ms** (8 `cargo test`) ≈ le séquentiel
  (8 960 ms) : le repli ne gagne rien, il garantit seulement la correction.
- La sonde de politique confirme que si l'adaptateur déclare le fichier commun,
  le pré-vol isole chaque proposition (6 lots, 6 analyses) : le gain du
  collecteur dépend donc de la granularité déclarée.

### 10.3 Verdict

**Le gate L2 passe avec un vrai `cargo test`** : sur des modifications réelles,
le groupement conserve une correction identique (12/12, digest identique) et
réduit le temps total jusqu'à tests verts de **69 %** (p50), y compris via le
collecteur `ravum-batch` réel (fenêtres 0/50 ms). Le gain est porté par la
suppression des recompilations intermédiaires (éditions séquentielles), pas par
un build initial plus rapide (cold identique).

Mais le verdict reste conditionnel et honnête :

- le gain suppose que les propositions sont **réellement disjointes** ; avec un
  adaptateur qui déclare le fichier commun, le pré-vol file-granulaire du
  collecteur isole tout et le gain retombe à zéro (sonde) — la granularité de
  conflit doit être le **hunk/symbole**, pas le fichier, ou l'adaptateur doit
  déclarer `paths=[]` avec des symboles disjoints et s'appuyer sur l'analyseur ;
- un lot contenant une édition invalide coûte N+1 analyses et ≈ le séquentiel :
  à forte fréquence de conflits, le groupement n'est pas rentable (cohérent avec
  les seuils `p*` des sections 8–9) ;
- fenêtre 50 ms : +116 ms p50 vs fenêtre 0, soit ~2 % du gain, acceptable pour
  la collecte ; fenêtre 100 ms non mesurée ici (les mesures L2.1/L2.2 restent la
  référence pour ce point).

### 10.4 Limites

- Une seule fixture, 6 éditions, 12 tests ; aucune extrapolation à des crates
  plus grosses, à d'autres types d'éditions ou à un workspace multi-crates.
- Le « cold » est un `CARGO_TARGET_DIR` neuf par mode/répétition ; il inclut le
  build de la crate de test (cargo/rustc), pas un cache partagé entre modes.
- Les wall-clocks sont mesurés sur une machine partagée (autres workers) :
  les valeurs absolues varient ; les écarts cold (~2,5 s) et warm (1,29 s vs
  0,27 s) sont néanmoins stables sur les 5 répétitions et cohérents avec la
  suppression de 5 recompilations.
- CPU/RSS sont ceux du processus harnais : le CPU des enfants `cargo`/`rustc`
  n'est pas attribué (RSS pic ~7,5 Mo pour tous les modes, non discriminant) ;
  le disque est la taille du `CARGO_TARGET_DIR` après le mode.
- Le mode `grouped` et les modes collecteur incluent une ré-exécution identique
  (warm) dans leur total ; c'est explicite dans le protocole et appliqué à tous
  les modes comparables.
- Les échecs de compilation ne produisent pas de ligne `test result` : le mode
  incompatible est compté via `hard_failures` (2) et non via `failed` (0 test
  échoué) ; la preuve JSON conserve les deux compteurs.
- Aucun `PROVED` : 12/12 tests verts et digest identique prouvent la correction
  observée pour ce corpus, pas la correction générale.

### 10.5 Reproduire

```powershell
# build du bench (une fois)
cargo build -p ravum-daemon --bin ravum-daemon-bench
# campagne complète (5 répétitions, 5 modes, ordre alterné)
target\debug\ravum-daemon-bench.exe --suite batch-e2e `
  --work-dir target\ravum-daemon-bench `
  --out-batch-e2e evidence/mvp0c-batch-e2e-2026-11-09.json `
  --batch-e2e-reps 5
# fixtures seules : 12 tests
cargo test --offline --manifest-path fixtures/grouping/Cargo.toml
```

Fichiers de la section 10 : `crates/ravum-daemon/Cargo.toml` (dépendance path
`ravum-batch`), `crates/ravum-daemon/src/bin/ravum-daemon-bench/{main.rs,batch.rs}`,
`fixtures/grouping/{src/lib.rs,tests/e2e.rs}` (12 tests), la présente section,
`evidence/mvp0c-batch-e2e-2026-11-09.json` (+ `Cargo.lock` régénéré).

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

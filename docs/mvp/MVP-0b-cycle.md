# MVP-0b — Mesure de bout en bout : édition → diagnostic → build → vérification → chargement → activation

> **But** : répondre à la seule question de P0-A/P0-I qui compte — *combien de temps
> s'écoule entre une modification réelle du source et un patch activé ?* — et mesurer ce
> que le moteur d'analyse persistant (MVP-0b) change réellement par rapport à `rustc` à
> froid (MVP-0a), **sans extrapoler**.
>
> Mesures locales, machine de référence décrite ci-dessous, charge reportée dans chaque
> fichier de preuve. Aucune garantie portable, aucune qualification temps réel.

## 1. Pourquoi une nouvelle mesure

Les campagnes L2 mesuraient **l'analyse seule** (édition → diagnostic). Elles laissaient
ouverte la métrique que le mandat place en priorité : **édition → patch activé**
(P0-A, P0-I), en séparant le coût de chaque étape (§7.6 du mandat). `ravum bench` mesurait
la boucle complète mais uniquement avec un `rustc` à froid et deux sources fixes.

Nouvelle commande : `ravum cycle-bench`, qui mesure les six étapes réelles de la boucle,
pour les deux variantes, sur des **éditions réellement nouvelles**.

## 2. Protocole (gelé avant exécution)

- **Fixture** : `fixtures/calculator/v1.rs` (petite unité, ~0,7 Ko) et une fixture
  synthétique de **2 313 lignes** générée par le harnais (`target/ravum/cycle-large.rs`,
  non versionnée : 700 fonctions + une fonction d'agrégation ; mêmes exports ABI).
- **Édition réelle** : à chaque itération, une nouvelle fonction publique est ajoutée au
  source (`cycle_bench_edit_<i>`). Le contenu change donc réellement, et `rustc` doit
  analyser et reconstruire du code neuf — ce n'est pas une édition de commentaire.
- **Deux variantes indépendantes** : pipeline + hôte + répertoire de travail + cache
  d'artefacts séparés, pour qu'aucune ne bénéficie du build de l'autre.
  - `cold` (MVP-0a) : analyse par un `rustc --emit=metadata` à froid à chaque soumission.
  - `daemon` (MVP-0b) : analyse par le daemon persistant (cache de verdicts adressé par
    contenu + `-C incremental` persistant).
- **Deux phases par contenu** : `edit` (première soumission, contenu neuf) puis
  `resubmit` (même contenu soumis à nouveau = cas interactif « buffer inchangé »).
- **Ordre alterné** des variantes par itération pour répartir la dérive de charge.
- **Chaque étape est chronométrée autour de l'appel réel** : création du snapshot,
  analyse/diagnostic, build cdylib (codegen + link dans un seul processus rustc),
  vérification smoke (`TESTED`), chargement natif + validation ABI/manifeste, activation.
- **Aucun refus n'est masqué** : un `resubmit` est refusé `AlreadyActive` par conception
  (ce n'est pas une nouvelle activation) ; le nombre d'activations et l'issue de chaque
  soumission sont écrits dans les preuves. Une édition qui n'active pas est un échec du
  harnais (aujourd'hui : 0).
- **Machine** : Windows 11 26100, i7-13620H 10c/16t, 15,71 Go RAM, `rustc 1.95.0`.
  Campagnes dans une fenêtre calme : charge CPU **11 %**, 3,6–3,9 Go libres, calibration
  `rustc -vV` p50 **48 ms** (petite) / **82 ms** (grande). D'autres campagnes de la même
  journée ont été faites sous 50–93 % : elles ne sont pas mélangées ici.

Commandes exactes :

```powershell
cargo run -q --bin ravum -- cycle-bench fixtures/calculator/v1.rs --iterations 10 --out evidence/mvp0b-cycle-bench.json
cargo run -q --bin ravum -- cycle-bench target/ravum/cycle-large.rs --iterations 5 --out evidence/mvp0b-cycle-bench-large.json
```

## 3. Résultats mesurés

### 3.1 Petite unité (10 éditions réelles, 40 soumissions)

| Variante / phase | analyses | activées | analyse p50 / p95 | build p50 | vérif. | charg. | **total p50 / p95 / max** | builds cachés |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `cold` / edit | 10 | **10** | 81 / 93 ms | 230 ms | 2 ms | 2 ms | **316 / 335 / 335 ms** | 0 |
| `cold` / resubmit | 10 | 0 (AlreadyActive) | 79 / 81 ms | 2 ms | 1 ms | 1 ms | **85 / 87 / 87 ms** | 10 |
| `daemon` / edit | 10 | **10** | 95 / 106 ms | 236 ms | 2 ms | 2 ms | **339 / 362 / 362 ms** | 0 |
| `daemon` / resubmit | 10 | 0 (AlreadyActive) | **0 / 0 ms** | 3 ms | 2 ms | 2 ms | **8 / 9 / 9 ms** | 10 |

Cache du daemon : 10 *miss* (éditions) + 10 *hits mémoire* (resoumissions), 10 exécutions
`rustc`. Calibration machine : `rustc -vV` p50 48 ms.

### 3.2 Grande unité synthétique (2 313 lignes, 5 éditions réelles, 20 soumissions)

| Variante / phase | analyses | activées | analyse p50 / p95 | build p50 | **total p50 / p95 / max** | builds cachés |
|---|---:|---:|---:|---:|---:|---:|
| `cold` / edit | 5 | **5** | 159 / 248 ms | 329 ms | **524 / 702 / 702 ms** | 0 |
| `cold` / resubmit | 5 | 0 | 167 / 210 ms | 2 ms | **177 / 221 / 221 ms** | 5 |
| `daemon` / edit | 5 | **5** | 160 / 262 ms | 435 ms | **600 / 743 / 743 ms** | 0 |
| `daemon` / resubmit | 5 | 0 | **3 / 4 ms** | 2 ms | **13 / 18 / 18 ms** | 5 |

Cache du daemon : 5 *miss* + 5 *hits mémoire*, 5 exécutions `rustc`. Calibration p50
82 ms. La différence de `build` entre variantes (329 vs 435 ms) n'est pas attribuée au
daemon : les deux builds sont des `rustc` identiques, lancés alternativement sur une
machine partagée ; l'écart est une variance observée, pas un effet du moteur d'analyse.

## 4. Lecture honnête

1. **Contenu inchangé (cas interactif dominant)** : le cycle complet passe de
   **85–177 ms** (MVP-0a) à **8–13 ms** (MVP-0b), soit **×10 à ×13**, parce que le cache
   de verdicts supprime le `rustc` d'analyse et que le cache d'artefacts supprime le build.
   La cible « diagnostic local p95 < 50 ms » est alors atteinte **de bout en bout**
   (analyse 0 ms, cycle 9–18 ms p95).
2. **Édition réelle** : MVP-0b **n'est pas plus rapide** dans ces conditions.
   - petite unité : 316 → 339 ms (+23 ms, ≈ +7 %) ;
   - grande unité synthétique : 524 → 600 ms (analyse identique, build plus lent dans cet
     échantillon).
   Le coût dominant est le **build + link à froid** (230–435 ms), que le daemon ne couvre
   pas. L'analyse avec `-C incremental` n'apporte pas de gain sur ces éditions
   (ajout de fonction en fin de fichier) aux tailles mesurées, ce qui confirme la
   conclusion de la campagne L2 : le gain incrémental dépend de la taille et du type
   d'édition, pas seulement du cache.
3. **Cible « petit patch p95 < 500 ms »** : **atteinte sur la petite unité**
   (335 ms cold / 362 ms daemon) mais **non atteinte sur l'unité de 2 313 lignes**
   (702 / 743 ms). C'est un résultat mixte, publié comme tel : elle dépend de la taille de
   l'unité et du build à froid.
4. **Aucune régression de correction** : 100 % des éditions (15/15) activées dans les deux
   variantes, et les resoumissions sont refusées `AlreadyActive` (sémantique attendue),
   jamais activées à tort.

## 5. Ce que cela implique pour la suite (P0-A/P0-I)

Le goulot n'est plus l'analyse mais **le build + link à froid**. Trois pistes, dans
l'ordre du rapport coût/bénéfice :

1. **Cache de builds par contenu** (déjà partiel) + **build incrémental** (`-C incremental`
   sur la compilation cdylib) mesuré sur des éditions réelles, y compris le coût du relink.
2. **Lien incrémental** (linker incrémental / `lld` si disponible) mesuré sur la même
   unité, sans changer de sémantique.
3. **Séparation analyse/preview** : afficher le diagnostic (< 50 ms pour l'inchangé,
   ~100–250 ms pour le modifié) sans attendre le patch ; réserver l'activation au cycle
   arrière-plan. Le mandat demande effectivement cette séparation (§7.2).

Aucune de ces pistes n'est implémentée à ce stade ; ce document ne fait que mesurer.

### 5.1 Deux accélérations de build testées et **rejetées par la mesure**

Avant d'intégrer quoi que ce soit, deux pistes ont été mesurées sur la même machine
(fenêtre calme) avec les flags exacts de l'adaptateur (`--crate-type cdylib`,
`-C opt-level=1 -C debuginfo=0 -C panic=abort`) et une édition réelle par itération :

| Piste | Petite unité (p50) | Unité 2 313 lignes (p50) | État incrémental | Verdict |
|---|---:|---:|---:|---|
| Build froid standard (`link.exe`) | 285 ms | 406 ms | — | référence |
| `-C incremental=<dir>` persistant | **321 ms** | **458 ms** | 0,17 Mo / 4,26 Mo | **plus lent** : le chargement/sauvegarde de session et le relink dépassent le gain de codegen |
| Lien par `lld-link` (rust-lld du toolchain) | non mesuré (petite unité dominée par le front-end) | **413 ms** | — | **aucun gain** (un outlier à 1 531 ms) |

Ces deux options ne sont **pas** intégrées au produit : les chiffres ne soutiennent pas le
changement, et le gain d'analyse observé en L2 ne se transpose pas au build. Le goulot
n'est donc ni le choix du linker ni l'absence de state incrémental à ces tailles : il est
dans le travail de compilation+relink lui-même. Les prochaines pistes crédibles sont de
**réduire le nombre de builds** (cache de builds par contenu plus agressif, patchs
multi-fonctions regroupés) ou d'isoler la **preview** du cycle de build complet, plutôt que
d'espérer un lien incrémental magique.

### 5.2 Deuxième levier mesuré : chevaucher l'analyse et le build (`--concurrent`)

L'analyse (diagnostic) et le build ne dépendent pas l'un de l'autre pour s'exécuter : le
diagnostic rapide sert à la preview, le build produit l'artefact. Les lancer **en
parallèle** transforme le temps total en `max(analyse, build) + queue`, au lieu de
`analyse + build`. `cycle-bench --concurrent` mesure ce chevauchement avec le **même
protocole** que la campagne séquentielle (mêmes contenus, mêmes éditions, mêmes caches,
variantes et hôtes séparés), dans la même fenêtre machine (charge 35–54 %, RAM libre
1,85–1,91 Go, reportée dans chaque fichier) :

| Campagne (édition réelle) | anal. p50 | build p50 | **total p50** | **total p95** | activées |
|---|---:|---:|---:|---:|---:|
| Petite unité, séquentiel | 122 ms | 253 ms | 390 ms | 450 ms | 10/10 |
| Petite unité, **concurrent** | 134 ms | 296 ms | **303 ms** | **316 ms** | 10/10 |
| Unité 2 313 lignes, séquentiel | 190 ms | 393 ms | 593 ms | 695 ms | 5/5 |
| Unité 2 313 lignes, **concurrent** | 193 ms | 402 ms | **412 ms** | **428 ms** | 5/5 |

- Gain sur le cycle complet : **−15 à −31 % en p50** et **−18 à −38 % en p95** selon la
  taille, pour la variante daemon comme pour la variante à froid (petite unité :
  390 → 303 ms p50 ; 2 313 lignes : 593 → 412 ms p50).
- Le coût du chevauchement est **visible et non caché** : chaque étape est plus lente sous
  contention (build 253 → 296 ms en petite unité ; analyse 190 → 193 ms en grande), mais
  le chemin critique reste inférieur à la somme.
- Conséquence sur la cible : avec ce scheduling, **p95 < 500 ms est atteint sur les deux
  tailles** (316 ms et 428 ms) alors que le séquentiel manquait la grande unité
  (695 ms). Le gain vient du **scheduling**, pas d'un build plus rapide : le build reste
  le chemin critique.
- Cas du contenu inchangé : aucun changement (analyse 0 ms pour le daemon, build déjà
  caché) ; l'analyse à froid reste le chemin critique et le chevauchement n'apporte rien
  (99–109 ms p50, écart dans le bruit).
- Limites : machine chargée et partagée, 10 et 5 itérations, unités mono-fichier ; sur une
  machine à 1–2 cœurs le chevauchement peut être contre-productif (deux `rustc` se
  disputent les cœurs), et les valeurs de « stages » en mode concurrent incluent la
  contention. La sémantique de sûreté est inchangée : l'activation n'est acceptée que si
  **l'analyse est propre et le build vérifié** ; un build lancé en parallèle d'un
  diagnostic en erreur est simplement jeté (aucune activation partielle).
- **Productisé** : `ravum watch --concurrent` et `ravum run --concurrent` utilisent
  `Pipeline::prepare_candidate_concurrent` (analyse ∥ build, puis seuils de vérification
  et fraîcheur inchangés). Vérification réelle : `run --concurrent` exit 0 ; `watch
  --concurrent` sur une copie de la fixture avec une édition réelle a activé la nouvelle
  version (`version":"v2"` dans les ticks, aucune activation partielle) ; un test
  d'équivalence séquentiel/concurrent (propre, invalide, périmé) est dans
  `ravum-coordinator`. Le mode reste **opt-in** : le défaut séquentiel est conservé pour
  les machines à peu de cœurs.



## 6. Limites

- Une seule petite unité + une fixture synthétique générée ; **aucun vrai crate
  multi-crates**, aucune macro procédurale lourde.
- Machine partagée : la charge est reportée (`machine.cpu_load_percent_at_start/end`,
  calibration `rustc -vV`) mais reste non contrôlée ; les écarts de quelques dizaines de
  millisecondes ne doivent pas être surinterprétés.
- `build_ms` couvre codegen + link dans un seul processus `rustc` : impossible de séparer
  les deux sans changer d'outil, et ce n'est pas tenté.
- Les builds MSVC ne sont pas reproductibles octet pour octet (déjà mesuré en M2) : deux
  variantes ne produisent pas le même `artifact_id`, ce qui est sans effet ici (hôtes
  séparés) mais interdit de comparer les tailles d'artefacts.
- Le cache d'artefacts est activé dans les deux variantes et chaque variante a son propre
  répertoire : la phase `edit` est un vrai build pour les deux, la phase `resubmit` un hit
  pour les deux. Un cache partagé entre variantes aurait biaisé la comparaison.
- Aucune mesure de RAM/CPU par étape au-delà du WorkingSet global du processus ; la
  consommation est celle du harnais complet, pas celle du seul daemon (voir la campagne L2
  pour le WorkingSet du daemon : 5,6–7,4 Mo).
- Aucun preview réel (pas d'interface) : « activation » signifie ici le chargement natif
  et la publication dans le `PatchHost` en processus.

## 7. Accélérations de build+link testées et écartées (relevé du 2026-11-08)

Le build+link à froid reste le chemin critique (`build_ms` p50 253 ms sur la fixture
courte au relevé officiel). Après avoir mesuré et rejeté `-C incremental` sur la cdylib et
`lld-link` (§5.1), quatre leviers d'options rustc restaient candidats. Ils ont été mesurés
par `scripts/build-flags.ps1` : même source, **9 jeux d'options** (dont un contrôle
`baseline-b` aux options strictement identiques), **16 tours entrelacés** (l'ordre des
variantes tourne à chaque tour), aucun état incrémental, aucun cache, calibration du spawn
`rustc -vV` à chaque campagne. Deux tailles : 1 342 o / 51 lignes (fonctions ajoutées comme
lors d'une édition réelle) et 35 654 o / 1 619 lignes.

| Variante (options ajoutées aux options produit) | petit : médiane | Δ apparié | gros : médiane | Δ apparié |
|---|---:|---:|---:|---:|
| `baseline` (options produit actuelles) | 276,1 ms | — | 494,4 ms | — |
| `baseline-b` (**contrôle**, mêmes options) | 268,0 ms | +0,4 % (8/16) | 476,0 ms | +2,7 % (6/16) |
| `-C codegen-units=1` | 268,7 ms | −5,4 % (10/16) | 456,4 ms | −5,6 % (12/16) |
| `-C codegen-units=64` | 271,2 ms | −2,1 % (10/16) | 501,2 ms | +4,1 % (6/16) |
| `-C codegen-units=256` | 269,5 ms | −4,4 % (12/16) | 491,6 ms | +1,2 % (7/16) |
| `-C embed-bitcode=no` | 263,6 ms | −5,4 % (10/16) | 499,4 ms | −2,0 % (10/16) |
| `-C strip=symbols` | 266,3 ms | +0,2 % (8/16) | 486,6 ms | −2,2 % (10/16) |
| `codegen-units=1` + `strip=symbols` | 272,9 ms | −3,6 % (10/16) | 449,8 ms | −6,8 % (13/16) |
| `-C opt-level=0` (**change le code généré**) | 248,6 ms | −10,5 % (11/16) | 307,4 ms | −35,5 % (16/16) |

Lecture honnête :

- **Aucune variante qui conserve la sémantique ne dépasse le plancher de bruit de façon
  robuste** : le contrôle `baseline-b`, identique au produit, varie de +0,4 % à +2,7 % de
  médiane appariée, et les meilleures variantes conservatrices restent à −5 à −7 % avec
  10–13 tours sur 16 (signe binomial faible, tests multiples). Un relevé pilote à 8 tours
  donnait `codegen-units=1` à −12,2 % sur la grande source ; à 16 tours l'effet tombe à
  −7,7 % médian / −5,6 % apparié, ce qui illustre exactement le bruit à ne pas promouvoir
  en résultat.
- **`-C opt-level=0`** réduit franchement le build (−35,5 % apparié, 16/16 sur la grande
  source) mais **change le code généré** : le coût DSP à l'exécution n'est pas mesuré ici.
  C'est donc une **décision** (avec validation DSP), pas une optimisation appliquée.
- Conséquence : les options produit restent inchangées. Les leviers réellement mesurés
  restent le **chevauchement analyse/build** (`--concurrent`, productisé, §5.2) et le
  **cache d'artefacts par contenu** (resoumission = hit, §3).
- Limites : une machine, une forme de source, `rustc` direct sous Windows, machine
  partagée avec d'autres agents (calibration de spawn 60–72 ms médiane pendant ces
  campagnes) ; aucune conclusion sur le débit DSP, la qualité du code ou le temps réel ;
  aucune preuve de supériorité produit.

## 8. Reproduire

```powershell
cd D:\App\Ravum
cargo run -q --bin ravum -- cycle-bench fixtures/calculator/v1.rs --iterations 10 --out evidence/mvp0b-cycle-bench.json
# fixture synthétique (générée par la campagne, non versionnée) :
cargo run -q --bin ravum -- cycle-bench target/ravum/cycle-large.rs --iterations 5 --out evidence/mvp0b-cycle-bench-large.json
# lecture : scenarios.cold/edit, scenarios.daemon/resubmit, daemon.cache_*, machine.*
# options rustc (leviers de build+link, §7) :
pwsh -File scripts/build-flags.ps1 -Rounds 16 -Out evidence/mvp0b-build-flags-2026-11-08.json
pwsh -File scripts/build-flags.ps1 -Rounds 16 -AppendFunctions 400 -Out evidence/mvp0b-build-flags-large-2026-11-08.json
```

Preuves : `evidence/mvp0b-cycle-bench.json`, `evidence/mvp0b-cycle-bench-large.json`,
`evidence/mvp0b-build-flags-2026-11-08.json`, `evidence/mvp0b-build-flags-large-2026-11-08.json`.

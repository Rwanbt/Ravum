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

## 7. Reproduire

```powershell
cd D:\App\Ravum
cargo run -q --bin ravum -- cycle-bench fixtures/calculator/v1.rs --iterations 10 --out evidence/mvp0b-cycle-bench.json
# fixture synthétique (générée par la campagne, non versionnée) :
cargo run -q --bin ravum -- cycle-bench target/ravum/cycle-large.rs --iterations 5 --out evidence/mvp0b-cycle-bench-large.json
# lecture : scenarios.cold/edit, scenarios.daemon/resubmit, daemon.cache_*, machine.*
```

Preuves : `evidence/mvp0b-cycle-bench.json`, `evidence/mvp0b-cycle-bench-large.json`.

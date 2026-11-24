# Intégration V2 — L2, L4, L7b et L5b

| Champ | Valeur |
|---|---|
| Destination | `claude/trusting-cerf-4hhv5a` |
| SHA de départ | `c3a96ef0d2fd4bf42aab74015a3897d4802726fb` |
| SHA du code validé | `d9fbc57a46b71010fd718f98594ae8cda878278b` |
| Environnement | Linux x86_64, rustc 1.95.0, Cargo 1.95.0, Node 24.19.0, Playwright 1.58.2 |

## Fusions

| Lot | Branche | Commit de fusion |
|---|---|---|
| L2 | `v2/L2-ir-corpus` | `be2964e` |
| L4 | `v2/L4-dsp-audio` | `7a95188` |
| L7b | `v2/L7b-async-net` | `bafe92d` |
| L5b | `v2/L5b-dsp-abi3` | `d9fbc57` |

L2 et L4 ont fusionné directement. L7b conserve les cas de conformance
déjà présents et une seule implémentation HTTP asynchrone. Le cas `det`
qui exigeait le refus en l’absence d’horloge virtuelle vérifie désormais
l’acceptation du délai virtualisé. La déclaration de l’effet `nondet`
reste contrôlée et les entrées non rejouables restent interdites.

L5b partage le chargeur ABI 3 et le tampon d’état entre l’hôte avec fondu et
l’éditeur/runner à publication concurrente. La signature ABI 3 suit
l’ordre du RFC, avec le pointeur d’état avant les buffers audio. L’ABI 2
garde son ordre historique. Le wrapper intercepte un trap de migration
dans sa bibliothèque. L’extension de statut permet au chargeur de
refuser l’état candidat avant adoption.

## Vérifications

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 tools/unsafe_audit.py
python3 tools/features_coverage.py --strict
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
cargo run -p ravum-conformance --release -- run
python3 tools/build_sqlite_native.py
NODE_PATH="$playwright_modules" node scripts/e2e-browser/notes.mjs target/debug/ravum-lang
```

- Conformance locale `TESTED` : 756 réussites, 0 échec, 2 `SKIP`.
  Dimension `--verify-build` `NOT_RUN`.
- Workspace local `TESTED` : 2 351 réussites, 0 échec, 14 ignorés.
- Navigateur `TESTED` : 11 contrôles réussis, incluant l’état SQLite et
  l’échec observé après arrêt du serveur.
- Traçabilité stricte : 248 règles sur 248. Le contrôle de confinement
  `unsafe_audit.py` a réussi. Audit indépendant `NOT_RUN`.

## Limites d’intégration

La campagne workspace utilise un checkout sur système de fichiers Linux
natif, avec l’optimisation de la seule dépendance `sha2`. Aucun seuil
n’est ajusté. Le pipeline ne lie toujours pas de crate aux programmes
Ravum générés. La connexion TCP non bloquante, les clients HTTP
asynchrones et leur campagne TSan restent hors des résultats intégrés.
La matrice des gates relève du suivi d’intégration.

## CI de la PR #203 (tête e799910, 2026-11-22)

Vert : MVP Windows, MVP impact, tsan, reproductibilité, couverture,
VS Code, qualification Windows, contrats de gates, documentation.

Rouge : qualification macOS (run 45078761816), test
`rt_budget::block_processing_meets_its_deadline_across_hot_swaps`.
`cpu_misses=2` sur 20 000 blocs : 2,26 ms et 1,62 ms de temps CPU du
thread pour une échéance de 1,333 ms (p50 13,9 µs, p99.9 201 µs). Les deux
blocs ne sont pas des blocs de swap. Hypothèse non vérifiée : pics du
runner virtualisé. Le responsable a décidé de garder le contrat et de
relancer le job. La relance par l'agent est refusée (HTTP 403) ; elle
reste à faire par un humain. État : `BLOCKED` (action humaine).

Résolu dans la même PR : SIGBUS macOS arm64 de `hot_swap`. La barrière
`isb` seule (8e45daa) n'a pas suffi. `cranelift-jit` avec `selinux-fix`
(e799910) met le code JIT dans ses propres pages `mmap` : 5/5 sur macOS.
La cause exacte, liée aux pages du tas protégées par `mprotect`, n'est pas
établie (`TESTED`, pas expliqué).

Deuxième passage macOS (bb191fa, 2026-11-23) : même test rouge,
`cpu_misses=1`, un bloc à 3,96 ms de temps CPU (bloc 3523, pas un bloc de
swap ; p50 13,3 µs, p99.9 83 µs). Sur deux passages, on observe donc 1 à 2
pics isolés de quelques millisecondes, jamais aux blocs de swap. La cause
n'est pas identifiée : soit des pics de l'hôte virtualisé, soit un vrai
coût dans `poll`/`process_sample` sur macOS. Seul un Mac physique peut
trancher (ADR : seuil inchangé, décision V2-J2).

## `rt_budget` sur macOS : analyse des échecs de la PR #209 (2026-11-23)

Le test en échec est `block_processing_meets_its_deadline_across_hot_swaps`.
Il traite 20 000 blocs de 64 échantillons, avec 50 remplacements à chaud,
et vérifie deux choses :
- aucun bloc ne dépasse en temps CPU l'échéance de 1,333 ms ;
- le p99,9 du CPU reste sous 133 µs (un dixième de l'échéance).

| Commit | Bloc fautif | CPU du bloc | CPU = mural ? | p99,9 CPU | Assertion |
|---|---|---|---|---|---|
| bfc17da | 10 884 (hors remplacement) | 2,036 ms | oui | 70 µs | 1 bloc au-delà de l'échéance |
| 2ed017a | 6 011 (hors remplacement) | 2,093 ms | oui | 197 µs | 1 bloc au-delà de l'échéance |
| acf02b5 | — | max 344 µs | — | 137 µs | p99,9 > 133 µs |

La médiane reste à environ 13 µs par bloc. Les trois jobs tournaient en même
temps (vers 15 h 42), sur des runners `macos-latest` partagés.

**Les changements de la PR sont hors de cause (vérifié).** Entre 1e0b841
(vert sur macOS) et bfc17da (rouge), seul `ravum-lang-ir/src/lower.rs` a
changé, pas le JIT. L'IR JSON des 9 versions du programme mesuré est
**identique octet pour octet** aux deux commits (comparaison faite dans un
arbre de travail séparé). Le code natif exécuté est donc le même.

**Cause probable.** Le test mesure le travail d'un bloc avec
`CLOCK_THREAD_CPUTIME_ID`. Dans une machine virtuelle, le temps pendant
lequel l'hyperviseur retire le vCPU alors que le thread tourne est compté
dans le temps CPU du thread : le noyau invité ne voit pas ce retrait.
D'où un bloc à « CPU = mural ≈ 2 ms » sans travail supplémentaire, et un
compteur de préemption à 0, puisque le test ne reconnaît une préemption que
si le CPU reste bas. C'est une hypothèse étayée, pas une preuve. La sortie
du test affiche désormais `vm=` (`kern.hv_vmm_present`) et la charge de
l'hôte, à titre de diagnostic, sans changer aucune assertion.

**Proposition au responsable** (aucun seuil ni aucune assertion modifiés) :
1. faire porter « aucun bloc ne dépasse l'échéance » par une mesure
   déterministe du travail, le carburant consommé par bloc, que le JIT
   compte déjà ; garder la mesure horaire sur matériel réel et sur les
   runners Linux non virtualisés au sens de l'invité ;
2. ou, sur un runner où `vm=1`, rejouer un bloc fautif à l'identique sur
   une copie de son état. Un travail réellement trop long dépasse
   l'échéance à chaque fois ; un retrait de l'hyperviseur, non.
3. Seule une machine Apple physique peut trancher définitivement.

**Confirmation (job macOS de a4d40ba, 18 h 32).** Le diagnostic affiche
`vm=1` (le runner est une machine virtuelle) et une charge moyenne de
`1,97 / 5,17 / 9,03` sur 1, 5 et 15 minutes. Deux blocs à 3,46 ms et
3,11 ms, avec CPU = mural et 0 préemption détectée. La médiane reste à
13,5 µs et le p99,9 à 71 µs. Le même code passe quand l'hôte est peu
chargé. L'hypothèse du temps volé par l'hyperviseur, compté comme
travail du thread, est étayée par une mesure directe. La décision sur la
mesure (proposition ci-dessus) reste au responsable.

## `rt_budget` : borne de travail par le carburant (V2-G9, 2026-11-24)

`RtRunner::last_fuel_used()` expose le carburant consommé par le dernier
appel (une lecture de champ, sans allocation). Le test
`block_processing_meets_its_deadline_across_hot_swaps` additionne ce
carburant pour chaque bloc et exige, de façon déterministe :
- que chacun des 20 000 blocs, hot swaps compris, consomme exactement la
  même quantité ;
- que cette quantité soit inférieure au budget d'**un seul** appel
  (`RT_FUEL` = 100 000).

Sur Linux, chaque bloc consomme 2 112 unités : 64 échantillons × (32 tours
de boucle + 1 entrée de fonction). Le nouveau test
`a_spinning_edit_burns_exactly_one_budget_of_fuel` vérifie qu'une boucle
infinie consomme exactement `RT_FUEL`. Les assertions de temps CPU et de
temps mural sont **inchangées**. Sur une VM chargée, elles peuvent toujours
échouer sans que le code soit en cause. La borne par carburant établit, elle,
le travail du bloc indépendamment de l'hôte.

## `rt_budget` : qualification algorithmique et temporelle séparées (V2-G10, 2026-11-24)

- **Bloquant partout** :
  - borne par carburant. `RtRunner::last_fuel_used()` compte désormais le
    travail **total** de l'appel : tentative en trap et réexécution après
    rollback. Le test `a_rolled_back_call_counts_the_fuel_of_both_attempts`
    vérifie `RT_FUEL + 1` ;
  - pile, absence d'allocation, rollback, exactitude.
- **Runner identifié comme virtuel** (macOS `kern.hv_vmm_present = 1`,
  drapeau Linux `hypervisor`) :
  - les temps sont rapportés et le verdict temporel est `INCONCLUSIVE`,
    jamais `PASS` ;
  - une garde de régression reste bloquante sur le **p50** du temps CPU
    (< 1/10 de l'échéance), que le temps volé par l'hyperviseur ne déplace
    pas.
- **Machine physique, ou machine non identifiée** (Windows notamment) : les
  assertions de temps restent bloquantes, avec les seuils inchangés.
- **Conséquence** : les runners Linux et macOS de la CI sont virtuels. Leurs
  runs ne qualifient donc plus le temps réel : seule une campagne sur
  machine physique le fait, avec
  [`tools/qualification/rt_budget_physical.py`](../../tools/qualification/rt_budget_physical.py).
- Le carburant compte les retours en arrière de boucle et les entrées de
  fonction : ce n'est **pas** une preuve de temps d'exécution maximal.

### Révision après revue (2026-11-24)

- **Budgets de carburant explicites.**
  - Par tentative : `RT_FUEL`.
  - Réexécution après rollback : un second budget de même taille, et
    jamais de troisième tentative.
  - Total par appel : `RT_FUEL_MAX_PER_CALL = 2 × RT_FUEL`.
  - Par bloc audio : la longueur du bloc multipliée par le total par appel.

  Les additions sont saturantes. `set_fuel_budget` refuse, côté hôte, un
  budget dont le double ne tiendrait pas dans un `u64`. Tests en `f32`,
  `f64` et DSP avec état, quand la seconde tentative échoue aussi (état
  restauré).
- **Script de qualification physique fail-closed**
  (`tools/qualification/rt_budget_physical.py`, il remplace l'ancien `.sh`,
  qui pouvait conclure à un succès sans preuve).
  - Une exécution n'est comptée `PASS` que si trois conditions sont
    réunies : le code de sortie de Cargo vaut 0, le test d'échéance a
    réellement tourné, et une seule ligne de verdict temporel est présente
    avec ses statistiques (p50, p95, p99, p99.9, max, dépassements).
  - Verdicts : `PASS` 0, `FAIL` 1, `INCONCLUSIVE` 2 (hôte virtuel), `NOT_RUN`
    3 (zéro exécution, ou campagne interrompue).
  - Le rapport JSON contient le SHA, l'état propre ou modifié du dépôt,
    l'OS, le CPU, `rustc` et le profil ; le journal complet de chaque
    exécution est conservé.
  - Neuf tests automatisés couvrent : réussite, erreur de compilation,
    panic, absence de preuve, interruption, hôte virtuel, zéro exécution,
    régression, absence de régression.
- **Régressions** : références versionnées par profil matériel et logiciel
  (`tools/qualification/rt_budget_baselines.json`, vide tant qu'aucune
  campagne n'a été mesurée). Une statistique au-delà de 2 fois sa référence
  donne `FAIL`. La garde absolue sur le p50 reste dans le test.
- Un succès de Cargo n'est **jamais** une qualification temporelle : sur un
  hôte virtuel, le verdict reste `INCONCLUSIVE`.

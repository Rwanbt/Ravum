# RFC-P2-F — Compilation continue : snapshots, cache, annulation, activation

> **Statut : PROPOSED — CONCEPTION SEULEMENT.** Aucune ligne de code n'est
> proposée comme implémentée. Ce document décrit un état cible, mesure l'écart, et
> répartit le reste à faire entre ce qui est déjà écrit ailleurs dans le dépôt, ce
> qui est mécanique, et ce qui doit être décidé ou mesuré.
>
> | Élément | Valeur |
> |---|---|
> | Date | 2026-11-11 |
> | Branche / HEAD | `feat/lang-core-1.0-rc` @ `5d88ecb` |
> | P0 concernés | P0-A (feedback continu), P0-B (performance/artefacts), P0-C (sûreté), P0-H (sobriété) |
> | Invariants visés | INV-03 (activation sûre), INV-04 (contexte exact), INV-07 (résultats obsolètes) |
> | Objet | P2-F de la décomposition P2 de la session 2026-11-11 |

## 1. Ce qui existe, mesuré

### 1.1 La boucle du langage

| Élément | État réel | Preuve |
|---|---|---|
| `ravum-lang watch` (alias `dev`) | **polling 250 ms** (`.rv`, `ravum.toml`, `ravum.lock`), rebuild complet, `--run` relance le binaire | `crates/ravum-lang-cli/src/commands/watch.rs:56` (`const POLL_INTERVAL`) |
| Cache | un **dossier de sortie**, pas un cache d'analyse : `<package>/.ravum/{gen,bin}` | `crates/ravum-lang-cli/src/util.rs:106` (`ensure_cache_dir`) |
| Éviction | **aucune GC automatique** : `ravum-lang cache clean` est la seule éviction documentée | `crates/ravum-lang-cli/src/commands/cache.rs:1-6` |
| Écriture du fichier généré | atomique (fichier temporaire + `rename` + `sync_all`) | `util.rs:58` (`write_bytes_atomic`) |
| Identité de build | **absente** : un seul chemin `.ravum/gen/main.rs` et un seul `.ravum/bin/<nom>.exe` par paquet | `pipeline.rs:407-449` |
| Compilation | **un `rustc` neuf** par build, sans `-C incremental=` | argv capturé (RFC-P2-VERTICALS-BLOCKERS §1 sonde #11) |
| Annulation | **aucune** : `command.output()` est bloquant, sans délai ni kill | `pipeline.rs:481` |
| Résultat obsolète | **non modélisé** : aucun identifiant de snapshot dans la chaîne | `pipeline.rs` (aucun champ) |
| Diagnostics liés à un snapshot | **non** : rendus depuis le `SourceMap` chargé en mémoire, sans identité publiée | `render_diags` (`pipeline.rs:88`) |
| Débounce | aucun : chaque changement d'mtime/taille déclenche un rebuild | `watch.rs` (`scan`) |

### 1.2 Latence de la boucle (mesurée)

`examples/dsp-gain-iir`, binaire `target/debug/ravum-lang.exe`, 3 répétitions,
machine partagée avec d'autres agents (charge CPU observée **26 % → 46 %**
pendant la mesure, donc majorant) :

| Étape | Médiane | Min–max |
|---|---:|---|
| `check` (analyse Ravum complète) | **15,8 ms** | 13,0 – 23,4 ms |
| `build` (analyse + génération + `rustc` debug) | **392,6 ms** | 361,0 – 428,9 ms |
| `build --release` (édition Rust optimisée) | **1 470 ms** (1 mesure) | — |
| Détection d'un changement (`watch`) | ≤ 250 ms (intervalle de polling) | — |

**Conclusion de conception n°1** : l'analyse Ravum coûte ~16 ms, `rustc` coûte
~377 ms (96 % du build). Toute stratégie d'invalidation ciblée qui optimise
l'analyse gagne au mieux 4 % de la boucle. L'effort d'ingénierie doit viser
**l'étape `rustc`** (réutilisation, cache, ou exécution asynchrone annulable), pas
le checker.

### 1.3 Expérience mesurée : l'identité de build n'existe pas

Sonde reproductible (paquet jetable `target/probes/runtime-p2f/lastgood/`) :

1. source bonne → `build` **OK** ; empreintes : `gen/main.rs = 3115ABA8…`,
   `bin/lastgood.exe = 279F3B64…` (142 336 octets) ;
2. source remplacée par un programme **accepté par `check`** (0 erreur) mais
   **refusé par `rustc`** (`E0106: missing lifetime specifier` ×3) — la
   divergence P0-A déjà connue, reproduite ici dans un paquet neuf ;
3. `build` **échoue** (exit 1) ; empreintes après : `gen/main.rs = A571E793…`,
   `bin/lastgood.exe = 279F3B64…`.

Résultat : **le fichier généré a changé, le binaire n'a pas changé.** Le dossier de
cache décrit alors une source qui **ne correspond pas** au binaire présent. C'est
une violation directe d'INV-04 (« toute évidence porte ses identités de snapshot,
build, preuve, artefact ») : après un build raté, l'artefact et sa source
générée divergent silencieusement.

**Conclusion de conception n°2** : « garder la dernière bonne version » n'est pas
seulement une question d'activation ; il faut d'abord que **l'artefact et sa
source soient nommés par la même identité**. Aujourd'hui, même le binaire survivant
n'est plus traçable.

### 1.4 Ce qui existe déjà ailleurs dans le dépôt (et n'est pas branché)

Le dépôt implémente déjà l'essentiel des primitives — pour la boucle Rust du
projet, pas pour le langage Ravum :

| Primitive | Crate | Contrat réel |
|---|---|---|
| Snapshot immuable, adressé par contenu | `ravum-snapshot` | `SourceSnapshot` **jamais muté** ; un overlay produit un **nouveau** `SnapshotId` (doc de `src/lib.rs:1-6`) |
| Snapshot courant + rejet des résultats obsolètes + ordre analyze→build→verify→publish + rétention | `ravum-coordinator` | « owns which snapshot is current and which finished results are therefore `STALE` » (`src/lib.rs:1-8`) |
| Révisions + cache + GC + priorité | `ravum-daemon` | protocole `analyze` / `advance_revision` / `gc` / `stats` (`src/bin/ravum-daemon.rs:1-30`) |
| Collecte bornée, horloge injectée, sans appeler `rustc` | `ravum-batch` | `now_ms: u64` fourni par l'appelant, donc **aucun test flaky** (`src/lib.rs:1-30`) |
| Activation atomique + sûreté des appels en vol + récupération différée | `ravum-patch-host` | `ActivationReport`, garde non possédante, déchargement hors thread audio (tests `rt_lifecycle`) |

**Fait décisif** : `crates/ravum-lang-cli/Cargo.toml` ne dépend que de
`ravum-lang-syntax`, `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-lsp`.
Aucune de ces primitives n'est utilisée par la chaîne Ravum. La boucle continue
du langage est donc **indépendante et moins sûre** que la boucle du projet.

## 2. Matrice exigence → état → reste à faire

| # | Exigence | État mesuré | Ancre existante | Ce qui reste | Classe |
|---|---|---|---|---|---|
| R1 | **Snapshots immuables** | absent côté langage : `watch` compare des couples (mtime, taille) | `ravum-snapshot` (`SourceSnapshot`, `SnapshotId`) | construire un `SourceSnapshot` depuis le paquet Ravum (sources `.rv` + manifeste + lock) et le faire circuler dans `Frontend` | `FIXABLE_NOW` (réutiliser, pas réinventer) |
| R2 | **Cache incrémental valide** | absent : `.ravum/{gen,bin}` est un dossier de sortie, pas un cache ; `cache clean` est la seule éviction | `ravum-daemon` (GC + `keep_recent` + bornes d'octets) | clé de cache = (SnapshotId, profil, options, toolchain) ; politique de rétention bornée | `NEEDS_EXPERIMENT` — il faut d'abord mesurer si `rustc -C incremental=` apporte un gain (voir §3.3) |
| R3 | **Invalidation ciblée** | inexistante : chaque changement → analyse complète (16 ms) + build complet (377 ms) | aucune | deux niveaux : (a) invalider l'artefact si `check` est inchangé et que seul un commentaire change ; (b) invalider finement par module — **non prioritaire**, l'analyse est à 4 % du coût | `DEFERRED` pour (b), `NEEDS_EXPERIMENT` pour (a) |
| R4 | **Compilation annulable** | impossible : `command.output()` bloque jusqu'à la fin de `rustc` | `ravum-batch` (fenêtre bornée) ; aucun équivalent CLI | passer à `spawn()` + attente bornée + `kill()` sur nouvelle révision ; compteur de génération pour ignorer un résultat périmé | `FIXABLE_NOW` (mécanique, mais doit être conçu avec R5) |
| R5 | **Résultats obsolètes rejetés** | non modélisé : aucun identifiant dans la chaîne | `ravum-coordinator` (`STALE`) | porter la révision dans `Frontend`/`Artifacts` et refuser d'activer un résultat dont la révision ≠ révision courante | `FIXABLE_NOW` (dépend de R1) |
| R6 | **Dernière bonne version conservée** | **violé et mesuré** : §1.3 (source générée et binaire divergent) ; et `watch` tue l'enfant **avant** de compiler (P0-C) | `ravum-patch-host` (activation atomique) | chemins d'artefact **uniques par build** + pointeur « dernier bon » ; ne tuer l'enfant qu'après un build réussi | `FIXABLE_NOW` — c'est exactement le correctif P0-B/P0-C déjà identifié |
| R7 | **Activation contrôlée** | `watch --run` = **restart** brutal (kill puis spawn) ; pas d'état préservé (T8 : « rebuild+restart ») | `ravum-patch-host` (pointeur de version, garde en vol, récupération différée) | pour le langage : activer = lancer le **nouveau** binaire, garder l'ancien vivant jusqu'au succès ; le hot swap d'état reste hors périmètre (`unsupported.md` §5, Q-UNS-5) | `FIXABLE_NOW` pour le « keep-then-swap » ; `DEFERRED` pour la préservation d'état |
| R8 | **Rollback** | impossible : le chemin du binaire est partagé, donc le précédent est écrasé au build suivant | `ravum-patch-host` (`ActivationReport` rejeté ⇒ version active inchangée) | conserver N artefacts par build id + un pointeur ; rollback = repointer puis relancer | `FIXABLE_NOW` (dépend de R6) |
| R9 | **Diagnostics liés au snapshot** | non : `check --json` ne porte aucune identité de snapshot ; `render_diags` utilise le `SourceMap` courant | `ravum-snapshot` | ajouter `snapshot_id` (et `build_id` pour `build`) aux sorties JSON et au texte ; un diagnostic d'une révision antérieure doit être **rejeté**, pas affiché | `FIXABLE_NOW` (changement de schéma JSON ⇒ coordonner avec le contrat `schema_version`) |

## 3. État cible proposé (conception, non implémentée)

### 3.1 Chaîne d'identités

```
sources .rv + ravum.toml + ravum.lock
        └── SourceSnapshot{ id }            (immuable, adressé par contenu)
              └── analyse  → CheckedProgram{ snapshot_id }
                    └── génération → gen source (chemin unique par build)
                          └── rustc → artefact (chemin unique par build)
                                └── ActivationReport{ snapshot_id, build_id, artifact }
```

Règles :

1. un `SnapshotId` ne change jamais ; un overlay produit un nouvel id ;
2. **aucun chemin d'artefact partagé** : `<package>/.ravum/builds/<build_id>/…`,
   ce qui supprime la classe de défauts P0-B par construction ;
3. `.ravum/current` (ou un petit manifeste) pointe le **dernier build activé** ;
   un build raté ne touche pas ce pointeur ;
4. un résultat dont l'identifiant ≠ courant est `STALE` et ne peut pas être activé
   (INV-07) ;
5. `--emit-rust FILE` reste un chemin explicite fourni par l'utilisateur : il est
   hors du cache et n'est jamais activé.

### 3.2 Activation et rollback

| Étape | Comportement cible | Aujourd'hui |
|---|---|---|
| changement détecté | compiler **sans** tuer le processus en cours | tue d'abord (`watch.rs`, branche de changement) |
| build réussi | lancer le nouveau, puis arrêter l'ancien (ou laisser l'ancien finir) | kill puis spawn |
| build raté | l'ancien continue ; diagnostics affichés avec le `snapshot_id` fautif | l'ancien est déjà mort (P0-C) |
| rollback | repointer `current` vers un build_id antérieur et relancer | impossible (chemin écrasé) |

### 3.3 Expérience à faire avant d'écrire un cache (R2)

**Objectif** : mesurer si `rustc -C incremental=<dir> -C codegen-units=…` réduit
les ~377 ms du build pour un programme Ravum réaliste.

Protocole minimal :

1. prendre `examples/dsp-gain-iir` (et un programme synthétique de taille
   moyenne, cf. `docs/qualification/PERFORMANCE.md` §2 pour la forme) ;
2. mesurer `build` **sans** option incrémentale (déjà : médiane 392,6 ms debug) ;
3. rejouer avec `-C incremental=<package>/.ravum/incr` en ajoutant les arguments
   dans `pipeline.rs:456` ;
4. 5 répétitions, médiane, machine non chargée ou charge déclarée ;
5. **critère de décision** : un gain ≥ 30 % sur l'étape `rustc` justifie un cache ;
   en dessous, le cache n'est pas rentable et R2 reste `DEFERRED`.

Tant que cette mesure n'existe pas, écrire un cache serait une abstraction sans
consommateur réel démontré (INV-10).

### 3.4 Ce que ce RFC ne fait pas

- il **n'implémente** ni R1–R9, ni le cache, ni l'annulation ;
- il **ne promet pas** de hot reload d'état : `unsupported.md` §5 et Q-UNS-5
  placent le hot swap hors du langage (propriété de runtime/daemon) ;
- il **ne remplace pas** la tâche P0-B/P0-C : R6/R7/R8 sont précisément leur
  contenu, et ce document ne fait que les relier aux exigences P2-F.

## 4. Plan par étapes (chacune livrable et vérifiable)

| Étape | Contenu | Dépend de | Classe |
|---|---|---|---|
| **S1** | `snapshot_id` dans le `Frontend` et dans les sorties `check --json` / texte (R1, R9 partiel) | — | `FIXABLE_NOW` |
| **S2** | chemins d'artefact uniques par build + pointeur `current` + ne pas écraser un bon artefact (R6, R8) | S1 | `FIXABLE_NOW` (contenu P0-B) |
| **S3** | `watch` : compiler d'abord, n'arrêter l'ancien qu'après succès (R6, R7) | S2 | `FIXABLE_NOW` (contenu P0-C) |
| **S4** | compilation asynchrone annulable + rejet des résultats périmés (R4, R5) | S1 | `FIXABLE_NOW` |
| **S5** | mesure `rustc -C incremental` (§3.3), puis décision sur le cache (R2) | — | `NEEDS_EXPERIMENT` |
| **S6** | invalidation ciblée par module (R3) | S5 seulement si S5 conclut au gain | `DEFERRED` |

Test d'acceptation commun à S1–S4 : le scénario mesuré en §1.3 doit devenir
**impossible** — après un `build` raté, `gen` et le binaire doivent être soit
identiques (tous deux périmés, mais cohérents), soit absents, et le build
précédent doit rester activable et identifiable.

## 5. Risques et limites

| # | Risque | Mitigation |
|---|---|---|
| F-R1 | Le cache masque une divergence `check`/`build` (P0-A) : un artefact caché peut être « vert » pour une source qui ne compile pas | la clé de cache inclut l'identité du *build*, pas seulement du `check` ; un build raté n'écrit jamais de cache |
| F-R2 | L'annulation d'un `rustc` laisse des fichiers temporaires | builds dans un dossier par `build_id`, GC sur rétention bornée (modèle `ravum-daemon`) |
| F-R3 | Ajouter `snapshot_id` au JSON change le contrat d'outillage | champ additif + bump de `schema_version` documenté, coordination avec les consommateurs (`lsp`, tests `cli.rs`) |
| F-R4 | La rétention de N artefacts consomme l'espace disque (P0-H) | borne en octets + `cache clean` existant, politique explicite (ne pas copier la rétention du daemon sans la mesurer) |
| F-R5 | Le « keep-then-swap » ne préserve aucun état : un DSP qui tourne perd son état à chaque activation | c'est déjà la limite déclarée de T8 ; ne pas la présenter comme du hot reload |

## 6. Questions ouvertes

- **Q-P2F-1** — Faut-il brancher `ravum-lang-cli` sur `ravum-snapshot` /
  `ravum-coordinator` (dépendance nouvelle, +2 crates) ou dupliquer un
  sous-ensemble minimal dans la CLI ? Recommandation : **réutiliser**
  `ravum-snapshot` seul (types + ids), et **ne pas** tirer `ravum-coordinator`
  tant que la boucle du langage n'a pas besoin de sa politique de rétention —
  INV-10 exige un consommateur réel.
- **Q-P2F-2** — Le `snapshot_id` doit-il porter les **options de build**
  (profil, `--release`, cible) ? Recommandation : non — le snapshot est la
  source ; le `build_id` est (snapshot, options, toolchain). Deux notions, deux
  champs.
- **Q-P2F-3** — Le `watch` doit-il rester un polling à 250 ms ou passer à un
  watcher d'événements (crate externe) ? Recommandation : rester en polling —
  la latence mesurée (≤ 250 ms) est négligeable devant les 392 ms de build, et
  le polling n'ajoute aucune dépendance.

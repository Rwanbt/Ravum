# RFC-DETERMINISTIC-PROFILE-1 — Profil `det` : exécution rejouable

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-DETERMINISTIC-PROFILE-1` |
| Statut | **ACCEPTED** — ratifiée par le responsable le 2026-11-20 (V2-R1), avec l'enregistrement et le rejeu des E/S (§2.3) ; réponses du §5 : propositions retenues (`det`, `nondet` produit, PRNG écrit dans le runtime) |
| Date | 2026-11-20 |
| Auteur | agent de développement |
| Gate | T17 (« le profil déterministe du langage (horloge, aléa, ordonnancement virtualisés, replay par seed) n'est pas fait ») |
| Reprend | `spec/language/effects.md` §4 (profils), effet réservé `nondet` (Q-EFF-3), `docs/reports/T17-STEP1-HOT-SESSION-REPLAY.md` |
| Portée | `ravum-lang-sema` (profil), runtime stdlib (voie `rustc`), `std::task`, `std::thread`, CLI (`run`/`test --profile det --seed`) |

> **Statut de preuve.** Rien n'est implémenté. Un rejeu réussi est `TESTED`
> pour les entrées rejouées, jamais `PROVED`.

## 1. Problème

L'étape 1 de T17 rejoue bit à bit une session de hot swap sur le JIT, parce
que les entrées temps réel n'ont ni horloge, ni aléa, ni E/S (INV-05). Un
programme `.rv` ordinaire, lui, n'est pas rejouable : l'ordre entre threads
n'est pas spécifié (STD-18.9), l'exécuteur `std::task` dort selon l'horloge
(CONC-6.7), et `timeout` (RFC-ASYNC-NET-1) lira une horloge. Un échec
intermittent ne peut donc pas être reproduit. L'effet `nondet` existe, mais
il est vide (aucune source ne le produit).

## 2. Proposition : un profil `det`

Un **quatrième profil** de vérification (`--profile det`), au même titre que
`core`/`pure`/`rt` (EFF-4) : un mode, pas une annotation source. Il combine
un refus statique et une exécution virtualisée.

### 2.1 Sources de non-déterminisme et leur traitement

| Source | Sous `core` | Sous `det` |
|---|---|---|
| Ordre des threads (`thread::spawn`, canaux) | OS | **ordonnanceur déterministe** : un seul thread OS exécute à la fois, commutation aux points de synchronisation (`send`, `recv`, `join`, `spawn`), choix dérivé de la graine |
| Exécuteur `std::task` | sommeil réel ≤ 1 ms | **horloge virtuelle** : le sommeil avance l'horloge sans attendre |
| Horloge (`task::timeout`, futures `time::*`) | monotone réelle | **virtuelle**, part de 0, avancée par l'ordonnanceur |
| Aléa (futur `std::random`) | OS | PRNG fixé (SplitMix64 / xoshiro256**) initialisé par la graine |
| E/S `fs`, `net`, `io` (lecture) | réelles | **refusées** (`E-EFF-PROFILE`), sauf en mode enregistrement (§2.3) |
| `io` (écriture stdout/stderr) | réelle | permise : c'est la sortie comparée |

### 2.2 Règles

| ID | Règle proposée | Niveau |
|---|---|---|
| DET-1 | `--profile det` est accepté par `check`, `run`, `test` ; il interdit au point d'entrée, transitivement, les effets `fs net ffi unsafe` et la lecture d'`io` (`io::read_line`) : `E-EFF-PROFILE`. `thread`, `async`, `alloc`, `panic` et l'écriture d'`io` sont permis. | MUST |
| DET-2 | `nondet` devient un effet **produit** : toute fonction qui lit l'horloge réelle ou l'aléa de l'OS le porte. Sous `det`, ces fonctions sont **liées à leur version virtuelle** au lieu d'être refusées ; sous `pure`, `nondet` reste refusé (EFF-4.2 inchangé). | MUST |
| DET-3 | `run --profile det --seed <u64>` : deux exécutions de même source, même graine, même binaire, même plateforme produisent la **même sortie octet à octet** et le même code de sortie. Graine absente : `0`. | MUST |
| DET-4 | La graine choisit l'ordre des threads prêts à chaque point de synchronisation ; elle ne change jamais la sémantique d'un programme dont la sortie ne dépend pas de l'ordre (EFF-4.9 : le profil n'ajoute que des refus et fixe un ordre parmi ceux que `core` permet). | MUST |
| DET-5 | `test --profile det --seeds N` exécute chaque test pour les graines `0..N` ; un échec affiche la graine (`seed=<k>`) et `run --seed <k>` le reproduit. | MUST |
| DET-6 | Interblocage sous `det` (aucun thread prêt, aucun futur réveillable) : arrêt immédiat avec `panic: deadlock (det, seed=<k>)` et code `1`, au lieu d'une attente infinie. | MUST |
| DET-7 | Le rapport de profil déclare spécification, profil `det`, graine(s), plateforme, et résultat `PASS`/`FAIL` pour ces graines, jamais `PROVED` (EFF-6.7). Le déterminisme entre plateformes n'est **pas** garanti (flottants `libm`, Q-STD-6). | MUST |
| DET-8 | Le JIT et l'interpréteur de l'IR, qui n'ont ni threads ni horloge, sont déterministes par construction ; `det` n'y change rien. | MUST |

### 2.3 Enregistrement et rejeu des E/S (`--record` / `--replay`)

Ajouté à la demande du responsable (2026-11-20) : sans lui, un serveur ou un
outil qui lit des fichiers ne serait pas rejouable.

```text
ravum-lang run --profile det --seed 7 --record session.rvlog   # E/S réelles, journalisées
ravum-lang run --profile det --replay session.rvlog            # E/S servies par le journal
```

| ID | Règle proposée | Niveau |
|---|---|---|
| DET-9 | `--record <f>` : sous `det`, les effets refusés par DET-1 (`fs`, `net`, lecture d'`io`) sont **permis** ; chaque appel d'une fonction de la stdlib à effet externe est exécuté réellement, et son **résultat** (valeur rendue, `Err` compris, octets lus) est ajouté au journal avec son rang, le nom de la fonction et un digest de ses arguments. `ffi` et `unsafe` restent refusés (un appel natif n'est pas journalisable). | MUST |
| DET-10 | `--replay <f>` : aucune E/S réelle n'a lieu ; chaque appel rend le résultat journalisé au même rang. La graine est lue dans le journal (`--seed` contradictoire : exit 2). | MUST |
| DET-11 | **Divergence** : au rejeu, un appel dont le rang, le nom ou le digest d'arguments ne correspond pas au journal, ou un journal épuisé, arrête le programme : `panic: replay diverged at #<n>: expected <f>(…), got <g>(…)`, code `1`. Jamais de valeur inventée, jamais de repli sur une E/S réelle (fail-closed). | MUST |
| DET-12 | Les écritures externes (`fs::write`, `net::write_str`, `http::respond`) sont journalisées à l'enregistrement ; au rejeu elles ne sont **pas** effectuées : leur argument est comparé au journal (DET-11) et le résultat journalisé est rendu. La sortie `io` (stdout/stderr) est réellement écrite dans les deux modes et doit être identique (DET-3). | MUST |
| DET-13 | Le journal est un fichier texte versionné (`ravum-rvlog-1`), une ligne JSON par événement ; en tête : version de spécification, graine, **identité de build** (digest de la source, des dépendances, des options, de la cible, de `rustc` et du générateur), plateforme. (Amendé par le responsable le 2026-11-21 : le digest des octets du binaire n'est pas stable d'une reconstruction à l'autre sous macOS et Windows.) Un en-tête qui ne correspond pas au programme rejoué est refusé avant l'exécution (exit 2). | MUST |
| DET-14 | Le journal peut contenir des données sensibles (fichiers lus, requêtes reçues) : il n'est jamais écrit sans `--record` explicite, et le rapport le signale. | MUST |
| DET-15 | L'ordonnancement (DET-4) et l'horloge virtuelle sont rejoués depuis la graine, pas journalisés ; le journal ne contient que les E/S. | MUST |

## 3. Tests prévus

- **DET-T1** même graine ⇒ même sortie : programme à 4 threads et canaux
  imprimant dans un ordre dépendant de l'ordonnancement, 100 exécutions par
  graine, 0 écart.
- **DET-T2** graines différentes ⇒ au moins deux ordres observés (le profil
  explore réellement).
- **DET-T3** `test --seeds 64` trouve un défaut d'ordre injecté et affiche
  la graine ; `run --seed` le reproduit.
- **DET-T4** interblocage injecté → `panic: deadlock (det, seed=…)` en
  moins d'1 s.
- **DET-T5** horloge virtuelle : un programme qui attend 10 min de
  `timeout` virtuel se termine en < 1 s réelle.
- **DET-T6** négatifs : `fs::read` / `net::connect` / `io::read_line` sous
  `det` → `E-EFF-PROFILE` ; graine non numérique → exit 2 (EFF-4.10).
- **DET-T8** enregistrement puis rejeu d'un serveur `.rv` (requêtes HTTP
  réelles) : même sortie, aucune socket ouverte au rejeu (vérifié).
- **DET-T9** divergence : journal modifié (un octet d'argument, un appel
  supprimé, journal tronqué) → `replay diverged at #n`, code 1.
- **DET-T10** en-tête d'une autre source → exit 2 avant exécution.
- **DET-T7** non-régression : tout le corpus de conformance accepté sous
  `core` donne la même sortie sous `det` pour les programmes sans thread.

## 4. Alternatives écartées

- **Enregistrer l'ordonnancement OS** (rr, journal de commutations) :
  dépend de la plateforme, lourd, hors de l'architecture indépendante.
- **Interdire `thread` sous `det`** : simple, mais inutile pour les bugs
  qu'on veut reproduire (ordre entre threads).
- **Annotation source `#[deterministic]`** : les attributs ne sont pas
  sémantiques en Core 1.0 ; un profil est un mode de vérification (EFF-4).

## 5. Questions au responsable

1. Nom du profil : `det` ou `deterministic` ?
2. Produire `nondet` (DET-2) — ce qui répond à Q-EFF-3 — ou garder un
   mécanisme propre au profil ?
3. PRNG : SplitMix64 / xoshiro256** écrits dans le runtime (pas de
   dépendance) ?
4. Le mode enregistrement et rejeu (§2.3, DET-9 à DET-15) : règles à valider (ajouté à votre demande).

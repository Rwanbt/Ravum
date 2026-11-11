# RFC-P2-B/C/D/E — Verticales réseau et stockage : matrice de blocage exacte

> **Statut : PROPOSED — non ratifié.** Dossier de décision, pas une implémentation.
> Aucune ligne de code du langage n'est modifiée par ce document, et **aucune
> capacité réseau ou SQLite n'est ajoutée au langage**.
>
> | Élément | Valeur |
> |---|---|
> | Date | 2026-11-11 |
> | Branche / HEAD | `feat/lang-core-1.0-rc` @ `5d88ecb` |
> | Spécification de référence | `core-1.0-draft.1` |
> | Plateforme | Windows 11 x86-64, i7-13620H, 16 threads, ≈15,7 Go RAM, MSVC `14.50.35717` |
> | Toolchain | `rustc 1.95.0`, `cargo 1.95.0` |
> | Objet | P2-B (TCP), P2-C (HTTP), P2-D (SQLite), P2-E (premier vertical complet) |
>
> **Sources de vérité lues pour ce dossier** : `spec/language/effects.md`,
> `unsupported.md`, `stdlib.md` §14, `crates/ravum-lang-sema/src/{facts,builtins,effects,resolve}.rs`,
> `crates/ravum-lang-codegen/src/{emit,prelude}.rs`,
> `crates/ravum-lang-cli/src/pipeline.rs`, `DECISIONS_REQUIRED.md` (D2, D6, D7),
> `docs/architecture/PROJECT_INVARIANTS.md`.
>
> **Provenance** : sondes exécutées le 2026-11-11 entre 11:32 et 11:48 sur un
> arbre **partagé et édité en parallèle** (`emit.rs`, `own.rs`, `typeck.rs`,
> `run.rs`, `test.rs` modifiés pendant la session). Les sondes #1–#6 ont été
> rejouées sur le second binaire (reconstruit à 11:41:51) et rendent des codes
> **identiques**. Les numéros de ligne valent au SHA `5d88ecb` ; les fichiers
> sous édition concurrente sont cités par **nom de symbole**.

## 1. Méthode : tout est mesuré, rien n'est supposé

Chaque affirmation de blocage ci-dessous correspond à une sonde exécutée sur
`5d88ecb`. Les paquets de sonde sont jetables et vivent sous
`target/probes/runtime-verticals/<nom>/` (hors suivi Git). Chaque paquet est
trivial — un manifeste minimal et un `src/main.rv` dont la partie significative
est reproduite ci-dessous, ce qui suffit à rejouer la sonde :

```toml
[package]
name = "<nom-de-la-sonde>"
version = "0.1.0"
edition = "core-1.0"
```

Binaire utilisé : `target/debug/ravum-lang.exe`, se présentant comme
`ravum-lang 0.1.0 (edition core-1.0-draft.1)`.

| # | Sonde | Source `.rv` (essentiel) | Commande | Résultat mesuré |
|---|---|---|---|---|
| 1 | `net` déclaré | `fn f() effects(net) { }` | `check` | `E-UNSUPPORTED-EFFECT` — « effect `net` is not supported in Core 1.0 », exit 1 |
| 2 | `async` déclaré | `fn f() effects(async) { }` | `check` | `E-UNSUPPORTED-EFFECT`, exit 1 |
| 3 | threads | `use std::thread;` | `check` | `E-RES-UNKNOWN` — « unknown import path `std::thread` », exit 1 |
| 4 | appel réseau | `let b = http::get("http://127.0.0.1:1/");` | `check` | `E-RES-UNKNOWN` — « unknown name `http` », exit 1 |
| 5 | appel stockage | `let db = sqlite::open("x.db");` | `check` | `E-RES-UNKNOWN` — « unknown name `sqlite` », exit 1 |
| 6 | horloge | `let t = time::now();` | `check` | `E-RES-UNKNOWN` — « unknown name `time` », exit 1 |
| 7 | FFI vers un symbole absent | `extern "C" { fn sqlite3_open_v2(a: i32, b: i32, c: i32, d: i32) -> i32; }` + appel dans `unsafe` | `check` | **exit 0** — la déclaration et l'appel sont *acceptés* (types primitifs) |
| 8 | FFI vers un symbole absent | idem #7 | `build` | **exit 1** — `link.exe` → `LNK2019: unresolved external symbol sqlite3_open_v2` puis `LNK1120: 1 unresolved externals` |
| 9 | attribut de lien | `#[link(name = "sqlite3")] extern "C" { … }` | `check` | `E-UNSUPPORTED-ATTR` — « attributes (`#[...]`) are not supported in Ravum Core 1.0 », exit 1 |
| 10 | échappement de chemin | `let b = fs::exists("../../../Cargo.toml");` | `check` | **exit 0** (aucune restriction, conforme à EFF-6.2) |
| 11 | argv réel de `rustc` | — | wrapper `RAVUM_RUSTC` journalisant ses arguments | `--edition 2021 -C overflow-checks=on -C debuginfo=2 -C opt-level=0 --crate-name <nom> -o <binaire> <gen/main.rs>` |
| 12 | jeu de liens par défaut | — | sortie de `link.exe` (sonde #8) | `kernel32.lib` ×3, `ntdll.lib`, `userenv.lib`, **`ws2_32.lib`**, `dbghelp.lib`, `/defaultlib:msvcrt` |
| 13 | SQLite **en Rust** | `ravum-store` (`rusqlite 0.41`, `features=["bundled"]`) | `cargo test -p ravum-store` | **6/6 PASS** (CRUD, migration v1→v2, refus de schéma plus récent) |
| 14 | surface FFI-sûre | `ffi_safe()` | lecture `resolve.rs:1939` | `i8..i128`, `u8..u128`, `isize/usize`, `f32/f64`, `bool`, `()` — **rien d'autre** (ni pointeur, ni `&[u8]`, ni struct, ni `char`) |

## 2. Les quatre routes possibles, et laquelle est réellement ouverte

Toute capacité nouvelle doit passer par l'une de ces routes. Les nommer évite de
confondre « pas encore codé » et « impossible aujourd'hui ».

| Route | Description | Coût normatif | État mesuré |
|---|---|---|---|
| **A — builtin + prélude** | ajouter module + table de builtins + fonctions `rv_*` dans le prélude | **oui** : nouvelle capacité dans le langage (UNS-9.1, D6) | possible : c'est le mécanisme qui a produit `io`, `fs`, `env`, `math` |
| **B — FFI vers une bibliothèque native** | `extern "C"` + `unsafe` vers `ws2_32`/`sqlite3` | **oui** (D7) *et* extension des types FFI | **fermée** : sonde #8 (pas de lien), #14 (pas de tampon), #9 (pas d'attribut de lien) |
| **C — service hôte hors processus** | un programme Rust expose la capacité, le `.rv` lui parle | non (si le transport existe déjà) | **fermée** : aucun transport en `.rv` — pas de socket, pas de pipe, `std::process` n'expose que `exit` (`stdlib.md` §14) |
| **D — réimplémentation pure en `.rv`** | protocole/fichier implémenté avec `fs` + `io` | non | **ouverte** pour un format de fichiers ; **fermée** pour TCP et pour SQLite (voir §4, §6) |

Conclusion méthodologique : **la seule route non bloquée aujourd'hui est A, et
elle exige une décision humaine.** La route D est la seule qui n'exige aucune
décision, et elle ne peut produire ni TCP ni SQL.

## 3. Le problème de lien, prouvé

### 3.1 Ce que la chaîne fait aujourd'hui

`crates/ravum-lang-cli/src/pipeline.rs:456-479` construit la commande `rustc` et
n'ajoute **rien** d'autre que les options de compilation. Args capturés (sonde
#11) :

```
--edition 2021 -C "overflow-checks=on" -C "debuginfo=2" -C "opt-level=0"
--crate-name dsp_gain_iir -o <…>.exe <…>.ravum/gen/main.rs
```

Il n'y a **pas** :
`-L <dir>` · `-l <lib>` / `-l static=…` · `--extern` · `-C link-arg=…` ·
`--cfg` · de fichier de dépendances · d'étape de compilation de dépendances.

Le `link.exe` invoqué reçoit le jeu par défaut de la CRT Rust (sonde #12). Deux
observations en découlent, toutes deux mesurées :

1. **Un symbole de la CRT/du système est linkable.** `examples/ffi-abs` appelle
   `abs`. Et `ws2_32.lib` **est présent** dans la ligne de lien par défaut : les
   symboles Winsock (`WSAStartup`, `socket`, `connect`, `send`, `recv`, …) sont
   donc *linkables* aujourd'hui depuis un bloc `extern "C"` Ravum.
2. **Un symbole d'une bibliothèque tierce ne l'est pas.** `sqlite3_open_v2` →
   `LNK2019` (sonde #8).

### 3.2 Pourquoi l'attribut de lien ne sauve pas la situation

Rust permet `#[link(name = "sqlite3")]`. Ravum refuse **tous** les attributs
(`E-UNSUPPORTED-ATTR`, sonde #9) ; `#[link]` est explicitement listé comme refusé
dans `spec/language/unsupported.md` §3. Le code généré ne peut donc pas porter
l'information de lien, et même s'il le pouvait, rustc exigerait que
`libsqlite3.lib` soit trouvable dans un chemin de recherche — qu'aucun argument
ne fournit.

### 3.3 Ce qu'il faudrait ajouter, exactement

Pour rendre une bibliothèque native liée, il faut **les quatre** éléments
suivants — aucun n'existe :

| # | Élément manquant | Où cela irait | Blocage |
|---|---|---|---|
| L1 | Un emplacement déclaratif pour nommer la bibliothèque et son chemin (manifeste) | `ravum.toml` + `crates/ravum-lang-sema/src/loader.rs` (validation de manifeste) | aucun `[native]`/`[build]` n'existe ; un build script est `E-MOD-BUILD-SCRIPT` (`unsupported.md` §6) |
| L2 | La propagation vers `rustc` (`-L`/`-l`) | `pipeline.rs:456` | trivial à écrire, **impossible à décider sans L1** |
| L3 | Une politique de résolution (où est le `.lib`/`.a`, comment il est produit, comment il est vérifié) | nouveau : résolution de dépendances natives | absent ; le registre de packages lui-même est `UNSUPPORTED` |
| L4 | Un modèle de confiance (une bibliothèque native est du code non vérifié exécuté dans le processus) | spécification + `unsafe`/`ffi` (D7) | absent : D7 est justement la décision ouverte |

**Constat à retenir** : « lier `sqlite3` » n'est pas une tâche de plomberie de
20 lignes. C'est l'introduction d'un système de dépendances natives et d'un
modèle de confiance, dans un langage qui a explicitement refusé le registre, les
build scripts et les attributs. C'est pourquoi P2-D n'est pas classé
`FIXABLE_NOW`.

### 3.4 Le cas `libsqlite3-sys` (référence Rust)

`ravum-store` prouve que la capacité est disponible **côté Rust** : `rusqlite`
avec `features = ["bundled"]` compile l'amalgame C de SQLite via `cc` et lie le
résultat statiquement (sonde #13 : 6/6 tests). Ce chemin exige un compilateur C,
un build script et un linkage statique — les trois choses que la chaîne Ravum
n'a pas. Le vertical SQLite n'est donc pas « non écrit » : il est **structurellement
hors de portée du pipeline actuel**.

## 4. P2-B — TCP

### 4.1 Décisions normatives requises

| ID | Décision | Référence | Classe |
|---|---|---|---|
| B-D1 | Le groupe d'effet `net` devient-il actif (production possible) ? | EFF-1.1 ; `is_unsupported()` `facts.rs:61` | `AWAITING_HUMAN_DECISION` (D6) |
| B-D2 | Le profil `pure`/`rt` continue-t-il d'interdire `net` ? (oui, par cohérence) | EFF-4.2 / 4.7 | `AWAITING_HUMAN_DECISION` — recommandation : **oui, inchangé** |
| B-D3 | Granularité : un seul groupe `net`, ou séparation écoute / connexion / résolution DNS ? | EFF-1.1 : liste de groupes **fermée** (voir l'écart de rédaction 9/10 signalé en [RFC-P2-RUNTIME-CAPABILITIES.md](RFC-P2-RUNTIME-CAPABILITIES.md) §2) | `AWAITING_HUMAN_DECISION` — recommandation : **un seul groupe `net`**, pas de sous-capacité |
| B-D4 | DNS : la résolution est-elle dans le périmètre (elle est non déterministe et bloquante) ? | — | `AWAITING_HUMAN_DECISION` — recommandation : **non** au premier jet (adresses littérales seulement) |
| B-D5 | Bloquant uniquement, ou API non bloquante ? | CONC-1.1 (mono-thread), `async` refusé | `AWAITING_HUMAN_DECISION` — recommandation : **bloquant + timeouts**, car sans `async` une API non bloquante n'a pas de boucle d'événements |

### 4.2 Surface de code à toucher (route A), mesurée

Pour une seule fonction builtin (par ex. `net::connect`), neuf points d'édition :

| # | Fichier / ancre | Modification |
|---|---|---|
| 1 | `crates/ravum-lang-sema/src/facts.rs:61` | `is_unsupported()` cesse de renvoyer `true` pour `Net` |
| 2 | `crates/ravum-lang-sema/src/builtins.rs:66` | `PRELUDE_MODULES` : ajouter `"net"` |
| 3 | `builtins.rs:69` | `builtin_root_module` : `"net" => "std::net"` |
| 4 | `builtins.rs:83` | `BUILTIN_MODULES` : ajouter `"std::net"` |
| 5 | `builtins.rs:126` | `BUILTIN_FNS` : signature + `effects: &[Effect::Net]` |
| 6 | `crates/ravum-lang-codegen/src/emit.rs` — `enum FreeBuiltin` | nouvelle variante |
| 7 | `emit.rs` — `fn builtin_free` | mapping du chemin |
| 8 | `emit.rs` — `fn builtin_free_call`, `fn method_call_expr` + `crates/ravum-lang-codegen/src/prelude.rs` | émission de l'appel + fonction `rv_net_*` dans le prélude |
| 9 | `spec/language/{effects,stdlib,unsupported,features,diagnostics}.md` + `conformance/cases/` | surface normative + cas positifs/négatifs |

Aucune modification de `effects.rs` : les effets d'un builtin proviennent de la
table (`effects.rs:353`), donc l'inférence et le profil suivent automatiquement.

**Estimation de coût de la route A** : 4 fichiers de code (≈ 120–250 lignes
selon l'API), 5 documents normatifs, ≈ 6–10 cas de conformance, + la décision.
Le coût est **faible** ; c'est la décision qui est chère, pas le code.

### 4.3 Impact sur le modèle d'effets

- Aucun nouveau groupe nécessaire pour TCP : `net` existe déjà.
- L'inférence devient réellement productrice de `net` ; `facts.effects` change de
  contenu → tout outil qui suppose `net` impossible (dont ce dossier) change de
  comportement. C'est pourquoi la surface expérimentale
  ([RFC-P2-EXPERIMENTAL-SURFACE.md](RFC-P2-EXPERIMENTAL-SURFACE.md)) est un
  prérequis : elle permet de prototyper **sans** modifier les tables stables.
- `pure` et `rt` doivent continuer à refuser `net` ; sinon un programme `rt`
  pourrait bloquer sur un `connect`, ce qui contredirait l'intention de EFF-4.7.

### 4.4 Conception de test hermétique

Modèle déjà utilisé dans ce dépôt (à réutiliser tel quel, pas à inventer) :

- `apps/ravum-todo/tests/e2e.rs:42` : **`port: 0`** (port éphémère attribué par
  l'OS) ; `:363` : assertion `server.url().starts_with("http://127.0.0.1:")`.

Règles pour un test TCP Ravum :

1. **Toujours `127.0.0.1` et le port `0`**, jamais un port fixe ; lire le port
   effectif via une API de retour de `bind`.
2. **Aucune dépendance au réseau externe** ; aucun DNS (B-D4) ; si un test exige
   une résolution, il est `NOT_RUN`, pas « vert par chance ».
3. **Borne de durée au niveau du harnais**, pas dans le programme : un test qui
   peut bloquer indéfiniment n'est pas un test. Le programme `.rv` n'ayant aucune
   horloge (RC-6), il ne peut pas s'auto-borner ; c'est le lanceur (runner Rust /
   CLI) qui doit imposer la limite et tuer le processus.
4. **Négatifs obligatoires** : `connect` vers un port fermé → erreur typée, pas
   panic ; deuxième `bind` sur le même port → erreur ; `recv` sur socket fermé →
   `Err`, pas d'interruption du processus.
5. **Zéro allocation sur le chemin chaud** si un profil `rt` est visé — mesurable
   avec l'allocateur compteur de `ravum-dsp-harness` (`src/alloc.rs`), mais ce
   n'est branché que sur des tests Rust, pas sur des programmes `.rv` (RC-5).
6. **Déterminisme** : pas de test dépendant du temps (aucune horloge en Core 1.0)
   → un timeout *testé* exige `time`, donc B-D4/RC-6 avant tout test de timeout.

### 4.5 Classification P2-B

| Sous-problème | Classe |
|---|---|
| Décider d'activer `net` (B-D1…B-D5) | `AWAITING_HUMAN_DECISION` |
| Builtins + prélude TCP (une fois décidé) | `FIXABLE_NOW` (≈ 1 session, 4 fichiers) |
| TCP via FFI/Winsock | **fermée** : `EXTERNAL_DEPENDENCY` sur l'extension des types FFI (pointeurs/tampons), qui est une décision D7 |
| Tests hermétiques (loopback/port 0) | `FIXABLE_NOW` (modèle existant) |
| Timeouts testables | `AWAITING_HUMAN_DECISION` (dépend de l'horloge, RC-6) |

> **Cohérence avec la RFC existante.** [`RFC-PERIMETRE-UNIVERSEL-D6.md`](RFC-PERIMETRE-UNIVERSEL-D6.md)
> §3 Option 2 décrit déjà le « réseau bloquant minimal (`net`), sans async », avec
> conformance hermétique sur `127.0.0.1` et port éphémère, et le recommande comme
> **pilote 1.x**. Ce dossier ne le contredit pas : il ajoute (a) la surface de code
> exacte pour l'implémenter, (b) la preuve du blocage de lien qui interdit la route
> FFI, et (c) le contrat d'isolation qui permettrait de prototyper sans activer
> (§ [RFC-P2-EXPERIMENTAL-SURFACE.md](RFC-P2-EXPERIMENTAL-SURFACE.md)). Les deux
> documents restent `PROPOSED` et aucun n'autorise une implémentation.

## 5. P2-C — HTTP

HTTP/1.1 **n'a pas besoin d'un effet propre** : c'est un protocole au-dessus de
TCP. La seule décision est de savoir si l'on crée un groupe `http` ou si `http`
est une bibliothèque construite au-dessus de `net`. Créer un groupe est coûteux
et contradictoire avec le texte actuel : EFF-1.1 impose un « sous-ensemble des
9 groupes » alors que le tableau de `effects.md` §1 et l'énumération `Effect`
(`facts.rs:15`) en comptent **10** — `io`, `fs`, `alloc`, `panic`, `nondet`,
`rt`, `net`, `unsafe`, `ffi`, `async` — l'écart est dans la rédaction de la
spécification, pas dans le code, et il devra être corrigé avant d'ajouter quoi
que ce soit.
**Recommandation : bibliothèque au-dessus de `net`, aucun nouveau groupe.** Cela
réduit P2-C à « écrire du `.rv` », une fois P2-B fait.

| Sous-problème | Classe | Note |
|---|---|---|
| Décider la forme (groupe vs bibliothèque) | `AWAITING_HUMAN_DECISION` | recommandation ci-dessus |
| Serveur/parseur HTTP en `.rv` | `FIXABLE_NOW` **après** P2-B | le dépôt fournit déjà un modèle Rust minimal à imiter : `apps/ravum-todo/src/http.rs` (une requête par connexion, `Content-Length` seulement, pas de chunked, pas de keep-alive, pas de TLS, limites `MAX_HEADER_BYTES = 16 Kio`, `MAX_BODY_BYTES = 1 Mio`) |
| Client HTTP (sortant) | `FIXABLE_NOW` après P2-B | nécessite en plus une politique de redirection/limites |
| TLS | `EXTERNAL_DEPENDENCY` | aucune crypto dans le langage ; lier OpenSSL est le même problème que §3 (et pire : pas dans le jeu de liens par défaut) |
| En-têtes `Date` (RFC 9110) | bloqué par RC-6 (pas d'horloge) | `AWAITING_HUMAN_DECISION` |
| Tests hermétiques | `FIXABLE_NOW` | même modèle que §4.4, plus : requête surdimensionnée → refus explicite, pas de troncature silencieuse |

**Point d'honnêteté** : sans TLS, un « vertical HTTP » est un serveur HTTP en
clair sur loopback. C'est utilisable et testable, mais cela ne doit pas être
présenté comme « support web » : web/frontend et SQL figurent parmi les objectifs
P0-G (`docs/architecture/PROJECT_INVARIANTS.md`), **pas** dans le périmètre
Core 1.0 (`docs/architecture/RAVUM-CORE-DESIGN.md` §5, `spec/language/unsupported.md` §7).

## 6. P2-D — SQLite

### 6.1 Pourquoi SQLite est le blocage le plus dur

SQLite est une bibliothèque C dont l'amalgame distribué (`sqlite3.c`) est de
l'ordre de 250 000 lignes. Cinq voies examinées, toutes mesurées comme fermées
ou hors de portée du pipeline actuel :

| Voie | Pourquoi elle échoue / ce qu'elle coûte |
|---|---|
| Lier `libsqlite3` | §3 : aucun `-l`, aucun `#[link]`, aucune résolution de bibliothèque native |
| Compiler l'amalgame C dans le binaire | exige un build script + un compilateur C — `E-MOD-BUILD-SCRIPT` (refusé), et le pipeline n'a qu'une seule invocation `rustc` sur un seul fichier |
| Réimplémenter SQLite en `.rv` | hors de portée en Core 1.0 (pas de pointeurs, pas de `unsafe` hors FFI, pas d'I/O binaire — `fs` est texte UTF-8 strict, `spec/language/stdlib.md` §15) |
| Service hôte hors processus | route C fermée : aucun transport en `.rv` (pas de socket, pas de pipe) |
| **Format de stockage propre en `.rv`** | **seule voie ouverte** : un fichier texte/UTF-8 avec un enregistrement par ligne, écrit via `fs::write_string`, lu via `fs::read_to_string` |

### 6.2 Le vertical « stockage » réellement atteignable aujourd'hui

Ce qui est faisable **sans aucune décision** :

- un magasin clé/valeur ou un journal d'enregistrements dans un fichier texte ;
- réécriture complète du fichier à chaque mutation (pas d'append atomique :
  `write_string` est « non atomique, non durable » — `stdlib.md` §15) ;
- versionnage de schéma **lisible dans le fichier** et refus d'un format plus
  récent (le modèle fail-closed de `ravum-store` est transposable en `.rv` sans
  SQL) ;
- tests hermétiques : base temporaire sous `target/`, effacée après le test ;
  jamais de base partagée entre tests.

Une **implémentation de référence** existe depuis le 2026-11-11 :
[`examples/file-store`](../../examples/file-store/src/main.rv), exécutée et
mesurée — voir §7.4, limites comprises.

Ce que ce vertical **n'est pas** : SQL, requêtes, index, transactions,
concurrence, types. Il faut le nommer « stockage », pas « SQLite » — c'est
exactement le genre de glissement que INV-01 interdit.

### 6.3 Classification P2-D

| Sous-problème | Classe |
|---|---|
| Décider d'introduire des dépendances natives (L1–L4) | `AWAITING_HUMAN_DECISION` (D6 + D7 + ADR dédié) |
| Lier `sqlite3` | `EXTERNAL_DEPENDENCY` (dépend de la décision précédente, puis d'un travail de toolchain non trivial) |
| Bindings SQLite (API, types, erreurs) | `DEFERRED` (n'a de sens qu'après le lien) |
| Magasin de fichiers en `.rv` | `FIXABLE_NOW` |
| Horodatage des enregistrements | bloqué par RC-6 (`time` absent) → `AWAITING_HUMAN_DECISION` |

## 7. P2-E — Premier vertical « full-stack »

### 7.1 Définition honnête

Un vertical full-stack minimal = **entrée/sortie + stockage + service + client**,
testé de bout en bout, dans un seul langage. Aujourd'hui :

- entrée/sortie : `IMPLEMENTED` (`io`, `fs`, `env`, `args`) ;
- service réseau : **fermé** (P2-B) ;
- stockage : **fermé** pour SQL (P2-D), **ouvert** pour un format de fichiers ;
- horloge : **fermée** (RC-6).

### 7.2 Chemin critique (ordonné, avec ce qui débloque quoi)

```
[1] Décision D6 : le périmètre Core 1.x inclut-il le réseau ?      (HUMAIN)
        └─ si NON → aucun vertical réseau ; s'arrêter ici, honnêtement.
        └─ si OUI ↓
[2] ADR : forme de l'extension d'effets (net actif ; http au-dessus)  (HUMAIN + spec)
[3] Surface expérimentale : --experimental net, fail-closed           (FIXABLE_NOW, RFC-P2-EXPERIMENTAL-SURFACE)
[4] Builtins + prélude TCP (route A)                                  (FIXABLE_NOW, 4 fichiers)
[5] Horloge monotone + timeouts (RC-6)                                (HUMAIN → puis FIXABLE_NOW)
[6] Bibliothèque HTTP en .rv au-dessus de net                         (FIXABLE_NOW)
[7] Magasin de fichiers .rv (peut démarrer dès maintenant, en //)      (FIXABLE_NOW)
[8] Cas de conformance + tests hermétiques loopback/port 0            (FIXABLE_NOW)
[9] Décision sur les dépendances natives (SQLite/TLS)                 (HUMAIN)
[10] Vertical complet : service HTTP + stockage + client, une commande (après [4][6][7])
```

Le point important : **[7] et [8] ne dépendent d'aucune décision** et peuvent
produire un vertical utile (outil de fichiers + magasin versionné). Tout ce qui
touche au réseau est derrière [1].

### 7.3 Ce qui est déjà un vertical aujourd'hui

| Exemple | Ce qu'il prouve | Limite |
|---|---|---|
| `examples/file-tool` | `fs` + `io` réels, binaire natif | mono-fichier, pas de service |
| `examples/stats` | lecture de fichier + `Vec`/`String` + agrégation | idem |
| `examples/ffi-abs` | FFI vers la CRT | un seul symbole scalaire |
| `examples/file-store` | magasin clé/valeur **versionné** adossé à des fichiers, CLI texte, `Result` de bout en bout | pas une base de données, pas de service, mono-écrivain (voir §7.4) |
| `apps/ravum-todo` (Rust) | HTTP + SQLite + UI, **en Rust** | ce n'est **pas** un programme Ravum ; sert de modèle de test, pas de preuve de capacité `.rv` |

Il n'existe **aucun** vertical Ravum réseau ou SQL. C'est un fait, pas un
jugement.

### 7.4 Implémentation de référence du point [7] : `examples/file-store`

Le seul élément du chemin critique qui ne dépend d'aucune décision a été
**implémenté et exécuté** (2026-11-11) : `examples/file-store` (nouveau paquet,
`ravum.toml` + `src/main.rv`, 21 fonctions, **338 lignes**, `ravum-lang fmt --check` = 0 différence).

**Ce que c'est** : un répertoire contenant un fichier d'index (`<clé>|<version>`)
et un fichier par version (`<clé>.v<N>`). Cinq commandes : `put` (valeur sur
stdin), `get`, `list`, `history`, `delete` (pierre tombale).

**Preuve d'exécution** (`target/debug/ravum-lang.exe`, `check` = 0 erreur,
0 avertissement) :

| Commande | Sortie | Exit |
|---|---|---|
| `run . -- store put greeting` (stdin `hello world`) | `stored greeting v1 (store/greeting.v1)` | 0 |
| `run . -- store get greeting` | `hello world` | 0 |
| `run . -- store put greeting` (stdin `hello again`) | `stored greeting v2 (store/greeting.v2)` | 0 |
| `run . -- store list` | `greeting  v2` / `note  v1` | 0 |
| `run . -- store history greeting` | `v1 store/greeting.v1 present` / `v2 store/greeting.v2 present` | 0 |
| processus **neuf** : `run . -- store get greeting` | `hello again` (les données survivent au redémarrage) | 0 |
| `run . -- store delete note` | `deleted (tombstone: version files are retained)` | 0 |
| `run . -- store get note` | `error: no such key: note` | **1** |
| `run . -- store get ../etc/passwd` | `error: invalid key: …` | **1** |
| `run . -- no-such-dir list` | `error: store directory does not exist: …` | **1** |
| `run .` (aucun argument) | aide, puis sortie | 0 |

Effets inférés (extrait de `check --json`) — c'est aussi une démonstration du
modèle de capacités de [RFC-P2-RUNTIME-CAPABILITIES.md](RFC-P2-RUNTIME-CAPABILITIES.md) :

| Fonction | Effets |
|---|---|
| `valid_key` | `[]` (pure) |
| `current_version`, `field`, `without_key`, `version_path` | `[alloc]` |
| `load_index`, `save_index` | `[fs, alloc]` |
| `cmd_put`, `cmd_get`, `cmd_list`, `cmd_history`, `cmd_delete` | `[io, fs, alloc]` |

`--profile pure` et `--profile rt` refusent le programme (`E-EFF-PROFILE`,
effet `io` atteignable depuis `main`) : le paquet est honnête sur ses capacités.

**Limites, mesurées et non cachées** :

- le répertoire du magasin **doit exister** : Core 1.0 n'a pas de `create_dir`
  (seuls `read_to_string`, `write_string`, `exists`), et le programme le dit
  explicitement dans son message d'erreur ;
- `delete` **n'efface rien** sur le disque (pas de `remove_file`) : le fichier
  `note.v1` est toujours présent après suppression, avec son contenu ;
- **un seul écrivain**, aucun verrou, aucune concurrence (CONC-1.1) ;
- écriture via `fs::write_string` : **non atomique, non durable** (`stdlib.md` §15) —
  aucune revendication de résistance à une coupure de courant ;
- une valeur = **une ligne** (stdin) ;
- **aucun** SQL, aucune requête, aucun index secondaire, aucune transaction :
  appeler ce paquet « base de données » ou « SQLite » serait faux.

**Verdict** : P2-E reste **partiel**. Le point [7] du chemin critique est fait et
prouvé ; [8] (cas de conformance + smoke) est proposé au propriétaire de
`conformance/` et de `scripts/lang-smoke.ps1` ; [1]–[6] et [9]–[10] restent
derrière des décisions humaines. Ce paquet ne rend pas P2 « terminé ».

## 8. Matrice de blocage récapitulative

| # | Blocage | Classe | Coût mesuré / estimé | Débloque |
|---|---|---|---|---|
| BL-1 | Groupe `net` inactif (`E-UNSUPPORTED-EFFECT`) | `AWAITING_HUMAN_DECISION` (D6) | 1 décision ; 1 ligne de code (`facts.rs:61`) | TCP, HTTP |
| BL-2 | Aucun groupe `http` (liste de groupes fermée, voir §5) | `AWAITING_HUMAN_DECISION` | 0 si HTTP est une bibliothèque sur `net` (recommandé) | HTTP |
| BL-3 | Aucun builtin/module réseau | `FIXABLE_NOW` après BL-1 | 9 points d'édition, ≈ 120–250 lignes | TCP |
| BL-4 | Aucune horloge (`time` ABSENT) | `AWAITING_HUMAN_DECISION` | 1 groupe + 1 builtin + tests | timeouts, `Date`, horodatage |
| BL-5 | Aucun timeout/annulation | `AWAITING_HUMAN_DECISION` (ADR-008) | dépend de BL-4 | tests de robustesse |
| BL-6 | Pas de `-L`/`-l`/`#[link]`/build script | `EXTERNAL_DEPENDENCY` (sur BL-7) + `AWAITING_HUMAN_DECISION` (D7) | système de dépendances natives (L1–L4) | SQLite, TLS |
| BL-7 | Modèle de confiance pour du code natif non vérifié | `AWAITING_HUMAN_DECISION` (D7) | décision + spécification | BL-6 |
| BL-8 | Types FFI sans pointeur/tampon | `AWAITING_HUMAN_DECISION` (D7) | décision + règles de sûreté | TCP/SQLite via FFI |
| BL-9 | Aucun transport IPC en `.rv` (route C) | `DEFERRED` | dépend de BL-1 | service hôte hors processus |
| BL-10 | Magasin texte en `.rv` | **fait** le 2026-11-11 : `examples/file-store` (§7.4) ; reste la conformance/smoke | 338 lignes `.rv` livrées + 2 cas proposés au propriétaire | P2-E partiel |
| BL-11 | Tests hermétiques loopback/port 0 | `FIXABLE_NOW` | modèle existant (`e2e.rs:42`) | validation de BL-3 |

## 9. Ce que ce dossier ne fait pas

- Il **n'active rien** : `net` reste refusé, `http`/`sqlite`/`time` restent
  inconnus, et les sondes #1–#6 restent les réponses du compilateur.
- Il **ne planifie pas** de calendrier : les « coûts » sont des surfaces de code
  comptées, pas des délais, parce qu'aucune mesure de vélocité fiable n'existe
  pour ce dépôt.
- Il **ne recommande pas** d'implémenter SQLite : il recommande de **décider**
  d'abord si le langage acquiert un jour une notion de dépendance native, et de
  documenter que, tant que cette décision n'est pas prise, « Ravum + SQLite » est
  hors de portée du pipeline actuel.

## 10. Questions ouvertes (pour le responsable)

- **Q-P2B-1** — `net` doit-il être activé dans Core 1.x, ou un successeur
  (Core 1.1) est-il le bon véhicule ? Recommandation : successeur — le périmètre
  Core 1.0 est déjà `PARTIAL` (T6, T9) et l'ajout de `net` augmente la surface
  normative sans augmenter ce qui est prouvé.
- **Q-P2D-1** — Un magasin de fichiers versionné en `.rv` (BL-10) est-il un
  livrable acceptable pour P2-E, à condition de l'appeler « stockage » et non
  « SQLite » ? Recommandation : oui — c'est le seul vertical full-stack
  atteignable sans décision, et il exerce `fs`/`io`/`String`/`Vec`/`Result`.
  **Suite (2026-11-11)** : recommandation appliquée — le paquet existe, tourne et
  est mesuré (`examples/file-store`, §7.4). La question ouverte restante est de
  savoir si le corpus de conformance l'enregistre comme cas `run-pass`, ce qui
  revient au propriétaire de `conformance/` (deux `case.json` proposés).
- **Q-P2C-1** — Sans TLS, un serveur HTTP en clair a-t-il une valeur produit ?
  Recommandation : oui pour le loopback et l'outillage local, non pour une
  exposition réseau ; à écrire noir sur blanc dans le rapport du vertical.

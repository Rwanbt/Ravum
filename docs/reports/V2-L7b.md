# V2-L7b — TCP et HTTP asynchrones côté serveur

| Champ | Valeur |
|---|---|
| Gate | T6 |
| RFC | RFC-ASYNC-NET-1, V2-R3 et note V2-N1 |
| Branche / SHA de départ / SHA final | `v2/L7b-async-net` / `24d85d61fbb58e9f6a5d944c29fc94767edc03ed` / commit qui ajoute ce rapport |
| SHA des sources testées | `61e368992bddfb4719bafb489d62b2da543dbd09` |
| Outils / OS | rustc `1.95.0 (59807616e 2026-04-14)`, Cargo 1.95.0, Linux x86_64, Intel Core i7-13620H |

## Objectif et périmètre

Avancer sur les opérations ANET qui peuvent utiliser des sockets déjà
connectées. P0-C et P0-D s’appliquent. Aucun nouveau site `unsafe`, aucune
dépendance, aucun changement au chemin de liaison de rustc.

## Modification

- Déclarations, effets et abaissement de `net::read_exact_async`,
  `http::read_request_async`, `http::respond_async` et `task::timeout`.
- Les fonctions HTTP synchrones et asynchrones partagent l’analyse de la
  ligne, des en-têtes et de la longueur, le décodage UTF-8 du corps et le
  format de réponse. Elles lisent le même tampon TCP. La lecture et
  l’écriture asynchrones d’un corps cèdent la main entre les tranches.
- Le sondage du flux utilise `try_lock`. Une opération qui n’a pas accès au
  socket rend la main. Le mode bloquant est rétabli avant de rendre le
  tampon à son propriétaire.
- `timeout` abandonne le futur à l’échéance monotone. À délai nul, il
  sonde une fois. Sous `det`, le scheduler avance son horloge virtuelle
  lorsqu’une attente est signalée. `nondet` rend visible la lecture de
  l’horloge réelle conformément à DET-2.

## Tests positifs / négatifs

| Test | Résultat |
|---|---|
| `blocking_and_async_http_match_and_preserve_buffered_bytes` | Une deuxième tâche avance alors que le corps HTTP est incomplet. Les deux chemins répondent octet pour octet de la même façon. Les lectures exactes suivantes retrouvent tous les octets tamponnés. |
| Même test, refus HTTP | Ligne hors limite, 101 en-têtes, corps déclaré à 16 Mio + 1 octet et UTF-8 invalide. Même code de sortie et même diagnostic sur les deux chemins. |
| Même test, délai réel | La lecture sans données expire à 80,02 ms pour 80 ms demandées. Le même propriétaire relit ensuite le socket et répond avec le contenu reçu après le délai. |
| `deterministic_timeout_polls_once_at_zero_and_virtualizes_ten_minutes` | À délai nul, le futur prêt rend `Some` et le futur en attente rend `None`. Le cas rapide à délai positif rend `Some`. Les dix minutes virtuelles se terminent sans sommeil réel. |

## Commandes exécutées

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p ravum-lang-sema -p ravum-lang-codegen --lib
cargo test -p ravum-lang-cli --test async_net_server --test async_net_timeout -- --nocapture
python3 tools/unsafe_audit.py
python3 tools/must_audit.py
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
cargo run -p ravum-conformance --release -- run
```

## Limites, écarts et demandes hors périmètre

- Le Rust généré est un fichier autonome compilé sans `--extern`.
  L’autorisation V2-N1 sur la licence de `socket2` ne suffit donc pas à le
  rendre utilisable depuis les programmes générés. Le plan de liaison et la
  distribution de l’artefact runtime relèvent d’une décision distincte.
  Une autre possibilité est un adaptateur natif audité et lié avec le
  mécanisme FFI existant. Elle demande une décision de sûreté et une
  qualification multiplateforme.
- `connect_async` conserve son comportement actuel. `http::get_async` et
  `http::post_async` dépendent de la connexion non bloquante. ANET-T1, le
  scénario intégral ANET-T2, ANET-T3 et les cas client ANET-T6 restent
  ouverts. Le test différentiel d’ici porte sur le serveur et le tampon.
- Les opérations réussies partagent leurs octets tamponnés. Le test
  d’annulation expire avant qu’un octet soit reçu. Le sort d’un préfixe
  déjà consommé par un futur annulé nécessite un scénario dédié.
- TSan ANET-T7 `NOT_RUN`. La machine a les toolchains stables seulement.
- La suite workspace est lancée sur un système de fichiers Linux natif,
  dans une copie temporaire avec le correctif du harnais
  `64efd691b2b18fd2e5cb5e86fed98a63ab0e8891`.

## État des preuves

`TESTED` côté serveur, pour la virtualisation du délai et pour les
entrées et mesures décrites. Aucune nouvelle règle testable non liée
n’est ajoutée au comptage de `must_audit.py`.
Workspace avec 2 329 réussites, 0 échec et 14 ignorés. Conformance avec
748 réussites, 0 échec et 2 `SKIP`. La dimension `--verify-build` est
`NOT_RUN`. Formatage, Clippy et contrôle d’inventaire ont réussi.

## Proposition à l’intégrateur

Retenir les preuves serveur et de délai pour T6. La validation complète de
T6 exige encore le chemin de connexion non bloquante, les clients HTTP et
la campagne TSan.

## Historique de la branche d’intégration

## Fait sans attendre la décision : HTTP asynchrone côté serveur (ANET-2)

- Runtime (`prelude.rs`) : l'analyseur HTTP est rendu générique sur
  `BufRead` (`rv_http_parse_request`) : `read_request` et
  `read_request_async` partagent le **même** code, donc les mêmes règles et
  limites (STD-19.2/19.3). `RvReadRequest` accumule les octets sans bloquer
  jusqu'à une requête complète (tête + `Content-Length`), puis l'analyse ;
  `respond_async` réutilise `rv_http_response_text` (même réponse que
  `respond`, STD-19.5) et le futur d'écriture existant.
- Vérificateur : signatures `std::http::read_request_async` et
  `respond_async`, effets `net, async, alloc`. Générateur : appels
  `rv_http_*_async`.
- Spécification : **CONC-6.8** (`concurrency.md`), signatures dans
  `stdlib.md` §19, **F-STD-040** (`experimental`, FEAT-12.5) ; snapshots
  complétés (ajout seul).
- Tests : `run/l7b-async-http-server` — le client `a` n'envoie que la moitié
  de sa requête et attend ; `b` est servi d'abord, puis `a` (avec un
  `read_request` bloquant, le serveur se figerait sur `a`) ;
  `effects/l7b-http-async-effects-declared` (`E-EFF-UNDECLARED` sans
  `alloc`). Conformance 752 : 750 PASS, 0 FAIL, 2 SKIP ; codegen 222/0 ;
  `tools/tests` OK.
- Constat annexe : dans une `async fn`, une boucle (`for`/`while`) contenant
  `.await` **à l'intérieur d'un bras de `match`** est refusée
  (`E-TYP-STATEMENT-VALUE`) alors que la même boucle au niveau du corps est
  acceptée. Contourné dans le cas (fonction `pause`) ; défaut probable du
  checker, non corrigé ici.
- **`task::timeout` (ANET-5) : fait.** `RvTimeout` (prélude) : `Some(v)`
  si le futur se termine avant l'échéance (horloge monotone), sinon `None`
  et le futur est détruit ; l'attente laisse dormir l'exécuteur comme une
  attente d'E/S. Effets `async, alloc, nondet` : **premier producteur de
  `nondet`** (DET-2). Sous `det`, `nondet` est refusé (pas encore d'horloge
  virtuelle : fail-closed). Spécification : CONC-6.9, table `nondet` de
  `effects.md`, F-STD-041. Cas : `run/l7b-task-timeout` (lecture muette →
  `None`, futur rapide → `Some(42)`), `effects/l7b-timeout-is-nondet`
  (`E-EFF-UNDECLARED`), `effects/l1b-det-refuses-timeout` (`E-EFF-PROFILE`,
  accepté sous `core`).
- Mesures (SHA de ce commit) : conformance 755 → 753 PASS, 0 FAIL, 2 SKIP ;
  `ravum-lang-sema` + `-codegen` + `-cli` : 1 531 tests, 0 échec.
- Pas fait : `get_async`/`post_async` (attendent `connect_async`).

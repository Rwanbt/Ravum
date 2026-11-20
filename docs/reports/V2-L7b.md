# V2-L7b — T6 : RFC-ASYNC-NET-1, blocage avant implémentation

| Champ | Valeur |
|---|---|
| Gate | T6 |
| RFC | `RFC-ASYNC-NET-1` (`ACCEPTED`, V2-R3, sans dépendance nouvelle) |
| SHA | `5628e64` |
| État | `connect_async` : **bloqué** sur une décision ; côté serveur **fait** (ci-dessous) |

## Constat

ANET-1 exige un `connect_async` qui ne bloque pas l'exécuteur mono-thread.
Aujourd'hui (`crates/ravum-lang-codegen/src/prelude.rs`,
`rv_net_connect_async`) la connexion est faite par `std::net::TcpStream::connect`,
bloquant, au premier sondage (CONC-6.7).

La bibliothèque standard Rust **n'a pas** de connexion TCP non bloquante :
`TcpStream::connect` et `connect_timeout` bloquent ; `set_nonblocking` ne
s'applique qu'à un socket déjà connecté. Les trois voies restantes violent
chacune une contrainte déjà décidée :

| Voie | Contrainte violée |
|---|---|
| `socket2` (MIT/Apache-2.0) : socket non bloquant puis `connect` qui rend `EINPROGRESS` | **V2-R3** : aucune dépendance nouvelle |
| appel direct `libc::connect` | **`#![forbid(unsafe_code)]`** du Rust généré (ABI-2.4, F-META-024) |
| un thread système par `connect_async`, le futur sonde un canal | alternative **écartée** par la RFC (§4 : contraire à l'exécuteur mono-thread, CONC-1.2) |

`http::get_async`/`post_async` (ANET-2) dépendent de la même connexion.
`read_request_async`, `respond_async` (côté serveur) et `task::timeout`
(ANET-5) ne sont **pas** bloqués et peuvent être faits sans décision.

## Correction (après V2-N1)

`socket2` ne peut pas servir : le Rust généré est compilé par `rustc` comme
un fichier unique, sans aucune crate (`pipeline.rs`, pas de `--extern`).
Le recommander était une erreur de l'agent.

## Décision demandée

1. Autoriser `socket2` (recommandé : licence déjà permise par `deny.toml`,
   surface minimale, pas d'`unsafe` dans le code généré), **ou**
2. accepter un thread auxiliaire borné par `connect_async` (amender §4), **ou**
3. garder `connect_async` bloquant (CONC-6.7 inchangé) et ne faire que le
   côté serveur et `timeout`.

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
- Pas fait : `get_async`/`post_async` (attendent `connect_async`),
  `task::timeout` (ANET-5).

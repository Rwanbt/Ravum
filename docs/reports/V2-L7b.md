# V2-L7b — T6 : RFC-ASYNC-NET-1, blocage avant implémentation

| Champ | Valeur |
|---|---|
| Gate | T6 |
| RFC | `RFC-ASYNC-NET-1` (`ACCEPTED`, V2-R3, sans dépendance nouvelle) |
| SHA | `5628e64` |
| État | **bloqué sur une décision du responsable** ; aucun code modifié |

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

## Décision demandée

1. Autoriser `socket2` (recommandé : licence déjà permise par `deny.toml`,
   surface minimale, pas d'`unsafe` dans le code généré), **ou**
2. accepter un thread auxiliaire borné par `connect_async` (amender §4), **ou**
3. garder `connect_async` bloquant (CONC-6.7 inchangé) et ne faire que le
   côté serveur et `timeout`.

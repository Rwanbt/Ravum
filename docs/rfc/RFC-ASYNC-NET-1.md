# RFC-ASYNC-NET-1 — `connect` non bloquant et HTTP asynchrone

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-ASYNC-NET-1` |
| Statut | **ACCEPTED** — ratifiée par le responsable le 2026-11-20 (V2-R3) ; réponses du §5 points 1 à 3 retenues ; **point 4 (dépendance `mio`/`polling`) non décidé** : l'implémentation garde le sommeil de 1 ms (ANET-4) tant que le responsable ne l'a pas autorisée |
| Date | 2026-11-20 |
| Auteur | agent de développement |
| Gate | T6 (écart « `connect` bloquant et pas d'HTTP asynchrone ») |
| Reprend | [`RFC-E11-NET-HTTP`](RFC-E11-NET-HTTP.md), `spec/language/concurrency.md` §6 (CONC-6.6, CONC-6.7), `spec/language/stdlib.md` §17, §19 |
| Portée | runtime de `std::net`/`std::http` et de l'exécuteur `std::task` (voie `rustc` et ses tests), `spec/language/{concurrency,stdlib}.md` |

> **Statut de preuve.** Rien n'est implémenté. Compatible COMPAT-2.4 :
> toutes les fonctions existantes gardent leur sémantique.

## 1. Problème

L'exécuteur `std::task` est mono-thread (CONC-6.6). `accept_async`,
`read_line_async` et `write_str_async` ne le bloquent jamais, mais :

1. `connect_async` **établit la connexion de façon bloquante** au premier
   sondage (CONC-6.7) : un hôte lent ou injoignable fige toutes les tâches
   pendant tout le délai TCP du système (souvent > 1 min) ;
2. `http::get`, `http::post`, `http::read_request`, `http::respond` n'ont pas
   de forme asynchrone : un serveur asynchrone ne peut pas servir HTTP sans
   bloquer les autres tâches ;
3. aucun délai : une lecture sans fin bloque sa tâche pour toujours
   (STD-17.8 : « ni timeout »).

## 2. Proposition

### 2.1 Surface

```rv
// std::net — additions
pub fn connect_async(addr: &str) -> Future<Result<TcpStream, Error>>       // sémantique révisée (ANET-1)
pub fn read_exact_async(stream: &mut TcpStream, count: usize) -> Future<Result<String, Error>>

// std::http — additions (même sémantique que les formes bloquantes, STD-19)
pub fn read_request_async(stream: &mut TcpStream) -> Future<Result<HttpRequest, Error>>
pub fn respond_async(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> Future<Result<(), Error>>
pub fn get_async(url: &str) -> Future<Result<HttpResponse, Error>>
pub fn post_async(url: &str, content_type: &str, body: &str) -> Future<Result<HttpResponse, Error>>

// std::task — délai
pub fn timeout<T>(ms: u64, f: Future<T>) -> Future<Option<T>>              // forme spéciale, ANET-5
```

Toutes portent `effects(net, async, alloc)` (`timeout` : `async, alloc`) et,
comme CONC-6.7, **n'empruntent pas** leur argument au-delà de l'appel (le
futur partage le socket).

### 2.2 Règles

| ID | Règle proposée | Niveau |
|---|---|---|
| ANET-1 | `connect_async` ouvre un socket **non bloquant** ; la résolution d'adresse se limite aux littéraux IP et à `localhost` (pas de DNS : STD-17 inchangé) ; tant que la connexion n'est pas établie, le futur rend la main. Un échec (refus, injoignable) rend `Err`, jamais une panique. Changement de sémantique de CONC-6.7 sur ce seul point (le résultat est identique, seul le blocage disparaît). | MUST |
| ANET-2 | Les fonctions `http::*_async` ont exactement la sémantique de leur forme bloquante (STD-19.2 à STD-19.8 : limites, UTF-8 strict, `Connection: close`, `https://` → `Err`) ; seul le blocage change. Un même programme donne la même sortie avec l'une ou l'autre forme. | MUST |
| ANET-3 | Formes bloquante et asynchrone partagent le même tampon par socket (CONC-6.7) : aucun octet n'est perdu en passant de l'une à l'autre. | MUST |
| ANET-4 | Aucune fonction `*_async` n'appelle une primitive bloquante du système ; la seule attente de l'exécuteur reste celle de CONC-6.7 (sommeil ≤ 1 ms quand toutes les tâches attendent un socket), remplaçable par un `poll`/`epoll`/`kqueue`/IOCP sans changement observable. | MUST |
| ANET-5 | `task::timeout(ms, f)` rend `Some(v)` si `f` se termine en moins de `ms` millisecondes d'horloge monotone, sinon `None` ; `f` est alors **détruit** (son socket reste ouvert si un autre propriétaire existe, sinon fermé). `ms == 0` sonde une fois. | MUST |
| ANET-6 | Effets : `net`, `async` et `alloc` interdits sous `pure` et `rt` (`E-EFF-PROFILE`) ; `timeout` lit une horloge : elle sera `nondet` sous le profil déterministe (RFC-DETERMINISTIC-PROFILE-1), qui la virtualise. | MUST |
| ANET-7 | TSan : le corpus concurrent (`.github/workflows/tsan.yml`) inclut un serveur et un client asynchrones ; il reste bloquant. | MUST |

### 2.3 Exemple

```rv
async fn serve_one(listener: TcpListener) -> Result<(), Error> effects(net, async, alloc) {
    let mut stream: TcpStream = net::accept_async(&listener).await?;
    let request: HttpRequest = http::read_request_async(&mut stream).await?;
    http::respond_async(&mut stream, 200, "text/plain", http::path(&request).as_str()).await
}
```

(CONC-6.3 : une `async fn` ne lie aucune référence ; `&mut stream` est un
argument d'appel temporaire, permis comme pour `read_line_async`.)

## 3. Tests prévus

- **ANET-T1** `connect_async` vers une adresse qui ne répond pas
  (port filtré simulé) : une autre tâche continue d'avancer pendant
  l'attente (compteur de tours mesuré) — **négatif de régression** de
  CONC-6.7 actuel, qui échoue aujourd'hui.
- **ANET-T2** différentiel : `packages/http-notes` servi en bloquant et en
  asynchrone → mêmes réponses octet à octet sur un script de requêtes.
- **ANET-T3** 100 clients `get_async` concurrents sur un serveur
  asynchrone : toutes les réponses, aucune tâche affamée.
- **ANET-T4** limites STD-19.3 en asynchrone (ligne > 8 KiB, 101 en-têtes,
  corps > 16 MiB) → `Err`.
- **ANET-T5** `timeout` : lecture sans fin → `None` en ≤ ms + 10 ms ;
  opération rapide → `Some`.
- **ANET-T6** négatifs : `connect_async` sous `--profile pure`/`rt` →
  `E-EFF-PROFILE` ; `https://` → `Err` ; nom DNS → `Err`.
- **ANET-T7** TSan vert sur le nouveau corpus.

## 4. Alternatives écartées

- **Un thread par `connect`** : contredit l'exécuteur mono-thread et
  l'interdiction des primitives partagées (CONC-1.2).
- **Exécuteur multi-thread** : coût et non-déterminisme ; Q-CONC-3
  recommandait le coopératif.
- **DNS asynchrone** : dépendance et surface supplémentaires ; hors 1.0
  (STD-17, `stdlib.md` §16 table des refus).
- **Délais par socket** (`set_timeout`) plutôt que `task::timeout` :
  multiplie les API ; `timeout` compose avec tout futur.

## 5. Questions au responsable

1. Accepter le changement de CONC-6.7 sur `connect_async` (ANET-1) ?
2. Ajouter `task::timeout` (ANET-5), seule forme générique spéciale ?
3. Faut-il aussi une boucle `poll`/`epoll` réelle dès la V2 (ANET-4), ou
   garder le sommeil de 1 ms ?
4. Nouvelle dépendance éventuelle (`mio` ou `polling`, MIT/Apache-2.0) pour
   ANET-4 : à autoriser ?

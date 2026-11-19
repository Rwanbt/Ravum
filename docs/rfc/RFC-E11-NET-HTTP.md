# RFC-E11-NET-HTTP — Réseau TCP et HTTP/1.1 dans Core 1.0

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-NET-HTTP` |
| Statut | **`PROPOSED`** — aucune implémentation avant décision du responsable sur §7 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) ; ferme une partie de T6 et de T9 |
| S'appuie sur | [`RFC-P2-VERTICALS-BLOCKERS`](RFC-P2-VERTICALS-BLOCKERS.md) §4–§5 (BL-1, BL-2, BL-3, BL-11), [`RFC-PERIMETRE-UNIVERSEL-D6`](RFC-PERIMETRE-UNIVERSEL-D6.md) option 2 |
| Portée | `spec/language/{effects,stdlib,unsupported}.md`, `crates/ravum-lang-sema/src/facts.rs`, table de builtins, prélude de `ravum-lang-codegen`, conformance |

> **Ce que cette RFC ne promet pas.** Ni TLS, ni HTTP/2, ni exposition sur
> Internet, ni débit ou latence chiffrés (rien n'est mesuré). Un serveur Ravum
> 1.0 est un serveur **en clair, pour le loopback et le réseau local**.

## 1. Choix de route

`RFC-P2-VERTICALS-BLOCKERS` §2 montre que seule la **route A** (builtin +
prélude généré) est ouverte : le Rust émis peut appeler `std::net` sans aucune
dépendance native ni changement de l'édition de liens. Les routes B (FFI
Winsock) et C (service hôte) restent fermées et ne sont pas proposées.

## 2. Effet

`net` est déjà reconnu et refusé (`E-UNSUPPORTED-EFFECT`, `facts.rs`). Proposé :
l'**activer** (BL-1). HTTP est une **bibliothèque** sur `net`, pas un groupe
d'effet (BL-2 : coût 0).

| ID | Règle proposée | Niveau |
|---|---|---|
| NET-1.1 | Toute fonction de `std::net` et `std::http` porte l'effet `net`. | MUST |
| NET-1.2 | `net` est refusé au point d'entrée des profils `pure` et `rt` (`E-EFF-PROFILE`) ; le profil `core` n'interdit aucun effet. | MUST |

## 3. `std::net` (TCP bloquant)

> **Implémenté** (phase réseau d'E11) : `spec/language/stdlib.md` §17,
> FeatureId F-STD-036, cas `run/net-loopback-echo`,
> `run/net-connect-refused-is-err`, `effects/net-refused-under-pure`.
> **HTTP (§4) implémenté** le 2026-11-19 : `stdlib.md` §19 (STD-19.1 à 19.8),
> F-STD-038, cas `run/http-loopback-server`, `run/http-client-errors-are-err`,
> `effects/http-refused-under-pure`, et le vertical `packages/http-notes`
> (HTTP + SQLite, `crates/ravum-lang-cli/tests/http_notes.rs`). Écart : HTTP est
> dans le **runtime**, pas en `.rv` : le texte de Ravum n'a ni `split`, ni
> recherche, ni sous-chaîne (STD-9.11). La requête et la réponse sont des types
> opaques lus par des fonctions d'accès ; les erreurs sont le type `Error`.

```rust
fn listen(addr: &str) -> Result<TcpListener, Error> effects(net, alloc)
fn accept(l: &TcpListener) -> Result<TcpStream, Error> effects(net, alloc)
fn connect(addr: &str) -> Result<TcpStream, Error> effects(net, alloc)
fn read_line(s: &mut TcpStream) -> Result<Option<String>, Error> effects(net, alloc)
fn read_exact(s: &mut TcpStream, n: usize) -> Result<String, Error> effects(net, alloc)
fn write_str(s: &mut TcpStream, text: &str) -> Result<(), Error> effects(net, alloc)
fn local_port(l: &TcpListener) -> u16 effects(net)
```

| ID | Règle proposée | Niveau |
|---|---|---|
| NET-2.1 | Les données lues sont du **texte UTF-8** ; un octet invalide rend `Err`, jamais une substitution. | MUST |
| NET-2.2 | `listen("127.0.0.1:0")` choisit un port éphémère, lisible par `local_port` (tests hermétiques, BL-11). | MUST |
| NET-2.3 | Une erreur système devient `Err(Error)`, jamais une panique. Comme pour `std::fs`, l'erreur est le type prélude `Error` : son message est illustratif et non stable (STD-1.7). Un enum d'erreurs fermé reste possible plus tard sans casser le code qui ne fait que propager. | MUST |
| NET-2.4 | `Listener` et `Stream` sont transférables (RFC-E11-CONCURRENCY CONC-2.1) : un thread par connexion est écrivable. | MUST |
| NET-2.5 | Pas de timeout en 1.0 tant qu'aucune horloge n'existe (BL-4) ; à écrire dans `unsupported.md`. | MUST |

Octets bruts : hors 1.0 (pas de type tampon, BL-8). HTTP/1.1 n'en a pas besoin
pour des corps texte.

## 4. `std::http` (HTTP/1.1 serveur et client, en `.rv` ou en prélude)

```rust
struct Request { method: String, path: String, headers: Vec<(String, String)>, body: String }
struct Response { status: u16, headers: Vec<(String, String)>, body: String }

fn read_request(s: &mut Stream) -> Result<Request, HttpError> effects(net)
fn write_response(s: &mut Stream, r: &Response) -> Result<(), HttpError> effects(net)
fn get(url: &str) -> Result<Response, HttpError> effects(net)   // http:// seulement
```

| ID | Règle proposée | Niveau |
|---|---|---|
| HTTP-1.1 | Le parseur accepte HTTP/1.1 avec `Content-Length` ; `Transfer-Encoding: chunked` en requête rend `Err(HttpError::Unsupported)`. | MUST |
| HTTP-1.2 | Limites fixes : ligne de requête ≤ 8 KiB, 100 en-têtes, corps ≤ 16 MiB ; au-delà, `Err(HttpError::TooLarge)`. | MUST |
| HTTP-1.3 | `get` refuse `https://` avec `Err(HttpError::TlsUnsupported)`. | MUST |
| HTTP-1.4 | Une connexion par requête (`Connection: close`) en 1.0. | MUST |

**Lieu d'implémentation recommandé** : en **`.rv`** au-dessus de `std::net`
quand la surface du langage le permet (parse de lignes, `split`), sinon dans le
prélude Rust. Le premier exerce le langage (dogfood) ; le second est plus court.

## 5. Qualification

- Conformance hermétique : serveur et client dans le même test, port 0, jamais
  d'accès Internet (modèle `e2e.rs`).
- Cas `run-pass` : écho TCP, serveur HTTP qui répond à 3 requêtes puis s'arrête,
  client `get` sur ce serveur.
- Cas `run-fail`/sortie attendue : connexion refusée → `Err`, UTF-8 invalide →
  `Err`, requête trop grande → `TooLarge`.
- `compile-fail` : `net` sous profil `core` ; `https` n'est pas un refus de
  compilation, c'est une erreur d'exécution testée.
- Fuzz : `read_request` sur entrées arbitraires, sans panique (job fuzz smoke).

## 6. Vertical T9 visé

`examples/http-todo` : service HTTP + stockage (RFC-E11-SQLITE), un thread par
connexion, testé de bout en bout en CI sur les trois OS.

## 7. Décisions attendues du responsable

1. Activer `net` (BL-1) avec HTTP en bibliothèque (BL-2).
2. Texte seulement (pas d'octets bruts) en 1.0.
3. HTTP en `.rv` (recommandé) ou en prélude Rust.
4. Pas de TLS en 1.0 : un serveur Ravum est documenté « loopback / LAN ».

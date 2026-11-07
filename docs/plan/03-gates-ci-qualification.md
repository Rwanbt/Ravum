# Politique de CI, gates, tests et qualification

**Attention** : ce dossier ne contient pas encore la CI d'un compilateur réel. Le workflow `docs-check.yml` joint ne contrôle que la documentation. Les gates ci-dessous sont des exigences de future implémentation.

## Gates séquentiels

| Gate | Contenu requis | Verdict no-go |
|---|---|---|
| T0 — spécification | ADR tracées, threat model, objectifs/risques, corpus et critères de preuve | impossibilité de définir les garanties |
| T1 — faisabilité | S1–S5 reproduits, mesures, compromis, alternative B, inconnus documentés | cache false-safe, UAF, RT violation, backend incorrect |
| T2 — compilateur sûr | parser fuzz, types/borrow tests, oracle semantics, SMT statuses corrects, conformance, LSP | erreur mémoire ou fausse preuve confirmé(e) |
| T3 — intégration réelle | hot swap stress, audio RT on-device, WASM+browser, HTTP+DB E2E, simulation de fautes | fail sécurité/contrat/DB ou hot patch incorrect |
| T4 — release qualifiée | multi-OS/arch, binary provenance, SBOM, supply-chain, perf/RT profilées, audit indépendant, CI au SHA | P0/P1 ouvert, tests requis manquants, preuve hors cible |

## Tests obligatoires par zone

| Zone | Unit | Property/fuzz | Model/proof | Integration réelle | Performance |
|---|---|---|---|---|---|
| Lexer/parser/CST | ✓ | fuzz/recovery | roundtrip invariant | LSP overlay | parse p95/RSS |
| Type/borrow/effect | ✓ compile-fail | générateur programmes invalides | type soundness corpus | codegen Rust | check p95 |
| MIR/codegen | ✓ IR | differential/oracle | translation validation ciblée | native binaries | cold/warm/link |
| Contrats/SMT | ✓ encode | mutants/vacuité | known true/false/timeout | toolchains pinned | solve time |
| Cache preuve | ✓ keys | mutations hypothèses | fresh-vs-cached | multi-workspace | hit ratio/RAM |
| Simulateur | ✓ seed | fault matrix | exploration bornée | vrai réseau/DB | throughput |
| Hot patch | ✓ ABI | patch fuzz | invariants migration | multi-thread process | downtime |
| Audio | ✓ DSP | state/denormal tests | effect graph | vrai callback/device | xrun/p99.99 |
| Frontend | ✓ UI | state/type | contract cross-client | Playwright browsers | update/render |
| SQL | ✓ schema | SQL fuzz/migration | invariant DB limité | Postgres+SQLite | query latency |
| Agent API | ✓ JSON schema | hostile agent + stale rev | proof-id integrity | IDE + CLI | payload/cycles |
| Supply chain | ✓ policy | malicious build scripts | provenance verification | independent rebuild | binary size |

## Politique de refus

- `PROVED` ne suffit pas si la propriété concerne un autre SHA ou profil.
- `TESTED` ou `ASSUMED` ne remplace pas `PROVED` pour un contrat strict.
- `UNKNOWN`/`TIMEOUT` sont visibles et bloquants lorsqu'obligation requise.
- Tests flaky : isoler, diagnostiquer et corriger ; **ne pas désactiver silencieusement**.
- Un job externe indisponible peut être contourné seulement avec contrôle équivalent réellement exécuté et approuvé ; signaler le manque.
- Pas de merge ou publication automatique si le gate requis n'est pas vert sur le SHA exact.

## Preuve et attestation par SHA

Chaque gate émet `{sha, target, toolchain, policy_version, tests, obligations, assumptions, artifacts, signatures, timestamps, limitations}`. Les hash d'artefacts associent le binaire à son profil et aux résultats. L'interface UI/CLI affiche distinctement « testé », « prouvé sous hypothèses », « non démontré », « non couvert ».

## Bench performance

Rapports p50/p95/p99, CI noise détecté, comparaison baseline à environnement égal. Une amélioration de 10 % mesurée ne constitue pas une preuve WCET ; afficher intervalle de confiance, variance et méthodologie.

## Critère public de production-ready

Un **profil ciblé** (ex. Linux x86_64 serveur, ou plugin audio Linux) peut être qualifié T4 après revue ; cela n'autorise pas à qualifier le langage entier de « production ready universel ». Publier un support matrix clair, et une liste de garanties exactes/limites connues.

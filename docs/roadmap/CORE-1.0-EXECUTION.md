# Ravum Core 1.0 — index d'exécution (source unique de suivi)

> Document d'index, **pas** un tableau de statut parallèle. La vérité de
> l'avancement est dans les issues GitHub liées ci-dessous, les PR et les
> rapports de preuve. Aucun statut « Done » n'est écrit ici sans lien de preuve.
> Dernière mise à jour : 2026-11-09 (itération langage).

## Jalons et issues canoniques

| Jalon | Issue | Objet | Statut (2026-11-09) |
|---|---|---|---|
| T0 | [#88](https://github.com/Rwanbt/Ravum/issues/88) | Audit, durcissement P0, baseline reproductible | **PASS** — 4 défauts P0 corrigés (208 tests), workspace final 987/0 |
| T1 | [#89](https://github.com/Rwanbt/Ravum/issues/89) | Gate M5 : comparaison A/B/C, choix bootstrap | **PARTIAL** — B mesuré (parité MATCH), **C `NOT_IMPLEMENTED`**, décision non ratifiée |
| T2 | [#90](https://github.com/Rwanbt/Ravum/issues/90) | Spécification candidate Core 1.0 + conformance | **PARTIAL** — spec DRAFT 15 fichiers, conformance 106/107, `features.md` incomplet |
| T3 | [#91](https://github.com/Rwanbt/Ravum/issues/91) | Frontend : lexer, parser, diagnostics, formatter | **PARTIAL** — 69 tests + fuzz 249 k + Miri PASS ; LSP absent |
| T4 | [#92](https://github.com/Rwanbt/Ravum/issues/92) | Typage, ownership, borrows, effets | **PARTIAL** — 133 + 85 tests, finding CRITICAL corrigé ; pas d'audit externe |
| T5 | [#93](https://github.com/Rwanbt/Ravum/issues/93) | Backend natif, ABI, exécution | **PARTIAL** — Windows réel, Linux sur runner CI ; parité debug/release non mesurée |
| T6 | [#94](https://github.com/Rwanbt/Ravum/issues/94) | Runtime, stdlib Core 1.0, FFI | **PARTIAL** — stdlib + FFI primitive `extern "C"`/`unsafe` ; threads/net/async `UNSUPPORTED` |
| T7 | [#95](https://github.com/Rwanbt/Ravum/issues/95) | CLI, packages, LSP, outils | **PARTIAL** — CLI complète + `watch` + WASM WASI ; LSP `UNSUPPORTED` |
| T8 | [#96](https://github.com/Rwanbt/Ravum/issues/96) | Compilation continue, hot reload, collaboration | **PARTIAL** — `watch` = rebuild+restart (pas d'état préservé) ; collaboration non qualifiée |
| T9 | [#97](https://github.com/Rwanbt/Ravum/issues/97) | Applications réelles `.rv` | **PARTIAL** — 9 verticales exécutées (dont FFI et WASM) ; HTTP/SQLite/GPU absents |
| T10 | [#98](https://github.com/Rwanbt/Ravum/issues/98) | Conformance, fuzz, sanitizers, audits | **PARTIAL** — fuzz + Miri + 2 audits internes ; **TSan `NOT_RUN`**, audit externe absent |
| T11 | [#99](https://github.com/Rwanbt/Ravum/issues/99) | Performance, supply chain, distribution | **PARTIAL** — installateur + SBOM + provenance ; pas de signature ni build bit-à-bit |
| T12 | [#100](https://github.com/Rwanbt/Ravum/issues/100) | RC, dogfood, Go/No-Go | **PARTIAL** — RC interne non publiée ; Go/No-Go humain absent |

## Documents de référence (à lire dans cet ordre)

1. [`docs/architecture/RAVUM-CORE-DESIGN.md`](../architecture/RAVUM-CORE-DESIGN.md) —
   décisions techniques candidates, interfaces gelées, périmètre/UNSUPPORTED.
2. [`spec/language/`](../../spec/language/) — spécification candidate (DRAFT).
3. [`docs/reports/T0-AUDIT.md`](../reports/T0-AUDIT.md) — baseline et défauts P0.
4. [`docs/decision/M5-technical-report.md`](../decision/M5-technical-report.md) —
   protocole et résultats de la décision bootstrap.
5. [`docs/architecture/STATUS.md`](../architecture/STATUS.md) — état du socle
   d'orchestration (distinct du langage).
6. `conformance/` — corpus exécutable (source de vérité des règles testées).

## État de reprise indépendant du chat

Un nouvel agent doit pouvoir reprendre avec : ce fichier, le design, le dernier
rapport de conformance (`evidence/conformance-*.json` s'il existe), les issues
ci-dessus, le SHA courant de `dev` et la dernière note Obsidian
`projects/ravum/operations/sessions/`. Aucun résultat ne vit uniquement dans une
conversation.

## Règles de mise à jour

- Un changement d'état se fait dans l'issue (commentaire + preuve : commande,
  sortie, SHA), pas ici.
- Une PR relie : exigence (P0), ADR/RFC proposé, tests positifs/négatifs,
  résultats bruts, SHA, rollback, limites.
- Aucun `PROVED`, `production-ready` ou `stable` sans preuve liée.
- Licence, acceptation des ADR, fusion dans `main` et publication de release
  restent des décisions humaines (`DECISIONS_REQUIRED.md`).

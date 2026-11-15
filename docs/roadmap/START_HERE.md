# Commencer ici — Ravum (mise à jour 2026-11-13)

> **Ce document décrit l'état réel du dépôt, pas un plan à venir.** Il a été
> réécrit le 2026-11-13 : la version précédente affirmait « aucune implémentation
> commencée », ce qui était faux depuis le 2026-11-09 pour la plateforme et le
> 2026-11-12 pour le langage.

## Ce qu'est Ravum aujourd'hui

Deux lignes de travail réelles, qui ne se recouvrent pas encore :

1. **Plateforme Rust** (`crates/ravum-*` hors `ravum-lang-*`) : daemon d'analyse
   incrémentale, graphe d'unités, hot swap natif, hôte DSP sans allocation au
   callback. Elle opère sur du code **Rust**. Elle est sur `dev` et `main`.
2. **Langage Core 1.0** (`crates/ravum-lang-{syntax,sema,codegen,cli,lsp}`,
   binaire `ravum-lang`) : chaîne `.rv` → parse → resolve → typeck → own →
   effects → codegen Rust → `rustc`. Elle est **fusionnée dans `main` depuis le
   2026-11-12** (PR #101 puis #104), sans tag ni release. La cible décidée est
   **D2 « production ready »**, pas un MVP ; l'audit externe (T4) et le Go/No-Go
   (T12) restent des décisions humaines.

## Par où commencer

1. [`AGENTS.md`](../../AGENTS.md) — mandat permanent. Sa révision du 2026-11-13
   est une **proposition `PENDING_HUMAN_APPROVAL`** : elle corrige la phrase
   « pas encore un compilateur fonctionnel ».
2. [`DECISIONS_REQUIRED.md`](../../DECISIONS_REQUIRED.md) §0 — décisions déjà
   prises par le responsable (D2, D3, D10…).
3. [`docs/architecture/STATUS.md`](../architecture/STATUS.md) — état réel, deux
   lignes séparées.
4. [`docs/roadmap/CORE-1.0-GATE-MATRIX.md`](CORE-1.0-GATE-MATRIX.md) — registre
   canonique des gates T0–T12 ; les états ne se changent qu'avec commande + SHA.
5. [`docs/reports/GOAL-BASELINE.md`](../reports/GOAL-BASELINE.md) — chiffres de
   référence mesurés le 2026-11-13 et seuils de non-régression.
6. [`docs/reports/LANGUAGE-CORE-STATUS.md`](../reports/LANGUAGE-CORE-STATUS.md) —
   état courant du langage, historique des passes en annexe.

## Reproduire la base de référence

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets -- --test-threads=2
cargo run -p ravum-conformance -- run --verify-build
pwsh -File scripts/lang-smoke.ps1 -SkipBuild
python tools/validate_repo_docs.py
python tools/report_consistency.py
python tools/registry_audit.py
python tools/features_coverage.py --strict
```

Les résultats attendus (2026-11-13, `c931fdf`) sont dans `GOAL-BASELINE.md`.
Aucun chiffre ne doit régresser ; un `SKIP` ou un `NOT_RUN` n'est jamais un
`PASS`.

## Organisation des travaux

- Branches de tâche **depuis `dev`**, PR **vers `dev`**. Le chantier en cours
  vit sur `goal/production-ready` (partie de `test/coverage-batch-1` pour ne pas
  perdre le lot couverture, absent de `dev`).
- Une même frontière de module a un propriétaire de travail pour une PR donnée.
- Aucun test supprimé ou affaibli, aucun `PROVED`, aucune ratification d'ADR,
  aucun tag, aucune release sans décision humaine.
- Toute règle nouvelle ou modifiée passe par une RFC `docs/rfc/` (statut
  `PROPOSED`) et reste `PENDING_HUMAN_APPROVAL`.

## Historique — première vague M0 (conservée telle quelle)

La liste d'origine des premières PR de la vague M0 (issues #24, #26, #27, #28,
#29, #32, #33, #36) et les gates M0–M5 sont conservés dans
[`docs/roadmap/MVP.md`](MVP.md) et [`docs/roadmap/BACKLOG.md`](BACKLOG.md).
Ils décrivent l'itération MVP-0a ; ils ne sont plus l'ordre du jour du langage
Core 1.0, dont le registre est `CORE-1.0-GATE-MATRIX.md`.

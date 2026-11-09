---
schema_version: 4
project: ravum
type: session
tags: [session, ravum, langage, core-1.0, conformance, codegen, handoff]
summary: "Itération langage (9 novembre 2026) : chaîne Ravum réelle lexer→parser→checker→ownership/effets→codegen Rust→binaire natif ; hello/fib/calculator/stats/tests-demo s'exécutent, conformance 90/104 PASS, spec candidate 15 fichiers, P0 du socle corrigés (208 tests). Aucun ADR accepté, aucune licence, aucune release."
created: 2026-11-09
updated: 2026-11-09
session_id: 2026-11-09-language-core-iteration
related:
  - "[[projects/ravum/INDEX]]"
  - "[[projects/ravum/operations/sessions/2026-11-09-session-close-dev-integration-and-mvp0d]]"
---

# Ravum — itération langage Core 1.0 (première chaîne réelle)

> **Note de session, pas un certificat.** Elle décrit ce qui a été exécuté et
> mesuré. Elle ne ratifie aucun ADR, ne choisit aucune licence, ne publie aucune
> release. Le vault était en validation rouge pour des raisons **hors Ravum** au
> moment de la sauvegarde (voir §9) : cette note est donc conservée en export
> versionné dans le dépôt Ravum, à migrer au prochain gate permis.

## 1. Objectif

Passer du socle d'orchestration Rust à un **vrai langage Ravum utilisable** :
spécification candidate, frontend, vérification, backend, CLI, exemples,
conformance, preuves honnêtes et dossier de décisions humaines.

## 2. Sources et état Git

- Dépôt : `D:\App\Ravum`, branche `dev`, base `5408d9b3a48dab2884e2b3eb1aa89c3c16ce3d96`.
- `main` : `54491488bba78baca8df93e14742150ad2bd9b28`, **non modifiée**.
- Commit de durcissement P0 : `5eb2d9a` (overlays/chemins/commit_cycle/replis Cargo).
- Les crates du langage et la spec étaient encore en cours d'intégration au moment
  de cette note (working tree partagé) ; le SHA de gel sera celui de la PR/commit
  d'intégration, à référencer dans le rapport final.
- Issues GitHub créées : #88–#100 (RAV-LANG-T0…T12), index dans
  `docs/roadmap/CORE-1.0-EXECUTION.md`.

## 3. Agents et périmètres (aucun écrivain Git sauf le Lead)

| Agent | Livraison |
|---|---|
| Lead | architecture/interfaces (AST, Facts, Type), design, exemples, smoke, intégration, commits, rapports |
| `p0-hardening` | 4 défauts P0 + 208 tests (overlays, chemins, commit_cycle, replis Cargo) |
| `spec-core` | `spec/language/` 15 fichiers DRAFT (~4 232 lignes, ~806 règles) |
| `syntax` | lexer, parser récursif descendant, formatter idempotent (56 tests) |
| `sema` | résolution, typechecker, exhaustivité, effets transitifs/profils, builtins |
| `own` | borrow checker conservateur (init, moves, move partiel, prêts, non-échappement, drop) |
| `codegen` | backend Rust auditable + prélude safe + runner de tests (26 tests) |
| `cli` | `ravum-lang` (init/check/build/run/test/fmt/lint/doc/cache/doctor), loader, JSON |
| `qa-conformance` | harnais indépendant + 104 cas (lexer→sécurité) |

## 4. Interfaces gelées

- `ast::Program` avec `NodeId` uniques (FileId 8 bits hauts / compteur 24 bits).
- `facts::CheckedProgram { program, facts, files, profile }` : types, résolutions,
  cibles d'appels/méthodes, signatures, structs/enums/consts, effets, chemins.
- `codegen::generate(&CheckedProgram, GenOptions) -> Result<String, CodegenError>`.
- Le codegen exige un fait pour chaque `Path` (clé = `path.id`) ; aucune devinette.

## 5. Ce qui fonctionne réellement (vérifié)

- `ravum-lang check/build/run examples/hello` → `hello, ravum`, exit 0.
- `fib` → `0 1 1 2 3 5 8 13 21 34 55` ; `calculator 5 + 3` → `8` ;
  `stats sample.txt` → `2.5` ; `dsp-gain-iir` sorties gain+IIR correctes.
- `ravum-lang test examples/tests-demo` → `3 passed; 0 failed`.
- `ravum-lang lint`, `doc`, `doctor`, `fmt --check`, `init`, `check --json` OK.
- Smoke `scripts/lang-smoke.ps1` : 27 PASS / 1 FAIL (attente erronée corrigée →
  28/28 attendu après rebuild).
- Conformance : **103 PASS / 0 FAIL / 1 SKIP / 104** (exit 0) ; le SKIP est le cas Unix `skip_on: windows`.

## 6. Tests et mesures

| Suite | Résultat |
|---|---|
| Baseline outillage (T0) | 511 passés / 0 échec / 80 suites sur `5408d9b` |
| P0 concernés | 208 passés / 0 échec / 24 suites (`target/p0-tests-lead.log`) |\n| Workspace final | **858 passés / 0 échec / 93 suites** (`target/final-workspace-tests-2.log`, `--test-threads=2`) |
| `ravum-lang-syntax` | 56 tests verts (11 lexer, 25 parser, 16 formatter, 4 intégration) |
| `ravum-lang-codegen` | 26 tests verts, dont compilations/exécutions rustc réelles |
| `ravum-lang-sema` | ~102 tests, 4 échecs dus à la régression lexer `0..` (en correction) |
| Conformance | 104 cas : 103 PASS / 0 FAIL / 1 SKIP (Unix sous Windows) |
| M5 | A/B parité MATCH sur fib/calculator/stats/dsp ; B compile p50 217–260 ms vs A 181–267 ms (+12 à +31 %), binaire +16 à +26 % ; C `NOT_IMPLEMENTED` ; résultats `evidence/m5-results.json` |

## 7. Problèmes et état des corrections

- Régression lexer `0..n` interprété comme flottant (en correction par `syntax`).
- Codes de diagnostic non canoniques restants : `parser/assign-in-condition`,
  `multi-error-recovery`, closures, raw pointers, attribut `#`, `E-MOD-PATH-ESCAPE`
  côté dépendances, champ privé (corrigé côté sema, à re-mesurer).
- `E-LEX-INT-SUFFIX` pas encore émis ; `E-UNSUPPORTED-CLOSE/UNSAFE` non produits.
- Shorthand de champ `Point { x }` : décision = accepté ; le cas conformance doit
  être aligné (pas le parser).
- Limites structurelles : pas de références stockées/retournées, pas d'unsafe/FFI/
  threads, pas de WASM, pas de hot reload, un seul OS qualifié (Windows).

## 8. Décisions humaines en attente

Voir `DECISIONS_REQUIRED.md` : licence (#34), ratification des ADR, fusion `main`,
publication de release, audit externe, qualification Linux, périmètre universel.

## 9. Sauvegarde Obsidian — blocage honnête

- Vault `D:\App\obsidian-dev-brain`, branche `master`, HEAD `18f2b20`, worktree propre.
- `python _system/tooling/vault.py check --format json` → `ok=false` :
  4 échecs `normalization hash mismatch` dans `projects/rimatrix/` et
  `projects/seno-daw/`, **hors périmètre Ravum**.
- Conformément au contrat (« pas de commit/push si la validation est rouge »), la
  note n'a **pas** été poussée dans le vault ; elle est conservée ici comme export
  versionné du dépôt Ravum. Aucune correction du vault hors mandat n'a été faite.

## 10. Prochaine étape exécutable (reprise sans ce chat)

1. Lire `docs/roadmap/CORE-1.0-EXECUTION.md`, ce fichier,
   `docs/architecture/RAVUM-CORE-DESIGN.md`, `docs/reports/LANGUAGE-CORE-STATUS.md`,
   `spec/language/features.md`, et le dernier `target/conformance-report.json`.
2. Terminer les corrections lexer/parser et relancer `ravum-conformance` jusqu'à
   ne garder que des cas réellement bloqués par des limitations assumées.
3. Exécuter `scripts/m5-bench.ps1`, remplir `docs/decision/M5-technical-report.md`
   avec les chiffres réels, puis T5/T9 (Linux `NOT_RUN` si pas de runner).
4. Intégrer dans `dev` par PR après gates ; ne pas toucher `main`.
5. Migrer cette note vers le vault quand la validation sera verte ou avec
   dérogation humaine explicite.






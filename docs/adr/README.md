# Registre des décisions d'architecture (ADR)

**Tous les ADR sont `PROPOSED` au 23 octobre 2026** : aucun ne prétend à une approbation de l'utilisateur. Les décisions seront soumises à expériences falsifiables puis revue. Les nombres ADR sont stables ; ne pas renuméroter.

| ADR | Sujet | Statut | Dépendances |
|---|---|---|---|
| [ADR-000](docs/adr/ADR-000-perimetre-universel-et-garanties-conditionnelles.md) | Périmètre universel et garanties conditionnelles | PROPOSED | Aucune |
| [ADR-001](docs/adr/ADR-001-implementation-initiale-du-compilateur-en-rust.md) | Implémentation initiale du compilateur en Rust | PROPOSED | ADR-000 |
| [ADR-002](docs/adr/ADR-002-modele-memoire-ownership-emprunts-et-regions.md) | Modèle mémoire : ownership, emprunts et régions | PROPOSED | ADR-001 |
| [ADR-003](docs/adr/ADR-003-erreurs-panic-et-defaillance-a-lexecution.md) | Erreurs, panic et défaillance à l’exécution | PROPOSED | ADR-002 |
| [ADR-004](docs/adr/ADR-004-contrats-raffinements-et-invariants.md) | Contrats, raffinements et invariants | PROPOSED | ADR-003 |
| [ADR-005](docs/adr/ADR-005-systeme-deffets-et-capacites.md) | Système d’effets et capacités | PROPOSED | ADR-002, ADR-004 |
| [ADR-006](docs/adr/ADR-006-classes-temps-reel-et-certification-de-delai.md) | Classes temps réel et certification de délai | PROPOSED | ADR-005 |
| [ADR-007](docs/adr/ADR-007-arithmetique-entiers-flottants-et-fast-math.md) | Arithmétique : entiers, flottants et fast-math | PROPOSED | ADR-003, ADR-004 |
| [ADR-008](docs/adr/ADR-008-concurrence-synchronisation-et-annulation.md) | Concurrence, synchronisation et annulation | PROPOSED | ADR-002, ADR-005 |
| [ADR-009](docs/adr/ADR-009-frontieres-unsafe-ffi-et-code-de-confiance.md) | Frontières unsafe, FFI et code de confiance | PROPOSED | ADR-001, ADR-004 |
| [ADR-010](docs/adr/ADR-010-frontend-et-ir-semantique-multi-niveaux.md) | Frontend et IR sémantique multi-niveaux | PROPOSED | ADR-002, ADR-003, ADR-004 |
| [ADR-011](docs/adr/ADR-011-syntaxe-progressive-et-parsing-dedition.md) | Syntaxe progressive et parsing d’édition | PROPOSED | ADR-010 |
| [ADR-012](docs/adr/ADR-012-compilation-incrementale-a-requetes.md) | Compilation incrémentale à requêtes | PROPOSED | ADR-010, ADR-011 |
| [ADR-013](docs/adr/ADR-013-cache-de-preuves-et-identite-de-contexte.md) | Cache de preuves et identité de contexte | PROPOSED | ADR-012 |
| [ADR-014](docs/adr/ADR-014-protocole-de-preuve-statuts-et-confiance.md) | Protocole de preuve, statuts et confiance | PROPOSED | ADR-004, ADR-013 |
| [ADR-015](docs/adr/ADR-015-solveurs-et-moteurs-de-verification.md) | Solveurs et moteurs de vérification | PROPOSED | ADR-010, ADR-014 |
| [ADR-016](docs/adr/ADR-016-model-checking-fuzzing-et-simulation-deterministe.md) | Model checking, fuzzing et simulation déterministe | PROPOSED | ADR-008, ADR-015 |
| [ADR-017](docs/adr/ADR-017-codegen-initial-rust-sur-comme-backend-de-confiance-limite.md) | Codegen initial : Rust sûr comme backend de confiance limité | PROPOSED | ADR-010, ADR-012 |
| [ADR-018](docs/adr/ADR-018-backends-natifs-cranelift-et-llvm.md) | Backends natifs : Cranelift et LLVM | PROPOSED | ADR-017 |
| [ADR-019](docs/adr/ADR-019-daemon-permanent-isolation-et-budgets.md) | Daemon permanent, isolation et budgets | PROPOSED | ADR-012 |
| [ADR-020](docs/adr/ADR-020-compilations-a-la-frappe-et-source-incomplete.md) | Compilations à la frappe et source incomplete | PROPOSED | ADR-011, ADR-019 |
| [ADR-021](docs/adr/ADR-021-hot-swap-atomique-et-versionnement-du-code.md) | Hot swap atomique et versionnement du code | PROPOSED | ADR-019, ADR-020 |
| [ADR-022](docs/adr/ADR-022-migration-detat-rollback-et-effets-irreversibles.md) | Migration d’état, rollback et effets irréversibles | PROPOSED | ADR-021 |
| [ADR-023](docs/adr/ADR-023-runtime-minimal-et-profils-sans-runtime.md) | Runtime minimal et profils sans runtime | PROPOSED | ADR-000, ADR-002 |
| [ADR-024](docs/adr/ADR-024-thread-audio-et-bascule-dsp-aux-frontieres-de-bloc.md) | Thread audio et bascule DSP aux frontières de bloc | PROPOSED | ADR-006, ADR-021, ADR-022 |
| [ADR-025](docs/adr/ADR-025-frontend-full-stack-et-composants-webassembly.md) | Frontend full-stack et composants WebAssembly | PROPOSED | ADR-023, ADR-005 |
| [ADR-026](docs/adr/ADR-026-schemas-et-migrations-de-bases-de-donnees.md) | Schémas et migrations de bases de données | PROPOSED | ADR-025 |
| [ADR-027](docs/adr/ADR-027-api-machine-pour-les-agents-et-les-ide.md) | API machine pour les agents et les IDE | PROPOSED | ADR-014, ADR-019 |
| [ADR-028](docs/adr/ADR-028-autorite-des-contrats-et-changement-ia-verifie.md) | Autorité des contrats et changement IA vérifié | PROPOSED | ADR-027 |
| [ADR-029](docs/adr/ADR-029-security-by-default-et-sandbox-du-build.md) | Security-by-default et sandbox du build | PROPOSED | ADR-005, ADR-009 |
| [ADR-030](docs/adr/ADR-030-reproductibilite-provenance-et-artefacts.md) | Reproductibilité, provenance et artefacts | PROPOSED | ADR-029 |
| [ADR-031](docs/adr/ADR-031-ci-en-gates-et-qualification-exacte-au-sha.md) | CI en gates et qualification exacte au SHA | PROPOSED | ADR-014, ADR-029 |
| [ADR-032](docs/adr/ADR-032-performance-et-budgets-de-construction.md) | Performance et budgets de construction | PROPOSED | ADR-012, ADR-018 |
| [ADR-033](docs/adr/ADR-033-telemetrie-confidentialite-et-donnees-locales.md) | Télémétrie, confidentialité et données locales | PROPOSED | ADR-027, ADR-029 |
| [ADR-034](docs/adr/ADR-034-ergonomie-lsp-debug-et-navigation-des-preuves.md) | Ergonomie, LSP, debug et navigation des preuves | PROPOSED | ADR-011, ADR-027 |
| [ADR-035](docs/adr/ADR-035-certificats-de-preuve-et-trusted-computing-base.md) | Certificats de preuve et Trusted Computing Base | PROPOSED | ADR-014, ADR-018, ADR-030 |
| [ADR-036](docs/adr/ADR-036-interop-gpu-et-accelerateurs-specialises.md) | Interop GPU et accélérateurs spécialisés | PROPOSED | ADR-007, ADR-024 |
| [ADR-037](docs/adr/ADR-037-gestion-des-paquets-plugins-macros-et-ecosysteme.md) | Gestion des paquets, plugins, macros et écosystème | PROPOSED | ADR-028, ADR-029 |
| [ADR-038](docs/adr/ADR-038-ordre-des-plateformes-et-cout-de-portabilite.md) | Ordre des plateformes et coût de portabilité | PROPOSED | ADR-000, ADR-023 |
| [ADR-039](docs/adr/ADR-039-validation-de-loptimisation-et-compatibilite-de-versions.md) | Validation de l’optimisation et compatibilité de versions | PROPOSED | ADR-007, ADR-018, ADR-035 |
| [ADR-040](docs/adr/ADR-040-diagnostics-structures-audit-et-confidentialite-du-retour-ia.md) | Diagnostics structurés, audit et confidentialité du retour IA | PROPOSED | ADR-027, ADR-032 |
| [ADR-041](docs/adr/ADR-041-gouvernance-open-source-et-gestion-des-changements.md) | Gouvernance open source et gestion des changements | PROPOSED | ADR-030 |

## Politique ADR

1. `PROPOSED` → expérience nommée, résultats, comparaison des alternatives.
2. `ACCEPTED` exige revue explicite, SHA exact et impact documenté.
3. Nouvelle information : nouvel ADR qui supersède l'ancien, jamais réécriture silencieuse.
4. Une décision de langage ou sécurité exige une revue indépendante.
5. Les interfaces normatives de contrats ne sont jamais assouplies par un agent autonome.

Modèles : [`../../templates/ADR-template.md`](templates/ADR-template.md) et [`../../templates/RFC-template.md`](templates/RFC-template.md).

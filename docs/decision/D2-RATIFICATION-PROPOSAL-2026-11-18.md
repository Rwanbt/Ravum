# D2 — liste de ratification proposée (2026-11-18)

> **Statut : `PROPOSED`.** Étape E8 du plan de production. Ce document **ne
> ratifie rien** : `INV-13` et `AGENTS.md` interdisent l'auto-acceptation. Il
> confronte les « preuves à produire avant acceptation » de chaque ADR aux
> preuves présentes sur `dev` et propose un verdict au responsable, qui seul
> passe un ADR à `ACCEPTED`, `REJECTED` ou `DEFERRED`.
>
> Base : `dev` à `66f9104`. Les 42 ADR sont `PROPOSED`
> (`tools/validate_repo_docs.py`).

## Verdicts

- **A — prête à ratifier** : chaque preuve exigée par l'ADR existe pour le
  périmètre Core 1.0, avec commande, résultat et SHA dans le document cité.
- **B — partielle** : une partie des preuves existe ; la colonne « Manque »
  nomme ce qui reste. Ratifier en l'état reviendrait à accepter sans preuve.
- **C — non démarrée** : la fonctionnalité n'existe pas encore, ou relève de
  l'étape E11 (périmètre élargi, [D-18-2](DECISIONS-2026-11-18.md)).

Aucun verdict A ne remplace l'audit externe (T4, T10) : un ADR ratifié dit
que la décision est prouvée, pas que le code est audité.

## A — prêtes à ratifier (4)

| ADR | Preuves présentes |
|---|---|
| [ADR-002](../adr/ADR-002-modele-memoire-ownership-emprunts-et-regions.md) Ownership, emprunts | Corpus `compile-fail` d'ownership et d'emprunts ; contrat `check ⇒ build` bloquant, périmètre 222/222 ([CONFORMANCE-BUILD-CONTRACT](../qualification/CONFORMANCE-BUILD-CONTRACT.md)) ; oracle différentiel 6 000 programmes, 0 divergence ([T2-FINDINGS §30](../qualification/T2-FINDINGS.md)) |
| [ADR-017](../adr/ADR-017-codegen-initial-rust-sur-comme-backend-de-confiance-limite.md) Backend Rust sûr | Oracle différentiel (même campagne) ; test de puissance 13/13 ; S7 (Rust généré identique octet pour octet) ; contrat écrit [RFC-D2](../rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md) |
| [ADR-031](../adr/ADR-031-ci-en-gates-et-qualification-exacte-au-sha.md) CI en gates | Jobs requis réels sur trois OS ; gates de couverture, Clippy, `cargo-deny`, conformance bloquants ; preuves liées au SHA dans [CORE-1.0-GATE-MATRIX](../roadmap/CORE-1.0-GATE-MATRIX.md) |
| [ADR-001](../adr/ADR-001-implementation-initiale-du-compilateur-en-rust.md) Compilateur en Rust | Fuzz du parseur sur corpus hostile ([FUZZ-LEX-PARSE](../qualification/FUZZ-LEX-PARSE.md)) ; Miri en CI sur syntax et sema ; CI Linux, macOS et Windows ; mesures de [PERFORMANCE](../qualification/PERFORMANCE.md) |

## B — partielles (16)

| ADR | Manque |
|---|---|
| ADR-003 Panic et défaillances | Preuve que les profils strict et temps réel n'introduisent aucun chemin de panic |
| ADR-005 Effets et capacités | Cas de fuite de secret vers le frontend, d'escalade d'effet, d'appel bloquant indirect |
| ADR-007 Arithmétique | Suite IEEE NaN/Inf/-0 différentielle multi-architecture ; contrats d'erreur en ULP |
| ADR-009 Unsafe et FFI | Revue externe ; sanitizers (TSan est `NOT_APPLICABLE`, ASan non exécuté) |
| ADR-010 Frontend et IR | Tests de préservation de sémantique entre niveaux d'IR (une seule IR existe) |
| ADR-011 Parsing d'édition | Fuzz d'édition à chaque position ; tests d'interruption de frappe |
| ADR-012 Compilation incrémentale | Invalidations adversariales et benchmarks RAM (l'oracle froid/chaud existe) |
| ADR-027 API machine | Mutation des champs du contrat JSON ; clients autres que VS Code |
| ADR-029 Sécurité du build | Exfiltration simulée et masquage des secrets (chemins hostiles et symlinks couverts par T0) |
| ADR-030 Reproductibilité | Double build sur deux machines contrôlées (le build à deux racines est prouvé) |
| ADR-032 Performance | p95/p99 au repos, RSS, comparaison avec d'autres chaînes |
| ADR-034 Ergonomie LSP | Évaluation sur tâches concrètes, accessibilité clavier |
| ADR-038 Ordre des plateformes | Smoke sur machines réelles hors CI ; paquet reproductible publié |
| ADR-040 Diagnostics structurés | Re-localisation des diagnostics après édition ; preuve de non-exposition des secrets |
| ADR-041 Gouvernance | Test de protection de `main` ; relecteurs indépendants sur le code critique |
| ADR-037 Paquets et écosystème | Dépendances hostiles, semver, signature (la résolution locale déterministe existe) |

## C — non démarrées (22)

ADR-000 (périmètre universel : exige un programme audio `no_std` et un
serveur web, E11), ADR-004 (contrats et raffinements), ADR-006 (temps réel,
E11), ADR-008 (concurrence, E11), ADR-013, ADR-014, ADR-015, ADR-016 (preuves,
solveurs, simulation déterministe), ADR-018 (backends natifs Cranelift/LLVM),
ADR-019, ADR-020 (daemon, compilation à la frappe), ADR-021, ADR-022 (hot swap
et migration d'état, E11), ADR-023 (profil sans runtime), ADR-024 (DSP, E11),
ADR-025 (frontend web, E11), ADR-026 (bases de données, E11), ADR-028 (autorité
des contrats face à l'IA), ADR-033 (télémétrie), ADR-035 (certificats de
preuve), ADR-036 (GPU), ADR-039 (validation d'optimisation).

## Proposition au responsable

1. Ratifier les 4 ADR **A** (`ACCEPTED`), après lecture intégrale.
2. Garder les 16 ADR **B** en `PROPOSED` ; chaque ligne « Manque » devient une
   issue.
3. Pour les 22 ADR **C** : `DEFERRED` pour ceux hors de la 1.0 élargie
   (ADR-004, 013–016, 018, 023, 028, 033, 035, 036, 039) ; `PROPOSED` pour ceux
   que l'étape E11 doit prouver (ADR-000, 006, 008, 019–022, 024–026).

Le verdict **T1** (choix du backend ratifié) dépend du seul ADR-017, avec le
contrat RFC-D2.

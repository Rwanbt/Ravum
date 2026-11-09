# Décisions humaines indispensables — Ravum Core 1.0

> Ces décisions **n'appartiennent pas** aux agents. Ce dossier les rend explicites,
> avec recommandation, preuves disponibles, alternatives et conséquence d'une
> non-décision. Aucune n'est prise ici. Dernière mise à jour : 2026-11-09.

## D1 — Licence du code et gouvernance (#34) — **RÉSOLUE**

- **Décision appliquée** : **`MIT OR Apache-2.0`** (double licence, au choix du
  preneur). Fichiers [`LICENSE-MIT`](LICENSE-MIT) et
  [`LICENSE-APACHE`](LICENSE-APACHE) à la racine ; `license =
  "MIT OR Apache-2.0"` dans `[workspace.package]`, hérité par **les 23 membres**
  (`cargo metadata --locked`, SBOM régénéré : 23/23 `MIT OR Apache-2.0`).
- **Origine** : décision du responsable, appliquée sur `main` (`ca675497`) et
  `dev` (`fbaa506`, PR #103), puis répercutée sur la branche du langage (les 4
  crates `ravum-lang-*` et `ravum-conformance` n'avaient pas le champ).
- **Reste ouvert (outillage, pas décision)** : `THIRD-PARTY-NOTICES` dérivé du
  SBOM ; politique de contributions (DCO/CLA) ; marque « Ravum ».
- **Registre** : [`LICENSE-DECISION.md`](LICENSE-DECISION.md) (`DECIDED`).

## D2 — Ratification des ADR (000…041 + RFC de cette itération)

- **Recommandation** : ne ratifier que les ADR dont les tests de conformance
  prouvent la règle ; garder `PROPOSED` le reste.
- **Preuves** : `docs/adr/` (43 fichiers PROPOSED), `spec/language/` (DRAFT),
  `conformance/` (résultats réels).
- **Sans décision** : toute sémantique reste « candidate », jamais « stable ».

## D3 — Fusion dans `main` et publication de release

- **Recommandation** : aucune fusion `main` ni release stable avant audit
  indépendant et licence tranchée ; livrer une **release candidate interne non
  publiée** liée à un SHA, avec artefacts et checksums.
- **Preuves** : rapports T0-T11, `evidence/`, `docs/reports/LANGUAGE-CORE-STATUS.md`.
- **Sans décision** : `dev` reste la branche d'intégration ; aucun tag stable.

## D4 — Audit externe / revue indépendante de sécurité

- **Recommandation** : mandater un audit humain du borrow checker, du codegen et
  du prélude avant toute promesse de « production-ready ».
- **Pourquoi** : un agent ne peut pas certifier seul l'absence de failles d'un
  compilateur inédit ; les tests ne couvrent qu'un corpus fini.
- **Sans décision** : verdict plafonné à `CORE_BETA` même si tous les gates
  locaux passent.

## D5 — Cibles Tier 1 et qualification Linux

- **Recommandation** : Windows x86-64 = testé localement ; Linux x86-64 =
  **qualifié** par un runner GitHub Actions `ubuntu-latest` sur le SHA de la PR
  (machine indépendante), run
  [42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985) :
  workspace, conformance, smoke, install, Miri, fuzz et WASM verts ; TSan
  `NOT_APPLICABLE` documenté. Le WSL local de la machine de développement est
  hors service (service non réactif, pas d'admin) : il n'est **pas** une preuve.
- **Preuves** : `.github/workflows/linux-qual.yml`,
  [`docs/qualification/LINUX-QUALIFICATION.md`](docs/qualification/LINUX-QUALIFICATION.md),
  artefact `conformance-report-linux`.
- **Limite** : une seule image (`ubuntu-latest`) et un seul run ; pas de
  reproductibilité multi-images ni de qualification ARM/macOS.
- **Sans décision** : annoncer Linux Tier 1 seulement pour l'image testée, pas
  comme support universel.

## D6 — Périmètre des fonctionnalités universelles (T8-T11)

- Trajectoire : Core 1.0 RC limité au sous-ensemble documenté. **FFI minimale**
  (`extern "C"` + `unsafe { }`, signatures primitives) est désormais supportée et
  testée ; hot reload et WASM/WASI sont qualifiés séparément ; threads,
  GPU/mobile, hard-RT restent `UNSUPPORTED`.
- **Sans décision** : pas d'extension de promesse au-delà de ce qui est prouvé
  par la conformance et la CI.

## D7 — Ratification de la frontière `unsafe`/FFI

- **Recommandation** : considérer la FFI Core 1.0 comme **expérimentale** tant que
  (a) l'audit interne du codegen n'est pas revu humainement, (b) aucun audit
  externe n'a couvert la frontière C, (c) la politique de sûreté mémoire des
  bibliothèques C appelées n'est pas écrite. Le profil `pure` la refuse déjà.
- **Preuves** : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.7 bis, tests
  `crates/ravum-lang-sema/tests/ffi.rs`, cas `conformance/cases/run/ffi-abs`,
  `cases/types/ffi-bad-type`, `cases/types/ffi-outside-unsafe`.
- **Sans décision** : FFI utilisable en interne, jamais présentée comme certifiée
  mémoire, jamais dans un profil `pure`/`rt`.

## D8 — Arbitrage des codes de diagnostic émis hors spécification

- **Constat mesuré** (2026-11-09, `docs/qualification/T2-FINDINGS.md`) : la passe
  de traçabilité T2 a produit **268 cas** de conformance dont **30 échouent**
  parce que le compilateur ne fait pas ce que la spécification écrit. Dans
  **13 cas**, le code émis par le compilateur n'apparaît **dans aucun** document
  de `spec/language/` : `E-OWN-MOVE-BORROWED`, `E-OWN-MOVE-INDEXED`,
  `E-OWN-NOT-COPY`, `E-RES-IMPL-FOREIGN`, `E-RES-VARIANT-UNQUALIFIED`,
  `E-TYP-ARRAY-LEN`, `E-UNSUPPORTED-GLOB`, `E-UNSUPPORTED-LET-PATTERN`,
  `E-UNSUPPORTED-OR-PAT`, `E-UNSUPPORTED-REF-PAT`, `E-UNSUPPORTED-TRAIT-IMPL`,
  `E-UNSUPPORTED-TURBOFISH`, `E-UNSUPPORTED-TYPE`.
- **Question posée** : pour chaque condition concernée, quel nom est
  **canonique** ? Deux options, exclusives :
  - **(a) le nom émis gagne** : l'enregistrer dans `diagnostics.md` §3 (le code
    est plus spécifique) et mettre à jour la colonne `Pass / Fail` de
    `features.md` ;
  - **(b) le nom spécifié gagne** : renommer le code dans le compilateur et
    mettre à jour les tests qui l'assertent.
- **Pourquoi ce n'est pas une décision d'agent** : les deux options sont
  défendables, elles touchent une interface publique stable
  (`diagnostics.md` §3 se déclare registre canonique) et changent le contrat
  observable pour tout outil consommateur. Choisir revient à trancher un point
  d'API, pas à corriger un bug.
- **Conséquence d'une non-décision** : le corpus de conformance reste **rouge**
  (30 FAIL), le gate T2 reste `FAIL` et la CI de conformance reste rouge. C'est
  l'état exact, pas une régression : avant cette passe, les divergences existaient
  mais n'étaient pas mesurées.
- **Preuves** : `target/conformance-after-t2.json`,
  `python tools/triage_conformance.py target/conformance-after-t2.json`.

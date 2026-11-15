# Plan « production ready » — analyse du 2026-11-15, décisions et journal

> **Plan d'exécution et journal de reprise, pas un certificat.** Il ne promeut
> aucun gate : le registre canonique reste
> [`CORE-1.0-GATE-MATRIX.md`](CORE-1.0-GATE-MATRIX.md) et les critères R1–R14
> restent ceux de [`PRODUCTION-READINESS.md`](PRODUCTION-READINESS.md).
>
> **Mandat.** Le responsable a demandé le 2026-11-15, en session : « réalise
> toutes les étapes nécessaires pour que le repo soit de qualité production
> ready […] tranche les décisions en visant à ne pas avoir de dette technique,
> documente tout régulièrement ». Les décisions du §2 sont prises **sous cette
> délégation** ; celles du §3 ne sont pas délégables à un agent et restent
> humaines.
>
> **Reprise.** Un agent qui reprend ce chantier lit le §5 (journal) puis le §4
> (étapes) : la première étape non cochée est la suivante.

## 1. Constat mesuré (2026-11-15, `main` = `2f79a63`, `dev` = `581f0f6`)

Chaque ligne cite sa source. `VERIFIED` = commande exécutée ou fichier lu dans
cette session ; `INFERRED` = déduit, non exécuté.

| # | Constat | Gravité | Preuve |
|---|---|---|---|
| B1 | **`main` et `dev` sont rouges** sur trois workflows (Linux, macOS, Windows evidence) depuis la fusion de la vague 3 | **P0** | `VERIFIED` — `gh run list --branch main` : runs `43669217867`, `43669218045`, `43669218085` en échec, même étape « conformance » |
| B2 | Cause unique : **divergence `check → build`** sur `run/power-array-move-write-region-nested` (`check` exit 0, `rustc` `E0508`) ; `build_verified: false`, 487 cas / 484 PASS / 1 FAIL / 2 SKIP sous Linux | **P0** | `VERIFIED` — journal du run `43669217867` |
| B3 | Mécanisme : dans `MovePlanner` (`emit.rs`), l'ordre temporel repose sur `top` (indice de l'instruction de **premier niveau**) et sur l'ordre de l'instruction englobante. Dans un bloc imbriqué, toutes les instructions partagent le même `top`, et un usage entier situé dans la partie droite de `a = …` est classé **après** l'écriture : le hoist est bloqué, rien n'est abaissé | P0 | `INFERRED` de la lecture de `emit.rs:4178-4201`, `4376-4396`, `4594-4628`, `4924-5016` ; à confirmer par test |
| B4 | **Aucun `cargo clippy` en CI** ; ni `cargo audit`, ni `cargo-deny`, ni Dependabot, ni `CODEOWNERS`, ni `CHANGELOG.md` | P1 | `VERIFIED` — `grep clippy .github/workflows` (un seul résultat : installation du composant), fichiers absents |
| B5 | **Fichiers hors budget de taille** : `emit.rs` 8 377 lignes, `typeck.rs` 6 208, `parser.rs` 5 417, `loader.rs` 4 156, `own.rs` 3 757, `resolve.rs` 2 980 (seuil de refactor obligatoire du responsable : 1 500) | P1 | `VERIFIED` — `wc -l` |
| B6 | Couverture du code produit **90,37 %** (1 305 lignes) pour un objectif S5 de 100 % ; 14 exemptions proposées non tranchées | P1 | rapport vague 3 §0.3 (non re-mesuré ici) |
| B7 | Test de puissance T2 : 9 formes sur 13, 4 `NOT_RUN` ; campagne nominale 3 × 2 000 `NOT_RUN` | P1 | rapport vague 3 §0.1 |
| B8 | `OwnFacts` étape C : 2 familles migrées sur 7 ; le backend re-dérive donc encore des décisions d'ownership (double source de vérité, origine de B2/B3) | P1 | rapport vague 3 §0.4 |
| B9 | Mutation `emit.rs` : campagne complète interrompue, aucun score publié | P2 | rapport vague 3 §0.5 |
| B10 | Documents « courants » périmés (`STATUS.md` : 471 cas ; matrice §2 : 463/471 cas ; `START_HERE.md` : `AGENTS.md` encore « PENDING ») ; la matrice mêle état courant et 900 lignes d'archives | P2 | `VERIFIED` — lecture des trois fichiers |
| B11 | Aucune release, aucun tag, aucun artefact distribuable, aucune signature ; `rust-version = "1.74"` déclaré alors que seul 1.95.0 est testé | P1 | `VERIFIED` — `Cargo.toml`, matrice T11 |
| B12 | 97 issues ouvertes, dont la totalité des issues M0–M4 de la fondation, sans tri depuis l'intégration | P2 | `VERIFIED` — `gh issue list` |
| B13 | Sorties de mutation (`mutants*.out*`) à la racine du poste | P3 | `VERIFIED` — non suivies par git (`git ls-files` vide) : bruit local, pas une dette du dépôt |
| B14 | Audit humain indépendant absent (T4, T12, R4, R13) | **bloquant de label** | matrice §2 |

## 2. Décisions tranchées sous délégation (2026-11-15)

| ID | Sujet | Décision | Motif |
|---|---|---|---|
| **P-1** | Cas rouge `power-array-move-write-region-nested` | **Garder le cas dans le gate et corriger le compilateur** ; aucune mise hors gate, aucun `expected-fail` | Un gate rouge toléré n'est plus un gate ; OWN-5.3/5.4 (MUST) autorisent le programme, donc le refuser au `check` exigerait d'affaiblir la spec |
| **P-2** | Ordre des travaux | **Rien d'autre n'est fusionné tant que `dev` n'est pas vert** (stop-the-line) | SLA du responsable : défaut de build = immédiat |
| **P-3** | Lints | **`cargo clippy --workspace --all-targets -- -D warnings` devient un job bloquant** ; les avertissements existants sont corrigés, pas `allow`-és en bloc | Pré-commit Rust du responsable ; aucun `#![allow]` de confort |
| **P-4** | Chaîne d'approvisionnement | **`cargo-deny`** (advisories RustSec, licences compatibles `MIT OR Apache-2.0`, doublons, sources) en CI bloquante + **Dependabot** hebdomadaire (cargo, actions) | Un seul outil couvre audit + licences ; Dependabot évite la dérive silencieuse |
| **P-5** | Gouvernance | **`CODEOWNERS`** (responsable sur `spec/`, `conformance/`, `.github/`, `crates/ravum-lang-*`) et **`CHANGELOG.md`** (Keep a Changelog, dérivé des Conventional Commits) | Exigences « senior reflexes » ; R10 demande un canal d'avertissement de dépréciation |
| **P-6** | Taille des fichiers | **Découper les six fichiers > 1 500 lignes** en sous-modules à responsabilité unique, **sans changement de comportement**, une PR par fichier ; filet : suite complète + comparaison S7 (Rust généré identique octet pour octet) | Refactor obligatoire ; le découpage précède la couverture à 100 % pour ne pas écrire des tests contre une structure condamnée |
| **P-7** | `OwnFacts` étape C | **Terminer la migration** (extension de schéma par RFC `ACCEPTED` sous cette délégation : une décision par nœud de chaîne de place, propagation de « consommé » à travers `match`/`if`/bloc, identité de liaison unique par paramètre), puis **supprimer la re-dérivation du backend** | Deux implémentations de la même connaissance = la cause de toutes les divergences `check → build` trouvées depuis la passe 6 ; les garder serait la dette principale du projet |
| **P-8** | Exemptions de couverture | **Aucune exemption de confort.** Pour chacune des 14 : (a) si le chemin est réellement inatteignable, **supprimer le code** ou le remplacer par un invariant vérifié (`debug_assert!`/erreur interne testée) ; (b) sinon écrire le test. Une exemption ne subsiste que si la suppression est impossible, avec preuve, dans `tools/coverage_exemptions.json` | S5 ; « code mort » et « exemption » sont deux façons de garder une dette |
| **P-9** | Test de puissance T2 | **Exécuter les 4 inverses `NOT_RUN` et la campagne nominale 3 × 2 000 en CI** (`differential-oracle.yml`, déclenchement manuel + hebdomadaire) | Budget local insuffisant constaté en vague 3 ; la CI rend la preuve rejouable |
| **P-10** | Signature (D9) | **Attestations sans clé** : `actions/attest-build-provenance` (Sigstore, identité OIDC du workflow du dépôt public) sur les artefacts de release, vérification par `gh attestation verify`. Le mécanisme OpenSSH qualifié reste documenté comme alternative hors ligne | Supprime la question de garde de clé : l'ancre de confiance est l'identité du workflow, pas un secret détenu par quelqu'un. **Reste à confirmer par le responsable avant la première release** (§3) |
| **P-11** | Version minimale de Rust | **`rust-version` aligné sur la toolchain réellement testée** (1.95.0), tant qu'aucun job MSRV n'existe | Une MSRV déclarée et jamais testée est une fausse affirmation (R14) |
| **P-12** | Documents d'état | **Une seule source par fait** : la matrice ne garde que l'état courant (§2–§5) ; les sections datées `§0 bis…decies` et `§1.x` partent dans `docs/roadmap/archive/` ; `STATUS.md`, `START_HERE.md`, `LANGUAGE-CORE-STATUS.md` renvoient aux chiffres au lieu de les recopier ; `report_consistency.py` vérifie les chiffres restants | Deux relecteurs externes ont déjà pris des archives pour l'état courant |
| **P-13** | Périmètre de la 1.0 | **Inchangé** (D6, A2, A3) : langage Core 1.0 = `ravum-lang-*` ; les crates plateforme `ravum-*` restent **expérimentales**, hors promesse 1.0, et le disent dans leur `README`/`Cargo.toml` | Pas d'extension de promesse sans preuve |
| **P-14** | Issues | **Tri par preuve** : une issue de fondation n'est fermée « completed » que si un rapport mesuré la couvre, « not planned » si le périmètre gelé l'exclut ; sinon elle reste ouverte et reçoit un jalon | Les issues sont le backlog canonique ; 97 issues non triées ne le sont plus |
| **P-15** | Fusions | PR **vers `dev`**, une par étape, ≤ 400 lignes modifiées quand c'est possible, commits Conventional ; **aucune fusion vers `main`, aucun tag, aucune release sans accord explicite** | `AGENTS.md` du dépôt |

## 3. Ce qui reste humain (non délégable)

1. **Audit indépendant (D4 → T4, T12, R4, R13).** Un agent ne peut pas être
   l'auditeur indépendant de son propre travail. Tant qu'il manque, le dépôt
   peut être **techniquement prêt pour l'audit**, pas « production ready » au
   sens de `PRODUCTION-READINESS.md` §1. Le dossier (`docs/audit/AUDIT-BRIEF.md`)
   sera tenu à jour à chaque étape.
2. **Ratification des ADR (D2).** `INV-13` et `AGENTS.md` interdisent
   l'auto-acceptation. L'étape E8 produit une **liste de ratification
   proposée** (ADR dont la règle est prouvée par la conformance, lus en texte
   intégral) ; la signature reste au responsable.
3. **Go/No-Go (T12)**, **première release et tag**, **confirmation de P-10**.

## 4. Étapes (ordre de dépendance)

Chaque étape = une issue GitHub + une PR vers `dev`. « Fait » exige : commande,
sortie, SHA, CI verte.

- [ ] **E1 — Remettre `dev` au vert (P-1, P-2).** Corriger la planification des
  moves d'élément après une écriture entière imbriquée ; tests négatifs et
  positifs `rustc`-vérifiés ; sondage des variantes voisines (écriture
  conditionnelle, accès hors du bloc de l'écriture, boucle) ; conformance
  `--verify-build` 0 FAIL sur trois OS.
- [ ] **E2 — Garde-fous manquants (P-3, P-4, P-5, P-11).** Clippy `-D warnings`,
  `cargo-deny`, Dependabot, `CODEOWNERS`, `CHANGELOG.md`, `rust-version`.
- [ ] **E3 — Véracité documentaire (P-12).** Archivage des sections datées,
  chiffres courants re-mesurés, `START_HERE.md`/`STATUS.md` corrigés.
- [ ] **E4 — Découpage des six fichiers (P-6).** Ordre : `emit.rs`, `typeck.rs`,
  `parser.rs`, `loader.rs`, `own.rs`, `resolve.rs`.
- [ ] **E5 — `OwnFacts` étape C complète (P-7).** RFC d'extension de schéma,
  migration des cinq familles, suppression de la re-dérivation.
- [ ] **E6 — Couverture produit 100 % et mutation (P-8).** Y compris campagne
  `emit.rs` complète en CI.
- [ ] **E7 — Test de puissance T2 complet (P-9).**
- [ ] **E8 — Interfaces et distribution (P-10, R10, R11).** Workflow de release
  (artefacts Windows/Linux, SBOM, provenance attestée), canal de dépréciation,
  liste de ratification D2 proposée, contrat d'installation.
- [ ] **E9 — Tri des issues (P-14) et dossier d'audit à jour.**
- [ ] **E10 — Demande de décision finale** : audit, ratification, Go/No-Go.

## 5. Journal

| Date | Étape | Fait | Preuve | Suite |
|---|---|---|---|---|
| 2026-11-15 | Analyse | Lecture du mandat, de la matrice, de `PRODUCTION-READINESS.md`, du rapport vague 3 ; état CI et issues relevés ; mécanisme B3 localisé | §1 | Rédaction de ce plan, puis E1 |
| 2026-11-15 | E1 (en cours, **non terminé**) | Sondages : 17 programmes défaillants sur 30 acceptés, dont **6 sorties fausses silencieuses** ; `MovePlanner` remplacé par un abaissement par accès ; règles `OWN-5.11`/`OWN-5.12` ; 16 cas de conformance ; RFC et `T2-FINDINGS` §29 | 32 programmes de sondage rejoués : 28 sorties attendues, 4 refus au `check` ; oracle rapide graines 1 et 2 : 0 divergence | Fin de l'oracle, tests, conformance, 3e sondage, chiffres, issue + PR |
| 2026-11-15 | E1 (vérification locale terminée) | Oracle rapide **3 graines × 200, 0 divergence** ; `cargo fmt` ; **7 tests unitaires** `own.rs` et **1 test backend** pour champs/tuples/imbriqués/`self`/`match` ; **503 cas : 502 PASS / 0 FAIL / 1 SKIP**, `build_verified: true` (périmètre 222/222, 124 vérifiés, 98 implicites) ; workspace **1 740 / 0 / 8** ; MUST **811 / 467 / 344** ; docs courantes réalignées | Commandes et sorties : `T2-FINDINGS.md` §29.5 ; `target/conformance-e1.json` ; `target/e1-cargo-test-final.log` | **Reste : commit, issue GitHub, PR vers `dev`, CI trois OS.** Aucun gate promu ; la campagne nominale T2 reste E7 |

# Prompt d'audit externe — Ravum Core 1.0 (T4 / T10 / T12)

> **Usage** : copier le bloc « PROMPT » ci-dessous dans un agent externe disposant
> d'un accès **fichiers + shell** au dépôt (Claude Code, Codex CLI, agent avec
> outils). Un agent sans accès au dépôt ne peut pas produire T4 : le prompt le lui
> interdit explicitement.
>
> Ce fichier est un **outil de commande d'audit**. Il ne contient aucun résultat
> d'audit et ne remplace ni `AUDIT-BRIEF.md` ni la matrice de gates.

## Contexte pour le mainteneur

- Rôle visé : auditeur **indépendant** qui vérifie, rejoue et casse ; il ne
  répare pas, ne ratifie pas, ne fusionne pas.
- Ce que l'audit peut produire : des **constats** (findings) reproductibles et
  un avis par gate (`PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`).
- Ce qu'il ne peut pas produire : une décision Go/No-Go finale (c'est D3/D4,
  humaine), une acceptation d'ADR, une signature.
- Prévoir un **SHA figé** (le prompt demande de remplacer `<SHA_AUDIT>`), et un
  clone dédié : l'auditeur travaille sur une copie, jamais sur l'arbre de
  développement.

---

## PROMPT

Tu es **auditeur externe indépendant** du projet Ravum (langage de programmation
`Ravum Core 1.0`, implémentation Rust). Ta mission couvre trois gates :
**T4** (audit externe), **T10** (suffisance des preuves) et **T12** (avis de
Go/No-Go technique). Tu n'es pas développeur du projet : tu ne modifies pas
l'arbre audité, tu ne « corriges » rien, tu ne ratifies aucun ADR.

### 0. Règles absolues

1. **Aucun résultat inventé.** Chaque constat cite une commande, sa sortie
   pertinente, l'OS, la toolchain et le SHA. Si tu n'as pas exécuté, écris
   `NOT_RUN`.
2. **Aucune confiance dans les rapports.** Les documents du dépôt sont des
   *affirmations à vérifier*, pas des preuves. Un `PASS` écrit dans un fichier
   ne vaut rien sans rejeu.
3. **Aucun `PASS` par charité.** Une règle partiellement testée est `PARTIAL`.
   Un test ignoré, `SKIP`, `NOT_RUN`, `TIMEOUT` n'est jamais un succès.
4. **Aucune modification du dépôt audité.** Travaille dans un clone jetable.
   Si tu proposes un correctif, mets-le dans un patch séparé, non appliqué.
5. **Pas d'acceptation d'ADR, pas de fusion, pas de publication.**
6. **Langue du rapport : français ou anglais**, mais les codes de diagnostic,
   chemins et commandes sont cités **littéralement**.
7. Si tu n'as pas d'accès fichiers + shell : **arrête-toi** et déclare que T4 ne
   peut pas être instruit (une revue documentaire seule ne vaut pas T4).

### 1. Cible

- Dépôt : `D:\App\Ravum` (ou l'URL de ton clone). Branche à auditer :
  `feat/lang-core-1.0-rc`. **SHA figé : `<SHA_AUDIT>`** — vérifie-le avec
  `git rev-parse HEAD` et note tout écart.
- `main` n'est pas la cible et n'est pas une preuve de maturité.
- Toolchain de référence : `rustc 1.95.0` (voir `rust-toolchain.toml`),
  `cargo-llvm-cov 0.9.1`, `cargo-mutants 27.1.0`.
- Environnement de référence des mesures existantes : Windows 11 x86-64. La CI
  fournit Linux (`ubuntu-latest`). Aucune mesure macOS/ARM n'existe.

### 2. À lire d'abord (dans cet ordre)

1. `AGENTS.md` — mandat, interdits, définition de preuve.
2. `docs/architecture/PROJECT_INVARIANTS.md` — invariants P0-A…P0-I.
3. `spec/language/` — **la spécification est la seule source de vérité** :
   `grammar.md`, `types.md`, `expressions.md`, `ownership.md`, `effects.md`,
   `modules.md`, `stdlib.md`, `diagnostics.md`, `features.md`, `unsupported.md`.
4. `docs/roadmap/CORE-1.0-GATE-MATRIX.md` — matrice T0…T12 (statuts autorisés :
   `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN` uniquement).
5. `docs/qualification/` — `T2-FINDINGS.md`, `MUTATION-EQUIVALENCE.md`,
   `FUZZ-*`, `FUZZ-MIRI-SEMA.md`, `REPRODUCIBILITY.md`, `SIGNATURES.md`,
   `LINUX-QUALIFICATION.md`, `PERFORMANCE.md`.
6. `docs/reports/CORE-1.0-FINAL-REPORT.md` et `docs/reports/LANGUAGE-CORE-STATUS.md`.
7. `docs/audit/AUDIT-BRIEF.md` — cadre de mission déjà publié.
8. `docs/rfc/` — propositions non ratifiées (D2, D6) : **ne les traite pas comme
   des décisions**.

### 3. État revendiqué (à re-vérifier, pas à croire)

Les chiffres ci-dessous sont ceux que le projet revendique au SHA audité. Ton
travail est de les **reproduire ou de les contredire**.

| Élément | Revendication | Où le vérifier |
|---|---|---|
| Tests workspace | 89 suites, 1405 passés, 0 échec, 2 ignorés | `cargo test --workspace --all-targets` |
| Conformance | 386 cas, 385 `PASS`, 0 `FAIL`, 1 `SKIP` | `cargo run -p ravum-conformance -- run` |
| Registre des codes | 108 codes, 0 hors registre, 0 jamais émis | `python tools/registry_audit.py` |
| Règles tracées | 188/188 (1 exemption machine-lisible) | `python tools/features_coverage.py --strict` |
| Couverture `typeck.rs` | ≥ 90 % lignes / ≥ 85 % régions | `cargo llvm-cov -p ravum-lang-sema`, `python tools/coverage_report.py --check` |
| Couverture `resolve.rs` | ≥ 90 % / ≥ 85 % | idem |
| Mutation | campagnes ciblées, survivants expliqués | `mutants-*.out/outcomes.json`, `docs/qualification/MUTATION-EQUIVALENCE.md` |
| Reproductibilité | binaire bit-à-bit identique entre deux racines | `pwsh -File scripts/repro-check.ps1` |
| SBOM | SPDX 2.3 déterministe | `pwsh -File scripts/sbom.ps1`, `SOURCE_DATE_EPOCH=0` |
| Cohérence des rapports | chiffres identiques entre matrice et rapport | `python tools/report_consistency.py` |
| Gates | `5 PASS / 6 PARTIAL / 2 BLOCKED` | matrice §2 |

### 4. Ce que T4 doit réellement chercher

Ne te limite pas à rejouer les tests : ils sont écrits par le projet. Cherche
les **écarts entre la spécification et le comportement**, et les **programmes
acceptés qui ne devraient pas l'être**.

Priorités (du plus grave au moins grave) :

1. **Soundness du vérificateur** : trouver un programme que `ravum-lang check`
   accepte et dont le Rust généré ne compile pas, ou qui viole une règle MUST de
   `types.md`/`ownership.md`/`effects.md`. Méthode : échantillonner les règles
   MUST une par une, écrire le programme minimal qui les viole, comparer le code
   attendu (`diagnostics.md`) au code observé.
2. **Motifs et exhaustivité** : `match` sur entiers, flottants (G-6.36),
   négatifs (G-6.37), variantes, tuples, `Option`/`Result`, `None` nu,
   `None(payload)`, `!`. Cherche un `match` non exhaustif accepté ou un bras
   inatteignable qui masque un trou.
3. **Visibilité et modules** : `use` de modules privés, segments intermédiaires
   non `pub`, `pub(crate)`, cycles, `mod` sans fichier, chemins d'échappement.
   Le projet connaît **deux trous** dans ce domaine (voir §6) : cherche s'il en
   existe d'autres, notamment par les chemins de type et de valeur.
4. **Bornes et robustesse** : entrées hostiles (imbrication profonde, fichiers
   énormes, nombre de fichiers, tokens), budget de diagnostics (DIAG-1.7/4.3 :
   100 puis un unique `E-PARSE-TOO-LARGE`), absence de panique ou de
   débordement de pile. Un plantage est un défaut, pas un `SKIP`.
5. **Determinisme** : deux exécutions identiques produisent-elles la même sortie
   octet pour octet (DIAG-4.4) ? Ordre des diagnostics, JSON, formateur.
6. **Chaîne d'émission** : le code Rust généré (`.ravum/gen/main.rs`) est-il
   auditable, déterministe, sans `unsafe` non déclaré ? Les effets déclarés
   correspondent-ils aux opérations réellement émises ?
7. **FFI / `unsafe`** : surface `extern "C"` + `unsafe { }` minimale ; vérifier
   les refus et l'absence de contournement (D7 est ouverte : tu constates, tu ne
   ratifies pas).
8. **Preuves eux-mêmes** (T10) : la couverture est-elle *mesurée* et *bloquante
   en CI* ? Les survivants de mutation sont-ils réellement équivalents (lis le
   raisonnement, attaque-le) ? Les exemptions sont-elles justifiées et
   révocables ? Le fuzz et Miri couvrent-ils les mêmes chemins que les tests
   natifs ? Les benchmarks publiés sont-ils reproductibles sur ta machine, et
   sinon le disent-ils ?
9. **Chaîne de distribution** (T11/T12) : installateur, SBOM, checksums,
   provenance, signature (mécanisme vs clé absente — D9). Ne confonds pas
   « mécanisme qualifié » et « artefact signé ».

### 5. Commandes de rejeu minimales

Adapte à ton OS (les scripts `pwsh` sont Windows ; la CI Linux utilise `bash`) :

```
cargo test --workspace --all-targets
cargo build -p ravum-lang-cli --bin ravum-lang
cargo run -p ravum-conformance -- run
python tools/registry_audit.py
python tools/features_coverage.py --strict
python tools/report_consistency.py
python tools/json_contract.py
cargo llvm-cov -p ravum-lang-sema --json --output-path target/cov-audit.json
python tools/coverage_report.py --input target/cov-audit.json --check
pwsh -File scripts/lang-smoke.ps1
pwsh -File scripts/repro-check.ps1
pwsh -File scripts/sbom.ps1
pwsh -File scripts/install.ps1 --prefix <répertoire-jetable>
cargo mutants --file crates/ravum-lang-sema/src/types.rs
```

Un test qui échoue chez toi et pas dans le rapport est un **constat**, pas une
erreur de ta part : documente OS/toolchain et joins la sortie.

### 6. Ce que le projet sait déjà défectueux (à confirmer, pas à redécouvrir)

Liste issue de `docs/qualification/T2-FINDINGS.md`. Les points **corrigés en
passe 5** sont désormais des **revendications à attaquer** (le correctif est-il
complet, ou n'a-t-il fermé qu'un cas ?) ; les points **ouverts** sont à confirmer.
Puis **cherche ce qui n'est pas dans cette liste** :

**Corrigés en passe 5 — attaque le correctif, pas la description d'origine :**
- `use crate::geo;` sur un module privé : censé être accepté depuis le module
  déclarant et ses descendants (MOD-4.3), avec `E-RES-DUPLICATE` dans le module
  déclarant (MOD-3.7).
- Segments non finaux d'un `use` : `use crate::a::secret::X;` doit produire
  `E-RES-PRIVATE` (trou de confidentialité fermé) — cherche un chemin qui
  contourne encore la vérification (ré-export interdit, `super`, import via un
  alias, profondeur > 2).
- Famille complète des affectations composées : `+= -= *= /= %= &= |= ^= <<= >>=`
  censées être acceptées (G-6.8/EXPR-9.4) — attaque la sémantique `place op= e`
  (évaluation unique de la place, types mélangés, `bool`, décalages).
- `test fn` invalide → `E-TYP-MAIN` ; comparaisons entre types numériques
  différents → `E-TYP-MISMATCH` ; `Vec::new()` + `push` infère l'élément.
- MOD-3.2 : `use` relatif nu résolu du module courant vers la racine, double
  résolution → `E-RES-CONFLICT` — cherche de fausses acceptations ou de faux
  conflits.
- Codes inatteignables supprimés (`E-PARSE-BAD-SUFFIX`, `E-UNSUPPORTED-UFCS`,
  `E-UNSUPPORTED-COMPOUND-ASSIGN`) ; `E-OWN-LIMIT` rendu atteignable (borne par
  fonction 50 < budget fichier 100).
- Harnais : `W-*` acceptés (DIAG-1.6) ; phases JSON alignées sur le registre.

**Encore ouverts / suspects :**
- Coercition implicite `String → &str` : `TYP-13.1` ne l'admet pas, mais
  `line.trim()` rend un `String` (`STD-9.1`) et les exemples en dépendent
  (décision ouverte `D-COERCE-1`) — c'est une divergence assumée, à qualifier.
- `E-LIMIT-FILES` : aucun test dédié (4097 fichiers ou 200 modules) ; la limite
  n'a pas bougé mais la preuve manque.
- Deux sites de résolution restent inatteignables par construction
  (`E-RES-SELF` L559, `E-RES-SELF-OUTSIDE-IMPL` L2281) ; des tests « parse-only »
  documentent l'inatteignabilité.
- Aucun backend natif (voie C) : `T1` reste `PARTIAL` (décision `D2`).
- Réseau, async, threads, hot-state : absents et refusés par le langage
  (décision `D6`).

### 7. Livrable attendu (format imposé)

1. **En-tête** : SHA vérifié, OS, `rustc -vV`, versions `cargo-llvm-cov` /
   `cargo-mutants`, durée d'audit, clone utilisé.
2. **Tableau des gates** : une ligne par gate T0…T12 avec statut ∈ {`PASS`,
   `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`}, la preuve rejouée, et le désaccord
   éventuel avec la matrice du dépôt.
3. **Constats**, numérotés, chacun avec : sévérité (`critical` / `major` /
   `minor` / `info`), règle ou invariant concerné, programme minimal, commande,
   sortie observée, comportement attendu, et si possible un test qui échoue.
   Classes attendues : *accepté à tort*, *rejeté à tort*, *code/phase erroné*,
   *crash*, *non-déterminisme*, *preuve insuffisante*, *documentation fausse*.
4. **T10 — suffisance des preuves** : pour chaque type de preuve (tests,
   conformance, couverture, mutation, fuzz, Miri, repro, SBOM, CI), réponds
   « suffisant pour la revendication », « insuffisant » ou « absent », avec la
   raison. Distingue explicitement *mesuré* / *revendiqué* / *non vérifié*.
5. **T12 — avis Go/No-Go** : `GO`, `GO sous conditions` ou `NO-GO`, avec les
   conditions numérotées et le niveau de confiance. Rappelle que la décision
   finale est humaine (D3/D4) et que ton avis ne fusionne ni ne publie rien.
6. **Liste « non vérifié »** : tout ce que tu n'as pas pu exécuter, avec la
   raison (OS, réseau, temps, droits, absence de clé).
7. **Ce que tu n'as pas trouvé** : les zones où tu as cherché sans succès. Une
   absence de constat est une information, pas un blanc.

### 8. Barème de sévérité (à utiliser tel quel)

- **critical** : programme valide rejeté, programme invalide accepté qui produit
  du code Rust invalide ou viole une règle MUST de soundness ; crash du
  compilateur ; preuve annoncée `PASS` qui n'existe pas.
- **major** : code ou phase de diagnostic erroné ; règle MUST non appliquée sans
  conséquence de soundness ; preuve non reproductible ; CI verte sans le gate.
- **minor** : message trompeur, documentation interne incohérente, friction.
- **info** : observation utile sans défaut.

Ne gonfle pas la sévérité pour être remarqué, ni ne la réduis pour être agréable.

---

## Notes pour le mainteneur (hors prompt)

- Après l'audit, la matrice ne passe à `PASS` pour T4/T12 que si l'auditeur a
  **rejoué** des commandes sur le SHA figé et produit des constats ; un avis
  favorable sans rejeu n'est pas un audit.
- Les decisions D2/D6/D-PAT-1(=résolue)/D-CMP-1/D-COERCE-1/D9 restent humaines.
  Un audit peut les éclairer, jamais les clore.
- Le prompt demande une copie jetable : cela protège l'arbre de développement et
  garantit que l'auditeur ne « répare » pas ce qu'il devrait signaler.

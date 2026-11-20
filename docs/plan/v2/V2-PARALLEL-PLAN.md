# Plan V2 — exécution en sessions parallèles

| Champ | Valeur |
|---|---|
| Statut | **Validé par le responsable le 2026-11-20** (V2-P1 ordre, V2-D1 RFC ABI DSP 2, V2-E1 E2E navigateur dans la V2) |
| Base | branche `claude/trusting-cerf-4hhv5a`, commit de départ indiqué dans chaque rapport de chantier |
| Registre | `docs/roadmap/CORE-1.0-GATE-MATRIX.md` (fait foi ; **modifié seulement par l'intégrateur**) |
| Proposition source | `docs/roadmap/V2-PLAN-PROPOSAL.md` |

Ce plan découpe la V2 en **chantiers indépendants** qu'on peut confier à des
sessions d'agents différentes (Claude Code, Codex, DeepSeek, …) en même temps.
Chaque chantier a un **périmètre de fichiers propre** pour éviter les
conflits, une **branche propre**, un **rapport propre**, et des critères de
fin vérifiables. Le prompt de lancement est en §6.

## 1. Règles communes (toutes sessions, quel que soit l'outil)

1. Lire `AGENTS.md` en entier avant toute modification ; il prime sur ce plan.
2. Une session = **un** chantier. Branche : `v2/<id>` (ex. `v2/L2-ir-corpus`),
   créée depuis `origin/claude/trusting-cerf-4hhv5a`. Pas de force push, pas
   de merge, pas de PR sans demande du responsable.
3. Ne modifier **que** les fichiers du périmètre du chantier (§3). Besoin d'un
   fichier hors périmètre : l'écrire dans le rapport (« demande hors
   périmètre »), ne pas le modifier.
4. Fichiers **réservés à l'intégrateur** (jamais modifiés par un chantier) :
   `docs/roadmap/CORE-1.0-GATE-MATRIX.md`, `docs/decision/*`,
   `DECISIONS_REQUIRED.md`, `deny.toml`, `rust-toolchain.toml`, `Cargo.toml`
   racine, `.github/workflows/*` (sauf chantier L6, voir §3).
5. Nouvelle dépendance : interdite sans décision du responsable (licence,
   `deny.toml`). La proposer dans le rapport.
6. Un RFC/ADR est toujours écrit `PROPOSED` ; **jamais** d'auto-acceptation.
   Un chantier dont l'implémentation dépend d'un RFC s'arrête après le RFC.
7. Ne jamais supprimer, désactiver, marquer `#[ignore]` ni affaiblir un test
   ou un seuil. `SKIP`/`NOT_RUN` ≠ `PASS`. Aucun `PROVED`, aucun `PASS` de gate
   écrit par un chantier : seul l'intégrateur propose un changement d'état.
8. Thread audio (INV-05) : aucune compilation, allocation, blocage,
   migration ni destruction coûteuse dans le rappel.
9. Avant chaque commit : `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, les tests des
   crates touchés ; avant la fin : `cargo test --workspace` et la
   conformance (`cargo run -p ravum-conformance --release`) si le langage est
   touché. Rapporter les échecs tels quels.
10. Commits sans identifiant de modèle. Français pour les documents.
11. Fin de chantier : rapport `docs/reports/V2-<id>.md` (gabarit §5), commit,
    push sur `v2/<id>`, puis donner au responsable : branche, SHA, résumé,
    état des preuves, demandes hors périmètre.

## 2. Vue d'ensemble

> **2026-11-20** : L1, L5 et L7 sont **rédigées** (statut `PROPOSED`) sur la
> branche d'intégration `claude/trusting-cerf-4hhv5a` :
> `docs/rfc/RFC-DETERMINISTIC-PROFILE-1.md`, `docs/rfc/RFC-DSP-ABI-2.md`,
> `docs/rfc/RFC-ASYNC-NET-1.md`. Ne pas les relancer ; leurs implémentations
> (L1b, L5b, L7b) attendent la ratification du responsable.

| Id | Gate | Chantier | Dépend de | Parallélisable avec | Taille |
|---|---|---|---|---|---|
| L1 | T17 | RFC du profil déterministe (horloge, aléa, ordonnancement virtualisés, replay par seed) | — | tous | S (doc) |
| L2 | T14 | IR/JIT : 6 programmes restants du corpus (55/61) + structs non `Copy` au JIT | — | tous sauf L4 sur `ravum-lang-jit` (voir §4) | L |
| L3 | T2 | Rattacher les 95 MUST sans FeatureId | — | tous | M |
| L4 | T8/T9 | Brancher `examples/dsp-gain-iir` sur l'hôte audio, mesurer le fondu | — | tous | M |
| L5 | T8 | RFC ABI DSP 2 (état DSP au-delà de `{ z1, z2 }`) | — | tous | S (doc) |
| L6 | T9 | E2E navigateur : `packages/http-notes` piloté par Chromium réel | — | tous | M |
| L7 | T6 | RFC `connect` TCP asynchrone + HTTP asynchrone | — | tous | S (doc) |
| L1b, L5b, L7b | T17/T8/T6 | Implémentations des RFC | ratification du responsable | — | L |
| INT | — | Intégration : merge des branches, matrice, décisions | chantiers terminés | — | — |

Ordre validé (V2-P1) en cas de capacité limitée : L1 → L2 → L3 → L4 → L7,
avec L5 et L6 ajoutés (V2-D1, V2-E1). Les trois RFC (L1, L5, L7) sont courtes
et débloquent la suite : les lancer en premier sur des abonnements « légers ».

## 3. Chantiers

### L1 — T17 : RFC du profil déterministe

- **Contexte** : `docs/reports/T17-STEP1-HOT-SESSION-REPLAY.md` (§Limites :
  « le profil déterministe du langage (horloge, aléa, ordonnancement
  virtualisés, replay par seed) n'est pas fait ; il touche la stdlib et le
  checker »).
- **Périmètre** : `docs/rfc/RFC-DETERMINISTIC-PROFILE-1.md` (nouveau) uniquement.
- **Travail** : RFC `PROPOSED` : syntaxe/activation du profil (cohérente avec
  les profils d'effets existants, `spec/language/effects.md`), API stdlib
  virtualisées, règles MUST numérotées, diagnostics proposés (codes à
  vérifier libres dans `tools/diagnostic_codes.snapshot`), stratégie de
  replay par seed, tests positifs/négatifs prévus, alternatives écartées,
  questions au responsable.
- **Fini quand** : RFC écrite, chaque règle a un test prévu, aucune ligne de
  code modifiée.

### L2 — T14 : IR/JIT, corpus et structs non `Copy`

- **Contexte** : `docs/reports/T14-T15-IR-AND-JIT.md` (corpus machine 55/61 ;
  JIT limité aux structs `Copy`), `crates/ravum-lang-ir`, `crates/ravum-lang-jit`.
- **Périmètre** : `crates/ravum-lang-ir/**`, `crates/ravum-lang-jit/**` (sauf
  `src/hot.rs` et `tests/rt_budget.rs`, `tests/hot_swap.rs` : partagés avec le
  temps réel, lecture seule), rapport.
- **Travail** : (1) lister les 6 programmes du corpus non couverts et la
  construction manquante de chacun ; (2) couvrir ceux qui ne sont pas un
  « refus voulu » documenté (`sin`, `u128` : décider dans le rapport, ne pas
  forcer) ; (3) structs non `Copy` (champs `String`/`Vec`) dans l'IR et le JIT,
  sortie identique à la voie `rustc` et à l'interpréteur ; (4) oracle
  différentiel étendu (programmes générés), 0 divergence.
- **Négatifs** : programme refusé → refus fail-closed explicite, pas de
  repli silencieux.
- **Fini quand** : corpus N/61 mesuré et rapporté, structs non `Copy`
  couvertes par tests, différentiel 0 divergence, workspace vert.

### L3 — T2 : MUST sans FeatureId

- **Contexte** : `python3 tools/must_audit.py` (« unmapped MUST statements
  (95) »), `tools/features_coverage.py --strict`, `tools/feature_ids.snapshot`.
- **Périmètre** : tables de règles de `spec/language/*.md` (colonnes de
  rattachement seulement, **jamais** le texte normatif), `conformance/cases/**`
  (ajouts), `tools/feature_ids.snapshot`, rapport.
- **Travail** : pour chaque MUST non rattaché : FeatureId existant, ou
  nouveau FeatureId + cas de conformance positif et négatif. Aucune
  exemption nouvelle. Si un MUST est faux ou non testable : le lister dans
  le rapport pour décision, ne pas le modifier.
- **Fini quand** : `must_audit.py` → 0 non rattaché (ou liste résiduelle
  justifiée), `features_coverage.py --strict` vert, conformance 0 FAIL.

### L4 — T8/T9 : DSP `dsp-gain-iir` sur l'hôte audio

- **Contexte** : `examples/dsp-gain-iir/src/main.rv`, `crates/ravum-lang-audio`
  (trait `Processor`, `WavHost`), `docs/reports/V1-AUDIO-HOST.md`,
  `docs/reports/T16-NATIVE-HOT-SWAP.md`, `spec/language/reload.md`.
- **Périmètre** : `examples/dsp-gain-iir/**`, `crates/ravum-lang-audio/tests/**`,
  `crates/ravum-lang-audio/examples/**` (nouveaux fichiers), rapport. Lecture
  seule sur `ravum-lang-jit`.
- **Travail** : vertical qui fait tourner le DSP de l'exemple sur l'hôte WAV
  (CI) via le JIT, sortie comparée à la voie `rustc` ; édition à chaud
  (gain/coefficient) ; **mesure du fondu** entre versions (définir la mesure :
  discontinuité max entre le dernier échantillon de l'ancienne version et le
  premier de la nouvelle, et longueur de fondu si un fondu existe) ; 0
  allocation dans `process`. Ne pas élargir l'état au-delà de `{ z1, z2 }` (L5).
- **Fini quand** : test CI vert, mesure rapportée avec SHA et commande.

### L5 — T8 : RFC ABI DSP 2

- **Contexte** : `docs/rfc/RFC-E11-DSP-BLOCK.md` (§6 point 1),
  `docs/rfc/RFC-RV-DSP-BLOCK.md`, `docs/decision/E10-FINAL-DECISION-REQUEST-2026-11-19.md`,
  `crates/ravum-lang-jit` (bloc DSP à état `struct State` déjà côté JIT).
- **Périmètre** : `docs/rfc/RFC-DSP-ABI-2.md` (nouveau) uniquement.
- **Travail** : RFC `PROPOSED` : layout de l'état (struct `.rv` libre,
  contraintes `Copy`/taille), digest de schéma, compatibilité avec l'ABI 1
  `{ z1, z2 }` (COMPAT), migration (HR-1.7), sûreté (pas de `unsafe` nouveau
  ou sites listés), tests prévus, questions au responsable.
- **Fini quand** : RFC écrite, aucune ligne de code modifiée.

### L6 — T9 : E2E navigateur

- **Contexte** : `packages/http-notes` (HTTP + SQLite, `.rv`),
  `scripts/demo-m4-browser.ps1` (E2E navigateur de l'autre ligne, à imiter),
  `docs/qualification/PLATFORMS-MATRIX.md`. Chromium + Playwright sont
  disponibles dans les conteneurs Claude Code cloud ; ailleurs, les installer
  hors du dépôt.
- **Périmètre** : `packages/http-notes/**` (ajouts : page HTML servie si
  absente), `scripts/e2e-browser/**` (nouveau), `.github/workflows/e2e-browser.yml`
  (nouveau, **seul workflow autorisé** pour un chantier), rapport.
- **Travail** : construire et lancer `http-notes`, piloter un vrai Chromium
  (créer, lister, supprimer une note via la page), vérifier l'état SQLite ;
  négatifs : entrée invalide → erreur affichée, serveur arrêté → échec du
  test (pas de vert sans serveur). Pas de dépendance npm sans décision :
  préférer un script qui n'ajoute rien au dépôt, ou le proposer.
- **Fini quand** : test exécuté localement (log joint), workflow écrit
  (non activé tant que le responsable ne l'a pas fusionné).

### L7 — T6 : RFC async `connect` + HTTP

- **Contexte** : `spec/language/stdlib.md` (STD-17 TCP, STD-19 HTTP),
  `spec/language/concurrency.md`, ligne T6 de la matrice (« `connect`
  bloquant, pas d'HTTP asynchrone »).
- **Périmètre** : `docs/rfc/RFC-ASYNC-NET-1.md` (nouveau) uniquement.
- **Travail** : RFC `PROPOSED` : API, effets, annulation, délais, erreurs,
  interaction TSan, tests prévus, alternatives, questions.
- **Fini quand** : RFC écrite, aucune ligne de code modifiée.

## 4. Conflits connus et parade

| Fichier / zone | Chantiers | Parade |
|---|---|---|
| `Cargo.lock` | L2, L4, L6 | aucune dépendance nouvelle sans décision ; si `Cargo.lock` change quand même, le signaler, l'intégrateur régénère |
| `crates/ravum-lang-jit` | L2 (écrit), L4 (lit) | L4 n'utilise que l'API publique existante |
| `conformance/cases` | L3 (et L2 s'il ajoute des cas) | noms de cas préfixés par le chantier (`l2-…`, `l3-…`) |
| `docs/roadmap/*`, `docs/decision/*` | tous | réservés à l'intégrateur |

## 5. Gabarit du rapport `docs/reports/V2-<id>.md`

```
# V2-<id> — <titre>
| Champ | Valeur |
|---|---|
| Gate | T.. |
| Branche / SHA de départ / SHA final | |
| Outil / OS / matériel | (sans identifiant de modèle) |
| rustc | |
## Objectif et périmètre
## Modification (fichiers)
## Tests positifs / négatifs (noms, résultats)
## Commandes exécutées (exactes)
## Résultats mesurés
## Limites, écarts, demandes hors périmètre
## État des preuves (TESTED / NOT_RUN / … — jamais PASS ni PROVED)
## Proposition à l'intégrateur (changement d'état de gate suggéré, non appliqué)
```

## 6. Prompt à copier-coller (remplacer `<ID>`)

Voir `docs/plan/v2/LAUNCH-PROMPT.md`.

## 7. Intégration (INT)

Une seule session (ou le responsable) : pour chaque branche terminée,
relire le rapport, `git merge --no-ff` dans la branche d'intégration après
accord du responsable, rejouer workspace + conformance, mettre à jour la
matrice et les décisions, proposer les changements d'état. Ordre de merge
conseillé : RFC (L1, L5, L7) → L3 → L2 → L4 → L6.

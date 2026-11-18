# Prompt de reprise — Ravum, plan « production ready » (E5 → E10)

> À coller tel quel dans une session cloud. Il est autosuffisant : il ne suppose
> aucune mémoire de la session qui l'a produit. Vérifier chaque fait avant d'agir.

## Mission

Poursuivre le plan `docs/roadmap/PRODUCTION-PLAN-2026-11-15.md` sur le dépôt
`Rwanbt/Ravum`, branche de base **`dev`**, en livrant **une PR par étape** vers
`dev`, commits Conventional, CI verte, et **aucun gate promu sans preuve mesurée**.

Étapes restantes : **E5** (suite), **E6**, **E7** (fin), **E8**, **E9** (fin),
**E10** (préparation uniquement). Ne jamais fermer une issue sans lecture
point par point de ses critères.

## Règles non négociables

1. **Aucun gate promu.** T2 reste `PARTIAL` (344 MUST non mappés, D10). T4 et
   T12 restent `BLOCKED` (décisions humaines). Tout passage à `PASS` exige une
   commande, une sortie et un SHA. Un `SKIP` ou `NOT_RUN` n'est jamais un `PASS`.
2. **Pas de tag, pas de release, pas de signature, pas de merge vers `main`**
   sans accord explicite du responsable.
3. **Branches : `dev` et `main` uniquement** en fin de session. Supprimer chaque
   branche de tâche après fusion (`gh pr merge --delete-branch`).
4. **Ne jamais affaiblir un test, un seuil ou un contrat** pour obtenir du vert.
   Proposer le changement et demander l'approbation.
5. **Vérifier avant de pousser** : `git branch --show-current`, `git log -1`,
   `git status --short`. Une erreur de branche a déjà été commise dans cette
   session (un commit atterri sur `dev` par erreur, corrigé par `reset`).
6. **Ne pas écrire de script multi-lignes dans un bloc PowerShell** avec un
   here-doc : sur ce poste, le shell ne l'accepte pas. Écrire le script dans un
   fichier (`Write`), puis l'exécuter.
7. **Les seuils de couverture sont des planchers ratchet** : ne jamais les
   abaisser. Une découpe de fichier se re-baseline depuis l'artefact CI
   `coverage-frontend` (`gh run download`), avec un agrégat de module.

## État vérifié au moment de la rédaction

- `dev` = `20e5f8f` (merge de #141).
- Branches distantes : `dev`, `main`. Arbre propre.
- PR ouverte : **#142** (`docs/e9-t5-verified`), checks `mvp` encore en attente
  au dernier relevé. Vérifier son état avant toute action dessus.
- Issues ouvertes : **89** (90 au tri initial, moins #91 fermée).

### Réalisé (fusionné dans `dev`)

- **E1** divergence `check → build` fermée (abaissement par accès).
- **E2** garde-fous CI : Clippy `-D warnings`, cargo-deny, Dependabot,
  CODEOWNERS, CHANGELOG, MSRV 1.95.
- **E3** véracité documentaire : matrice 1 022 → 199 lignes, archive
  `docs/roadmap/archive/`, `evidence/current.json` régénéré.
- **E4** six fichiers > 1 500 lignes découpés (`emit`, `typeck`, `parser`,
  `loader`, `own`, `resolve`), S7 identique octet pour octet, 6/6.
- **E5-C1** `OwnFacts::copy_type` publiée ; divergence `Slice`/`Copy` corrigée
  (OWN-2.1/2.2 : une slice est mobile).
- **E7 (moitié)** campagne nominale : run `43753630956`, **6 000 programmes,
  0 divergence, 100 % acceptés**.
- **E9 (début)** : document `docs/roadmap/ISSUE-TRIAGE-2026-11-15.md`.
  - #91 (T3) **fermée « completed »** après vérification critère par critère.
  - #88 (T0) **maintenue ouverte** : son rapport `T0-AUDIT.md` est `PARTIAL`.
  - #93 (T5) **maintenue ouverte** : parité debug/release échantillonnée, et
    contradictions `ABI-2.5`/`ABI-3.3` dans `spec/language/abi.md`.

## Ce qui reste, dans l'ordre

### E5 — `OwnFacts` étape C (RFC-OWN-FACTS-2)

Lire `docs/rfc/RFC-OWN-FACTS-2.md` en entier. Faits : C1 fait. Reste :

- **C2 `owned_position`** : le checker doit **enregistrer** la position possédée
  pendant `check_expr` / `check_stmt` (`crates/ravum-lang-sema/src/own/`). C'est
  de l'instrumentation du cœur du vérificateur, pas un déplacement de code. Le
  garde de l'étape B (`verify_ownership_facts`, `emit/mod.rs`) doit être étendu,
  et il doit rester **non vide** : tester qu'il échoue sur un désaccord injecté.
- **C3 `consumed`** avec propagation `match` / `if` / bloc.
- **C4 `binding_identity`** : une identité unique par paramètre.
- **C5** : retirer `Emitter::is_copy_type`, `is_owned_position`,
  `consumed_by_checker` une fois l'équivalence prouvée. S7 identique octet pour
  octet à chaque étape.

Preuves exigées par étape : tests des deux crates, garde non vide, S7,
`ravum-conformance run --verify-build` (503 / 502 PASS / 0 FAIL / 1 SKIP
attendu avant changement), oracle différentiel.

### E6 — couverture 100 % et mutation

Dette mesurée en code produit : `emit` 84,89 %, `loader` 80,65 %, `own` ~91 %,
`resolve` ~94 %, `typeck` ~94 %. Mesurer depuis l'artefact CI, pas en local.
Objectif S5 : 100 % des lignes et régions du **code produit**, hors
`#[cfg(test)]`. Les chemins réellement inatteignables se documentent un par un
dans `tools/coverage_exemptions.json` (PENDING_HUMAN_APPROVAL), jamais par
abaissement de plancher. Campagne `emit.rs` complète en CI (`mutation.yml`).

### E7 — fin du test de puissance T2

Faits : campagne nominale 3 × 2 000 **faite** (run `43753630956`). Reste :
exécuter les **4 inverses `NOT_RUN`** de la méthode S6 (`docs/decision/DECISIONS-2026-11-14.md`)
et calculer le score sur 13 formes. Consigner dans le journal avec les chiffres
lus dans le log du job, pas déduits.

### E8 — interfaces et distribution

Workflow de release (artefacts Windows / Linux), SBOM, provenance attestée
(`actions/attest-build-provenance`, P-10 : confirmation humaine requise avant la
première release), canal de dépréciation, liste de ratification D2 **proposée**
(jamais acceptée par un agent), contrat d'installation. Aucune release.

### E9 — tri des issues (fin)

89 issues ouvertes. Méthode : pour chaque issue de gate (`RAV-LANG-T*`), lire
le corps, lister ses critères de PASS, vérifier chacun contre une preuve mesurée
citée, **puis** consigner dans le document de tri, **puis** fermer. Dans cet
ordre (une erreur d'ordre a été commise avec #91).

Restent à vérifier : T1, T2, T4, T6, T7 (#95), T8, T9, T10, T11, T12. T4 et T12
sont `BLOCKED` par décision humaine : ne pas fermer.

Les 9 épopées `[EPIC][E0x]` restent ouvertes : elles relèvent de la plateforme
Rust expérimentale, hors promesse 1.0 (P-13).

### E10 — préparation de la décision finale (humaine)

Ne pas rendre de verdict. Produire le dossier : `docs/audit/AUDIT-BRIEF.md` à
jour, liste des décisions humaines restantes (D2, D4, D9, T12), état honnête de
chaque gate, et la demande explicite d'audit indépendant, de ratification et de
Go/No-Go. L'audit (D4) ne peut pas être mené par un agent.

## Méthode de travail

- Une PR par étape, base `dev`, commit Conventional, description qui cite les
  chiffres et les commandes.
- Avant de fusionner : `gh pr checks <n>` jusqu'à 0 `pending`/`fail`, puis
  `gh pr view <n> --json mergeStateStatus` = `CLEAN`.
- Mesurer, puis écrire. Jamais l'inverse.
- Tout chiffre cité dans un document doit venir d'une sortie réelle de commande
  ou d'un log de job. Un chiffre inventé ou recopié de mémoire est un défaut.

## Commande de démarrage

```powershell
git fetch origin; git checkout dev; git pull --ff-only
git log --oneline -3
gh pr list --state open
gh issue list --state open --limit 200 --json number,title
```

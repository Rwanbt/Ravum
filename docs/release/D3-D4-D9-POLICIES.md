# Politiques D3 / D4 / D9 — fusion, promotion, release, audit indépendant, signature et custody

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL`.** Ce document **propose** des
> politiques ; il n'en applique aucune. Aucune fusion, aucun tag, aucune release,
> aucune signature, aucun audit, aucune génération de clé n'est effectuée ici.
> Les actes réservés au responsable (`main`, tag, release, approbation, signature)
> restent les siens. Date : 2026-11-11.

## 0. Faits vérifiés avant d'écrire des règles

| Fait | Valeur observée | Comment |
|---|---|---|
| Branche de travail | `feat/lang-core-1.0-rc` ; tête à la rédaction `5d88ecb`, puis `7e0c77a` (2026-11-11T15:49Z) — la tête avance, vérifier avant toute fusion | `git rev-parse`, API GitHub |
| PR | **#101 ouverte, draft, non fusionnée**, base `dev`, `mergeable_state: clean` ; 126 commits / 1616 fichiers à `5d88ecb`, 147 commits / 1834 fichiers à `7e0c77a` | API GitHub `GET /pulls/101`, 2026-11-11 |
| Releases publiées | **aucune** (`[]`) | API GitHub `GET /releases` |
| Tags | **aucun** (`[]`) | API GitHub `GET /tags` |
| CI sur `5d88ecb` | **12 runs** pour ce SHA ; celui de qualification Linux a été vérifié job par job : 7 `success` + TSan `skipped` | API GitHub `GET /runs?head_sha=…` et `/runs/42936163958/jobs` |
| CI sur `c67f17e` (passe suivante) | **12 runs de workflow** ; le run Linux `43026622427` est vert (7 `success` + TSan `skipped`), les 12 sont rapportés verts | API GitHub ; détail `docs/qualification/LINUX-QUALIFICATION.md` §11 |
| Licence | `MIT OR Apache-2.0`, 23/23 membres | `DECISIONS_REQUIRED.md` D1 (RÉSOLUE) |
| Signature | mécanisme `ssh-keygen -Y sign` qualifié, **clé absente** | `docs/qualification/SIGNATURES.md` |
| Audit externe | **aucun** | matrices de gates T4/T10/T12 |

Ces faits sont datés. Une politique n'est pas une observation : elle dit ce qu'il
faudrait faire, pas ce qui a été fait.

## 1. D3 — fusion vers `dev`, promotion vers `main`, release stable

### 1.1 Fusion vers `dev` (intégration)

**P3.1 — Une PR vers `dev` n'est fusionnable que si tous ces points sont vrais :**

1. la branche est à jour de `dev` ou le diff a été relu après rebase (pas de
   fusion silencieuse d'un conflit) ;
2. l'arbre est **propre** (aucune modification non commitée) et `Cargo.lock` est
   cohérent avec `Cargo.toml` (`--locked`) ;
3. les workflows **bloquants** sont verts sur le SHA de tête :
   `linux-qual.yml` (workspace, conformance, smoke, installateur, WASI, Miri,
   fuzz), `gate-contracts.yml` (couverture stricte, registre, contrats),
   `coverage.yml` (seuils), `docs-check.yml` ;
4. `vscode-e2e.yml` et `mvp.yml` sont verts ou leur échec est expliqué dans la PR
   avec la raison et la mesure (ce sont des preuves d'environnement, pas des
   gates de sémantique) ;
5. aucune régression de contrat : `python tools/features_coverage.py --strict`
   exit 0, `python tools/registry_audit.py` exit 0, `python -m unittest
   discover -s tools/tests` vert ;
6. aucun test supprimé ni assoupli sans décision humaine (AGENTS.md) ; un test
   modifié cite la règle qui l'exige ;
7. la PR nomme le SHA mesuré et les chiffres (pas « les tests passent »).

**P3.2 — Interdits à la fusion :** force-push sur une branche partagée,
réécriture d'historique, fusion d'une branche dont la CI a été contournée,
fusion d'un commit qui modifie un workflow de gate pour le rendre non bloquant
sans décision humaine. Un agent ne fusionne pas ; la fusion est un acte humain
ou explicitement délégué.

**P3.3 — Méthode de fusion proposée :** commit de fusion (pas de squash) pour
conserver la traçabilité des SHA mesurés ; message de fusion qui cite le SHA des
preuves et le run CI ; pas de tag sur `dev`.

### 1.2 Promotion vers `main`

**P3.4 — `main` n'est pas une preuve de maturité.** C'est la référence
documentaire (AGENTS.md). Une promotion vers `main` exige, en plus de P3.1 :

1. **D3 tranchée** : le responsable a écrit que la promotion est autorisée, avec
   la liste des gates acceptés en l'état ;
2. **D4 tranchée** : audit indépendant rendu (ou exemption d'audit écrite,
   datée, signée, avec la liste de ce qui n'est pas audité) — T4/T10/T12 ne
   peuvent pas rester `BLOCKED` sans que la promotion l'assume explicitement ;
3. **T2 sans faux `PASS`** : les exemptions (dont `F-META-006`) sont nommées et
   approuvées, et la distinction « 188 FeatureIds » vs « 807 MUST » est assumée
   (voir `docs/qualification/MUST-TRACEABILITY.md`) ;
4. **artefact de release candidate** produit à partir du SHA de `dev`, avec
   checksums, SBOM et manifeste de provenance (D9 non résolue ⇒ artefact **non
   signé**, donc non présenté comme provenance vérifiable) ;
5. **aucun gate `FAIL`** ; les `PARTIAL`/`BLOCKED` acceptés sont listés avec
   leur conséquence pour l'utilisateur ;
6. protection de branche : PR obligatoire, revue, CI requise, pas de push direct,
   pas de suppression de `main`.

**P3.5 — Ce qui est explicitement hors de cette politique :** le choix de la ou
des personnes autorisées à fusionner dans `main` et à approuver les exemptions ;
il appartient au responsable (D3/D4).

### 1.3 Release stable

**P3.6 — Une release stable exige tout P3.4, plus :**

1. **audit indépendant clos** (D4) : rapport daté, périmètre, findings, verdict
   par gate ;
2. **signature et ancre de confiance** (D9) : clé de projet, `allowed_signers`
   publié, vérification rejouée, artefact altéré rejeté ;
3. **reproductibilité** : la procédure mesurée (`scripts/repro-check.ps1`,
   bit-à-bit sur deux racines) est rejouée **sur le SHA de release**, pas
   seulement sur un SHA antérieur ;
4. **installation testée** sur chaque plateforme annoncée (voir
   `PLATFORMS-MATRIX.md`) ; une plateforme sans test d'installation n'est pas
   annoncée ;
5. **SBOM** régénéré et vérifié déterministe à époque fixée ;
6. **notes de release** honnêtes : ce qui est `UNSUPPORTED`, ce qui est
   `PARTIAL`, ce qui n'est pas prouvé (pas de « production-ready ») ;
7. **version** : `0.x` tant que les ADR ne sont pas ratifiés et que l'édition
   reste `core-1.0-draft.N` ; une version `1.0.0` exige la ratification (D2) et
   la fin des décisions bloquantes D2/D4/D6/D7/D9.

**P3.7 — Release candidate interne :** autorisée avant tout cela, **non
publiée**, liée à un SHA, avec artefacts et checksums, explicitement marquée
`CORE_RC_CANDIDATE`. C'est l'état actuel recommandé par D3.

**P3.8 — Rollback :** revenir en arrière = commit de revert (jamais de
réécriture), « yank » de la release publiée, conservation des artefacts et des
signatures ; un artefact retiré reste vérifiable par son checksum mais n'est
plus attribué au projet s'il n'est pas re-signé (repris de `KEY-CUSTODY.md` §4).

## 2. D4 — audit indépendant

**P4.1 — Indépendance, définition opérationnelle.** L'auditeur :

- n'a pas écrit le code audité (ni les crates, ni les tests, ni la spec) ;
- ne partage pas le contexte de session des auteurs (pas de « relire ce que
  j'ai fait ») ;
- reçoit le dépôt, la toolchain épinglée et les commandes de reproduction, pas
  une synthèse de résultats ;
- peut être un humain ou un agent **outillé**, mais seul un humain signe le
  verdict ; un audit mené par un agent reste un audit interne (matrice T4).

**P4.2 — Périmètre minimal proposé.** Dans cet ordre :

1. **borrow checker** (`own.rs`) : partial moves, prêts, échappement,
   interprocédural — le point où un faux négatif casse la sûreté ;
2. **codegen** (`emit.rs`, `prelude.rs`) : `unsafe` généré, wrapper FFI,
   absence de contournement du checker, déterminisme ;
3. **frontière FFI** : ce que la surface permet réellement (scalaire → scalaire),
   ce qu'elle prétend, et la TCB incluant la bibliothèque C appelée ;
4. **loader/résolution** : chemins, symlinks, confidentialité des modules,
   manifestes ;
5. **contrats de sortie** : codes de diagnostic, JSON, phases, exit codes ;
6. **preuves** : ce que le corpus, la mutation, la couverture et Miri ne
   couvrent **pas** (la liste des trous doit être dans le rapport).

**P4.3 — Méthode exigée.** Lecture du code + tests adversariaux exécutés +
reproduction indépendante ; chaque finding a : sévérité, fichier/ligne, programme
minimal, comportement observé, comportement attendu (règle citée), statut
(`FIXED`/`OPEN`/`ACCEPTED_RISK`). Un finding « accepté » doit être signé par le
responsable, pas par l'auditeur.

**P4.4 — Sortie et conservation.** Rapport daté, SHA, toolchain, plateforme,
commandes, verdict par gate (T4/T10/T12), liste explicite des `NOT_COVERED`.
Publication ou archivage avec hash ; le rapport ne doit pas réécrire les
résultats antérieurs.

**P4.5 — Critère de clôture (proposé).** Aucun finding `CRITICAL`/`HIGH`
inexpliqué ; les `MEDIUM` sont corrigés ou acceptés par écrit ; le rapport dit
ce qu'il n'a pas couvert. Un audit « vert » qui ne liste aucun trou n'est pas
recevable.

## 3. D9 — signature, provenance, custody de clé et ancre de confiance

**P9.1 — Ce qui est signé.** Un manifeste unique liste les hashes de : binaire(s),
SBOM SPDX, checksums, manifeste d'installation, rapport de conformance, notes de
release. La signature porte sur le manifeste (et, si l'outil le permet, sur
chaque fichier). Une signature qui ne couvre pas le binaire ne prouve rien.

**P9.2 — Mécanisme.** `ssh-keygen -Y sign/verify`, Ed25519, namespace
`ravum-release` — déjà qualifié (artefact intact accepté, artefact altéré rejeté,
`docs/qualification/SIGNATURES.md`). Aucun autre mécanisme n'est revendiqué
(GPG, sigstore, SLSA) tant qu'il n'est pas qualifié de la même façon.

**P9.3 — Custody (reprise de `KEY-CUSTODY.md`, option B recommandée).** Clé
privée hors ligne sur machine dédiée (air-gap) ; clé de secours hors ligne ;
propriétaire nommé ; lieu de stockage décrit ; rotation planifiée (12 mois) ;
procédure de compromission écrite ; la clé n'est **jamais** dans le dépôt, jamais
dans un secret CI utilisé pour signer, jamais sur le poste de développement.

**P9.4 — Ancre de confiance.** `docs/release/allowed_signers` publié dans le
dépôt, relu en revue, **et** copié hors bande (page de release / site) pour que
la compromission du dépôt seul ne suffise pas à réécrire la liste de confiance.
La politique doit dire **quelle copie fait foi** et comment un utilisateur la
vérifie ; sans cela, une signature vérifie « une clé publiée à côté de
l'artefact », ce qui n'atteste pas grand-chose.

**P9.5 — Provenance, sans surpromesse.** Le paquet de provenance proposé :
manifeste d'installation (`git_commit`, `git_dirty`, `sha256`, `rustc`,
`locked`), SBOM, checksums, commande de build exacte, SHA, toolchain, rapport de
conformance, signature. **Ne sont pas revendiqués** : reproductibilité bit-à-bit
sur une autre machine (la procédure mesurée est locale et à drapeaux), SLSA,
attestation de build, ni « built from source » vérifiable indépendamment.

**P9.6 — Qui peut signer.** Une ou deux personnes nommées, désignées par le
responsable ; un agent prépare les hashes et vérifie, il ne détient pas la clé et
ne signe pas. Un agent qui générerait une clé et signerait lui-même produirait
une preuve circulaire (déjà dit dans `DECISIONS_REQUIRED.md` D9).

**P9.7 — Ce qui débloque T11.** T11 reste `PARTIAL` tant que P9.3/P9.4 ne sont
pas vrais ; le mécanisme qualifié ne suffit pas (matrice T11).

## 4. Rôles et séparation

| Acte | Agent | Responsable / mainteneur |
|---|---|---|
| Mesurer, rédiger, proposer, vérifier | oui | — |
| Fusionner vers `dev` | non (politique proposée) | oui, ou délégation écrite |
| Fusionner vers `main` | **jamais** | oui, après D3+D4 |
| Taguer / publier une release | **jamais** | oui |
| Approuver une exemption (`F-META-006`, etc.) | **jamais** | oui |
| Signer un artefact | **jamais** (pas de clé) | personne nommée pour D9 |
| Ratifier un ADR/RFC | **jamais** | oui |
| Auditer | peut outiller, ne signe pas | auditeur humain indépendant |

## 5. Ce qu'un humain doit décider (liste courte)

1. **D3** : autoriser ou non la fusion de #101 vers `dev` (elle est `clean` et
   non fusionnée à ce jour), puis la promotion vers `main` ; choisir la méthode
   de fusion.
2. **D3** : accepter ou non une release candidate **non signée** comme
   livrable interne, et sous quel nom.
3. **D4** : mandater l'audit, nommer l'auditeur, fixer le périmètre §2 et le
   critère de clôture §2.5.
4. **D9** : choisir l'option de custody, nommer le détenteur de la clé, décider
   **où** `allowed_signers` fait foi (dépôt seul ou copie hors bande).
5. **D2/D6/D7** : les décisions de langage restent ouvertes ; aucune release
   stable ne doit les précéder.

## 6. Ce que ce document ne fait pas

- Il ne fusionne rien, ne crée aucun tag, ne publie aucune release, ne signe
  rien, ne génère aucune clé.
- Il ne ratifie ni D3, ni D4, ni D9 ; il ne remplace pas `KEY-CUSTODY.md`,
  `PROVENANCE.md`, `RELEASE-CHECKLIST.md` ni `SIGNATURES.md`, qu'il cite.
- Il ne présente pas les faits §0 comme une autorisation : une PR `clean` n'est
  pas une PR approuvée, et une CI verte n'est pas un audit.

# Décisions humaines indispensables — Ravum Core 1.0

> Ces décisions **n'appartiennent pas** aux agents. Ce dossier les rend explicites,
> avec recommandation, preuves disponibles, alternatives et conséquence d'une
> non-décision. Aucune n'est prise ici. Dernière mise à jour : 2026-11-09.

## D1 — Licence du code et gouvernance (#34)

- **Recommandation** : ne rien publier/distribuer tant que la licence n'est pas
  choisie ; conserver le dépôt privé/interne et compiler localement.
- **Preuves** : `docs/mvp/MVP-0b-licence-analysis.md` (analyse existante, aucune
  licence choisie) ; aucun fichier `LICENSE` ajouté par cette itération.
- **Alternatives** : permissive (MIT/Apache-2.0), copyleft, licence propriétaire,
  double licence.
- **Sans décision** : pas de diffusion publique, pas de publication de paquets,
  mais le développement/test interne continue.

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
  `NOT_RUN` tant qu'aucun runner/CI n'exécute réellement les E2E sur le SHA.
  Ne pas annoncer Linux Tier 1 sans exécution réelle.
- **Sans décision** : les gates multi-OS restent `PARTIAL/NOT_RUN`.

## D6 — Périmètre des fonctionnalités universelles (T8-T11)

- Trajectoire : Core 1.0 RC limité au sous-ensemble documenté ; hot reload,
  WASM/WASI, FFI, threads, GPU/mobile, hard-RT restent `UNSUPPORTED` ou
  expérimentaux jusqu'à qualification dédiée.
- **Sans décision** : pas d'extension de promesse ; le langage reste honnête sur
  ses limites.

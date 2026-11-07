# Décisions à arbitrer, ressources et stratégie de lancement

## Décisions prioritaires (revue architecture F0)

| Priorité | ADR à examiner | Question décisive | Proposition de départ | Preuve à exiger |
|---|---|---|---|---|
| P0 | ADR-000, ADR-038 | Qu'est-ce que « universel » au lancement ? | noyau commun ; Linux/Windows/WASM d'abord | 3 démos ciblées, support matrix |
| P0 | ADR-002, ADR-003 | Quel modèle mémoire / erreurs ? | ownership, explicit Result, RT panic interdit | fixtures compile-fail et runtime |
| P0 | ADR-004, ADR-014 | Quel contrat est « prouvé » ? | propriétés explicites + TCB traçable | corpus faux / unknown / vacuité |
| P0 | ADR-005, ADR-006 | Quels effets et profils RT ? | effets transitifs + 3 niveaux RT | exemple audio et tests interdits |
| P0 | ADR-010, ADR-012 | Quelle IR et incrémentalité ? | CST→HIR→MIR; queries à grain fin | oracle froid/chaud |
| P0 | ADR-013, ADR-015 | Comment ne pas réutiliser une preuve fausse ? | key contextuelle, fail closed | mutation de chaque dépendance |
| P0 | ADR-017, ADR-018 | Backend de départ ? | Rust sûr d'abord, Cranelift benchmark | 3 jeux, runtime/codegen comparés |
| P0 | ADR-019, ADR-020 | Quel mode de compilation vivante ? | daemon par projet, overlays et budget RAM | charge multi-projets p95/RSS |
| P0 | ADR-021, ADR-022 | Quand activer un patch ? | seulement compatibilité ou migration prouvée/testée | stress race / rollback |
| P0 | ADR-027, ADR-028 | Qui peut toucher aux spécifications ? | humain signe ; agent propose | agent adversarial modifiant contrat |
| P1 | ADR-030, ADR-031 | Que faut-il pour accepter un livrable ? | SHA exact, gates T0-T4, release scoped | audit CI négatif / provenance |
| P1 | ADR-032 | Quel matériel et SLO ? | au moins PC léger et PC standard | campagne de benchmark publique |

**Décisions conditionnelles** : ces orientations ne passent en `ACCEPTED` qu'après revues et expériences. Les inconnues doivent rester ouvertes plutôt qu'être cachées derrière une syntaxe précipitée.

## Équipe et profil d'effort

Ce chantier réunit plusieurs projets de recherche comparables à un compilateur, un runtime patchable, un système de preuve et une plateforme développeur. **Une version universelle, vérifiée, multi-cible et de qualité industrielle doit être envisagée sur plusieurs années et sous condition de financement/revue externe** ; elle ne peut être honnêtement promise en quelques semaines.

Ordre de recrutement / contribution si équipe : (1) spécialiste compilateur/semantics, (2) formal methods/SMT, (3) ingénieur runtime/concurrence/RT, (4) CI/perf/sécurité, (5) DX/LSP/IA, (6) web/data/ports. Si une seule personne lance le projet avec des agents IA, conserver ce découpage sous forme de revues externes indépendantes pour la sûreté critique.

## Budget de ressources

- Préférer Rust et outillage open source local pour construire le compilateur ; ne pas créer de dépendance obligatoire à un service IA payant.
- Chaque solveur, sandbox, linker, SDK, compilateur GPU ou outil de preuve doit être inventorié par version, licence, performance, sécurité et contraintes redistributives.
- Les budgets CPU/RAM du daemon sont **configurables** et mesurés ; priorité aux tâches interactives, annulation du travail obsolète et mode hors ligne.
- Les dépenses de cloud, tests sur device et audits externes sont à budgéter lors des décisions de qualification. Aucun devis chiffré n'est inventé dans ce dossier.

## Pilotage hebdomadaire (cadence suggérée)

- Début : choisir un risque / une hypothèse falsifiable / un responsable.
- Pendant : produire code + tests RED→GREEN + logs / mesure / SHA.
- Fin : rapport de preuves et de risques, mise à jour matrice requirement→ADR→spike→CI, Go/No-Go ; sélectionner le prochain chemin critique.
- Mensuellement : revoir ADR ouvertes, performances froid/chaud, ressources, fausses preuves, compatibilité cibles, et arbitrer pivots.

## Livrables finaux attendus avant première release profilée

1. Spécification langage versionnée et tests de conformité exécutables.
2. Compilateur toolchain stable, build/packaging propre sur cibles annoncées.
3. Rapport de confiance couvrant codegen, solveurs, caches et FFI.
4. Benchmark public reproduisible, p95 et ressources sur hardware défini.
5. Rapport de sécurité, fuzzer, tests d'intégration, protocole gestion vulnérabilités.
6. Suite de tests RT/fullstack/SQL selon les profils effectivement supportés.
7. Documentation API IA/LSP et migrations, restrictions et chemins d'erreur.
8. SBOM, attestations de provenance et politique de release/rollback.

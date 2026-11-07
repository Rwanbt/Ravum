# Threat model, risques et plans de réponse

## Actifs à protéger

Code utilisateur, contrats normatifs, secrets locaux, preuves/cache, binaire généré, stabilité de l'application en cours, données de BDD, thread temps réel, contrôles CI, environnement de compilation.

## Adversaires et frontières

1. Source hostile ou malformée qui cible parser, solver, compiler, FFI ou linker.
2. Dépendance/macro/build script malveillant ; supply-chain injectée.
3. Agent IA non malveillant mais erroné, ou instruction hostile glissée dans code/documentation.
4. Module patch incompatible ou adversarial ; état incohérent à migration.
5. Entrées réseau/client non fiables, secret serveur exposé à frontend.
6. Fautes OS/matériel/driver hors modèle, saturation ressources et dénis de service.

**Frontières de confiance** : buffer→parser, HIR→MIR, IR→SMT, IR→codegen, runtime→FFI/OS, agent→contrats/CI, composant→WASI/hôte, build→sources dépendantes, patch→application vivante. Chaque frontière nécessite contrats, validation des entrées et journal des hypothèses.

## STRIDE condensé

| Menace | Scénario | Mesure |
|---|---|---|
| Spoofing | agent falsifie preuve/SHA | empreinte contextuelle, signature, CI read-only |
| Tampering | modifie contrat/axiome/certificat | dépôt protégé, hash et validation de schéma |
| Repudiation | patch actif non attribuable | trace de révision/intention/auteur |
| Information disclosure | outil CI lit .env/secret, preview fuit DB | sandbox/capacités/redaction |
| Denial of Service | solver ou parser consomme tout RAM/CPU | budgets, timeout, annulation et isolation |
| Elevation | macros/FFI contournent permissions | sandbox, audit unsafe, moindre privilège |

## Top 12 risques et mesures

| ID | Gravité | Risque | Mitigation / pivot |
|---|---|---|---|
| R-01 | Critique | Fausse preuve par cache périmé | mutations contextuelles + oracle fresh, fail closed |
| R-02 | Critique | Unsoundness du modèle ownership ou lowering | différentiel, fuzzing, revue modèle, backend Rust bridge |
| R-03 | Critique | UAF à chaud pendant appel in-flight | epochs/safepoints, tests stress, refus patch |
| R-04 | Critique | Suppression/affaiblissement de contrats par IA | normative locks, review et policy-as-code |
| R-05 | Élevée | RT callback bloque ou alloue indirectement | effets transitifs, instrumentation et host réel |
| R-06 | Élevée | Une base prod diverge du schéma validé | version check runtime, migration N/N-1 et gate |
| R-07 | Élevée | Solver ou compilateur saturent poste utilisateur | CPU/RAM quotas, cancel, mode batch/offline |
| R-08 | Élevée | Rust transpile trop lent | benchmark Cranelift direct, pipeline optimisé |
| R-09 | Élevée | Dépendance compromise/exfiltration | lockfiles, sandbox, provenance, offline default |
| R-10 | Élevée | Mauvaise interprétation de « PROVED » | statuts, assumptions et TCB en UI/rapport |
| R-11 | Élevée | Écart sémantique multi-target/flottants | profils num, oracle differential, gate target |
| R-12 | Moyenne | Trop grande ambition/maintenance | profiles, épics séquentiels, Go/No-Go par phase |

## Plan de réponse sécurité

Sévérité P0 : bloquer merge/activation, reproducer, isoler cible, conserver logs et SHA, ouvrir issue privée si vulnérabilité, patch avec tests RED→GREEN, revue indépendante, rétroanalyse des preuves déjà émises et invalidation/correction du cache. Ne pas effacer les artefacts d'échec.

## Critères de conservation des garanties

- Chaque règle de sécurité doit avoir au moins un test **positif** et un **négatif**.
- Les hypothèses `unsafe`/FFI doivent être listées dans le manifeste ; pas de prouveur opaque donnant confiance implicite.
- Les comportements extérieurs non modélisés sont marqués comme tels, notamment les événements réseau réels, erreurs matérielles et ordonnancement OS.

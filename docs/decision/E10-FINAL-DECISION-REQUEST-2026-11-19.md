# E10 — Demande de décision finale (Core 1.0)

| Champ | Valeur |
|---|---|
| Date | 2026-11-19 |
| Base | `dev` @ `c96d80a` (après #201) |
| Auteur | agent de développement |
| Statut | **demande** — aucune décision n'est prise par ce document |

Ce dossier rassemble ce qui est **mesuré** et ce qui ne peut être décidé que
par le responsable. Aucun gate n'est promu par l'agent ; aucune RFC n'est
acceptée par l'agent ; aucun tag, aucune release, aucune signature, aucune
fusion vers `main` n'a lieu sans votre accord.

## 1. Ce qui est mesuré (dernière observation)

| Mesure | Valeur | Source |
|---|---|---|
| CI de la dernière PR (#201) | 18/18 checks `success` (Linux, macOS, Windows, TSan, couverture, reproductibilité, VS Code) | GitHub Actions sur `eb58c6a` |
| Conformance (local, Linux) | 727 cas : 725 PASS, 0 FAIL, 2 SKIP (Windows seulement) ; sans `--verify-build` | `ravum-conformance run` |
| FeatureIds | 244/244 satisfaits au critère strict | `tools/features_coverage.py --strict` |
| Énoncés MUST | 876 ; **95 sans FeatureId** | `tools/must_audit.py` |
| TSan | 10 cas concurrents, 10 exécutions chacun en CI, std reconstruite avec TSan, témoin de course signalé | `.github/workflows/tsan.yml` |

SKIP et NOT_RUN ne sont jamais comptés comme PASS. Un run vert est une
observation sur ce corpus, cet hôte et ce commit, pas une preuve.

## 2. État des gates

| Gate | État | Ce qui manque | Qui peut le lever |
|---|---|---|---|
| T0 | `PASS` | — | — |
| T1 | `PARTIAL` | Voie C hors 1.0 ; choix final du backend non ratifié | **vous** (ADR) |
| T2 | `PARTIAL` | 95 MUST sans FeatureId (dette tracée, D10) | agent (travail) ; vous (accepter la dette) |
| T3 | `PASS` | — | — |
| T4 | `BLOCKED` | Audit externe humain du typechecker et du borrow checker | **auditeur externe** |
| T5 | `PASS` | ABI stable non revendiquée | — |
| T6 | `PARTIAL` | HTTP asynchrone absent, `connect` bloquant ; RFC E11 non ratifiées | vous (ratification) ; agent (HTTP async si voulu) |
| T7 | `PASS` | — | — |
| T8 | `PARTIAL` | État DSP limité à `{ z1, z2 }` (ABI 2 non ratifiée) ; `examples/dsp-gain-iir` non branché ; fondu non mesuré sur un bloc `.rv` | vous (ABI) ; agent (vertical) |
| T9 | `PARTIAL` | Vertical DSP hôte (`dsp-gain-iir`) ; E2E navigateur absent | agent ; vous (périmètre navigateur) |
| T10 | `PARTIAL` | Audit externe ; couverture loin de 100 % sur quelques fichiers | **auditeur externe** |
| T11 | `PARTIAL` | Aucune clé de projet, aucune ancre de confiance | **vous** (D9, custody de clé) |
| T12 | `BLOCKED` | Dogfooding indépendant ; Go/No-Go | **vous** |

## 3. Décisions demandées

1. **Ratifier ou amender les cinq RFC E11** (toutes `PROPOSED`), chacune avec
   les écarts d'implémentation notés dans son §Avancement :
   - `RFC-E11-CONCURRENCY` — CONC-2.1 (transférable = sans référence) comme
     substitut de `Send` ; effet `thread` ; TSan comme condition de `PASS` de T6 ;
     la règle CONC-6.3 conservatrice (aucune référence dans une `async fn`) ;
   - `RFC-E11-NET-HTTP` — texte seulement, HTTP dans le runtime et non en `.rv`,
     pas de TLS (serveur « loopback / LAN ») ;
   - `RFC-E11-SQLITE` — syntaxe `extern "C" from "<lib>"`, `CHandle` au lieu
     d'`extern type`, SQLite en paquet sans effet `db`, binaires non publiés ;
   - `RFC-E11-HOT-RELOAD` — module en processus plutôt qu'en `cdylib`,
     événement = ligne de texte, schéma structurel ;
   - `RFC-E11-DSP-BLOCK` — ABI DSP 2 inchangée (état `{ z1, z2 }`) ou nouvelle
     ABI à état libre ; traitement par échantillon.
2. **Ratifier la liste D2** (`D2-RATIFICATION-PROPOSAL-2026-11-18.md`).
3. **T1** : ratifier le backend de référence (voie B) pour la 1.0, voie C hors périmètre.
4. **T11 / D9** : désigner la garde de la clé de signature
   (`docs/release/KEY-CUSTODY.md`) ; sans clé, aucune release signée.
5. **T4 / T10** : commander l'audit externe, ou accepter explicitement une
   release candidate **sans** audit, écrite comme telle.
6. **T12** : organiser le dogfooding indépendant, puis prononcer le Go/No-Go.

## 4. Recommandation de l'agent

**No-Go pour une release 1.0 aujourd'hui.** Deux gates sont `BLOCKED` pour des
raisons que seul un humain peut lever (T4 audit externe, T12 Go/No-Go), et
aucune clé de signature n'existe (T11).

**Possible dès vos décisions 1 à 4** : un tag de release candidate interne
non signé (`0.1.0-rc`), annoncé comme non audité, pour lancer le dogfooding
de T12. L'agent ne crée ce tag que sur votre instruction explicite.

## 5. Travail restant que l'agent peut faire sans décision

- brancher `examples/dsp-gain-iir` sur l'hôte de blocs (T8/T9) ;
- HTTP asynchrone au-dessus de CONC-6.7 (T6), si vous le voulez dans la 1.0 ;
- réduire les 95 MUST sans FeatureId (T2).

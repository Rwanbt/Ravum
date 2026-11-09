# MVP-0d — rapport de cycle : incrémentalité, overlays, groupement

**Date :** 9 novembre 2026
**Branche :** `feat/mvp0d-continuous-collaboration`, créée depuis `dev` (`6b0ac7b`)
**Objet :** améliorer l'infrastructure existante du MVP-0a/0b/0c, sans refonte de
compilateur ni nouveau langage.

> **Rapport de cycle, pas un certificat.** Chaque affirmation ci-dessous est liée à un
> test exécuté ou à une mesure reproduite localement. Les gates non franchis sont
> marqués `PARTIAL` et leurs conditions sont explicites.

## 1. État au démarrage

| Élément | Valeur |
|---|---|
| `dev` après intégration | `6b0ac7b` — MVP-0a/0b/0c présents, chaîne propre documentée (`docs/mvp/INTEGRATION-0abc.md`) |
| Suite complète sur `dev` | 496 réussis / 0 échec / 79 suites, `fmt` et build verts, démos M1/M3/M4 et E2E navigateur verts |
| CI par impact sur une PR vers `dev` | fonctionnelle (51 s sur PR documentaire, suite produit conservée) |

## 2. Lot A — incrémentalité avancée

Détail : [`MVP-0d-incremental.md`](MVP-0d-incremental.md).

- activation Cargo résolue par `cargo metadata` (repli `--no-deps` ⇒ activation
  **inconnue**, jamais vide) ;
- `context_id` v2 : features activées, crate types, build script, kind ;
- `--cfg feature="…"` transmis à rustc ;
- normalisation des chemins (préfixe verbatim Windows) qui rendait le `.rmeta`
  non déterministe et le `content_id` absolu ;
- 2 cas de test ajoutés, 2 compteurs mis à jour avec justification, aucun test
  supprimé ni affaibli.

**Gate II : `PARTIAL`** — dépendances inter-crates couvertes, invalidation
conservative, fallback fonctionnel, résultats périmés rejetés (déjà MVP-0c),
nouvelle couverture features + déterminisme. Manquent : build scripts, dépendances
optionnelles, cibles multiples, et un gain de performance mesuré (ce lot est
qualitatif).

## 3. Lot B — overlays privés et intégration transactionnelle

Détail : [`MVP-0d-overlays.md`](MVP-0d-overlays.md).

- `BaseSnapshot` content-adressée, `Overlay` privé par agent, `ChangeCoordinator`
  tout ou rien ;
- base périmée → `RejectedStale` ; chemin partagé → `ConflictPath` sans application
  partielle ; chemins dangereux → `RejectedInvalid` ;
- `publish()` avance la base canonique ;
- 9 tests, dont concurrence (8 threads) et absence de perte (12 propositions) ;
- stockage mesuré : **262 144 o** d'overlays contre **3 276 800 o** pour 8 copies
  complètes, soit **−92,0 %** de duplication.

**Gate III : `PARTIAL`** — overlays isolés, propositions concurrentes, gestion des
conflits et vérification du code fusionné existent et sont testés. Manquent :
publication Git de bout en bout depuis les overlays, campagne A/B/C/D réelle, et
récupération après interruption du coordinateur en mémoire.

## 4. Lot C — groupement adaptatif

Détail : [`MVP-0d-batch-window.md`](MVP-0d-batch-window.md).

- `AdaptiveWindow` bornée (≤ 100 ms) : croissance sur densité, rétrécissement sur
  rareté ; fenêtre fixe conservée par défaut ;
- 5 tests déterministes + mesure sur trace synthétique :
  fixe 0 = 12 lots / 200 ms, fixe 50 = 4 lots / 360 ms, fixe 100 = 4 lots / 510 ms,
  adaptative = 4 lots / 450 ms.

**Gate IV : `PARTIAL`** — un point de comparaison mesuré existe, les résultats
défavorables sont conservés. Manquent : campagne A/B/C/D réelle, RAM/CPU, énergie,
et comparaison sur des projets représentatifs (Lot D non exécuté).

## 5. Lot D — projets représentatifs

**`NOT_RUN`.** Aucun corpus externe n'a été exécuté pendant ce cycle. Les fixtures
internes (multi-crates, features, build script) et l'application témoin M4 restent
les seules bases.

## 6. Qualité

| Contrôle | État |
|---|---|
| Tests supprimés ou affaiblis | **aucun** ; 2 compteurs mis à jour avec un cas de repli ajouté en contrepartie |
| Tests contournés (`ignore`, désactivation CI) | aucun |
| `PROVED` sans preuve | aucun |
| Dépendance lourde ajoutée | `sha2` seulement, déjà présente dans le workspace (même version), pour l'identité de snapshot |
| Affaiblissement de contrat | aucun ; les replis conservateurs sont conservés, pas retirés |

## 7. Risques et questions ouvertes

1. La résolution `cargo metadata` complète peut échouer hors ligne : le repli
   `--no-deps` est explicite, mais il **réduit** la couverture incrémentale.
2. La normalisation de chemins suppose des arbres sous la limite Windows classique
   (pas de chemin > 260 caractères) : un chemin long perdrait le support verbatim.
3. Le coordinateur d'overlays est en mémoire : la durabilité dépend du journal de
   publication existant, non branché ici.
4. Le gain de la fenêtre adaptative dépend d'une distribution d'arrivées non
   mesurée sur un usage réel.
5. Les 42 ADR restent `PROPOSED` ; aucune licence n'est choisie ; `main` n'est pas
   touchée ; aucune release.

## 8. Prochaine étape proposée

1. Brancher `snapshot_after()` sur `GitWriter` et prouver la publication atomique
   depuis un overlay (test de crash inclus).
2. Exécuter une campagne A/B/C/D pré-enregistrée sur une fixture plus grosse, avec
   RAM/CPU et plusieurs répétitions, et conserver les résultats défavorables.
3. Étendre le graphe aux build scripts (`OUT_DIR` observé) et aux arêtes de
   dépendances optionnelles résolues.
4. Constituer le corpus du Lot D (petit projet Rust, workspace multi-crates,
   full-stack, module DSP, projet non Rust).

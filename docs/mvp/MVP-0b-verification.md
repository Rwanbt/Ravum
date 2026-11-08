# MVP-0b / L4-D — Vérification bornée, statuts de preuve et intégration d'un prouveur externe

> **Portée** : état réel de la vérification après MVP-0a, conservation des garanties M2,
> recherche réelle d'un prouveur externe, défaut trouvé pendant l'audit et corrigé.
> **Aucune preuve générale de sûreté n'existe, et ce document n'en revendique aucune.**

## 1. Ce qui est conservé de M2 (et vérifié à nouveau)

Le crate `ravum-proof` implémente des obligations sur un **domaine fini explicitement
énuméré** ; il n'y a ni solveur, ni preuve universelle. Vérifié le 2026-11-08 :

| Vérification | Commande | Résultat |
|---|---|---|
| Tests du crate | `cargo test -p ravum-proof` | **28 réussis** (19 unitaires + 9 `proof_cache`), 0 échec |
| Obligation satisfaite, domaine énuméré | `ravum prove fixtures/proof/positive.rs --claim finite --inputs 0,1,2,3 --json` | `status: "proved"`, `authorizes_activation: true`, hypothèses : « declared domain: 4 explicit input(s) », TCB : rustc + loader + harness |
| Contre-exemple | `ravum prove fixtures/proof/negative.rs --claim non-negative --inputs 0,1,2,3 --json` | `status: "tested"`, `counterexample: "f(2) = -1 < 0"`, **exit 1 attendu**, activation refusée |

L'enregistrement de preuve porte : clé de vérification dérivée du contexte complet
(snapshot, contexte de build, artefact, outil, version), propriété, hypothèses, domaine
déclaré, base de confiance, toolchain, horodatage, contre-exemple. Le ledger est
append-only et un enregistrement de contexte périmé est `Stale`, jamais réutilisé comme
courant (9 tests `proof_cache`).

### Règles de statut (conservées telles quelles)

- `TESTED`, `ASSUMED`, `UNKNOWN`, `PENDING`, `STALE`, `TIMEOUT`, `UNSUPPORTED` **ne
  deviennent jamais** `PROVED`.
- Un `PROVED` n'autorise l'activation que pour le domaine explicitement énuméré.
- Un prouveur absent produit `Unsupported`, jamais un succès.
- Le cache ne peut pas élever un statut : une entrée de cache est réutilisée seulement si
  sa clé de contexte complète correspond ; un `Timeout` reste `Timeout`.

Tests dédiés : `unsupported_backend_never_authorizes_activation`,
`timeout_never_authorizes_activation`, `counterexample_is_cached_and_never_upgraded`,
`changing_the_artifact_forces_a_new_run`, et la suite `proof_cache`.

## 2. Défaut trouvé pendant l'audit MVP-0b et corrigé

En re-exécutant la suite plusieurs fois, `timeout_never_authorizes_activation` échouait
**par intermittence** en exécution parallèle (18/19 ou 19/19 selon les runs).

Cause racine (identifiée dans le code, pas contournée) : le helper de test
`broker_with` construisait le chemin du ledger temporaire à partir du compteur
d'exécutions du vérificateur, qui vaut **0 au départ de chaque test** :

```text
%TEMP%/ravum-broker-<pid>-0.jsonl   ← partagé par tous les tests du binaire
```

Comme tous les tests construisent aussi le **même** `ProofContext` et la même obligation,
la clé de vérification était identique : un enregistrement `Proved` écrit par un test
était un **hit de cache frais** pour le test `Timeout`, qui recevait alors `Proved` et
échouait sur son assertion.

Correction : séquence process-wide unique pour le nom du fichier de ledger
(`LEDGER_SEQ.fetch_add(1)`), donc un ledger par test, sans masquer l'assertion. Vérifié
par 6 exécutions consécutives : **19/19 à chaque fois**. Le commentaire de code explique
le défaut et la raison du correctif.

**Conséquence honnête pour la baseline** : le chiffre « 139 tests réussis » de l'audit
L0 était exact pour cette exécution, mais la suite contenait un test instable latent.
Il est corrigé ici, pas ignoré.

## 3. Prouveur externe : état réel

| Élément | Statut observé |
|---|---|
| `kani` dans `PATH` | **absent** (`kani --version` → commande inconnue) |
| `cargo kani` | **absent** (sous-commande inconnue) |
| Harness Kani dans le dépôt | **aucun** |
| Résultat de l'adaptateur `KaniVerifier` | `Unsupported { detail }` après détection ; jamais un succès |
| Preuve externe exécutée | **aucune** |

Conclusion : **aucune intégration de prouveur externe n'est opérationnelle**. Le code est
prêt à détecter un prouveur et à refuser honnêtement de conclure sans harness — c'est
exactement le comportement actuel. Aucun `PROVED` externe n'existe dans ce cycle.

### Plan d'intégration (proposé, non exécuté)

1. Installer un prouveur dans un environnement dédié (`cargo install --locked
   kani-verifier` puis `cargo kani setup`) — **non fait** : téléchargement global, à
   décider ; aucune autorisation implicite n'est prise ici.
2. Écrire un harness Kani par propriété ciblée (p. ex. absence de débordement sur
   `f64 -> f64`, ou invariant d'état DSP sur un domaine déclaré) et le versionner avec la
   propriété.
3. Faire produire par l'adaptateur un enregistrement `PROVED` **uniquement** si un
   certificat/harness a réellement été exécuté, avec version du prouveur et base de
   confiance dans le ledger.
4. Tests adversariaux : prouveur qui échoue, qui timeout, qui retourne un statut ambigu,
   version de prouveur différente → le cache doit être invalidé et l'activation refusée.

## 4. Limites (à ne pas lire comme des acquis)

- `PROVED` ne couvre que des domaines finis énumérés ; aucune propriété générale.
- Aucun prouveur externe installé ni exécuté : aucun certificat indépendant.
- Le harness « exhaustif » teste des points, pas des intervalles continus.
- Le cache de preuves est invalidé par mutation de contexte (tests), mais son modèle de
  menace n'est pas formellement analysé.
- La sûreté mémoire du code natif chargé (`unsafe`, FFI) n'est pas prouvée ; seules des
  frontières `unsafe` documentées et des tests dynamiques existent.
- `cargo test` vert ne prouve ni la correction générale du compilateur, ni l'absence de
  corruption mémoire, ni une garantie hard-RT.

## 5. Reproduire

```powershell
cd D:\App\Ravum
cargo test -p ravum-proof
cargo run -q --bin ravum -- prove fixtures/proof/positive.rs --claim finite --inputs 0,1,2,3 --json
cargo run -q --bin ravum -- prove fixtures/proof/negative.rs --claim non-negative --inputs 0,1,2,3 --json   # exit 1 attendu
kani --version    # doit échouer : prouveur non installé (état observé)
```

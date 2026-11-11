# MVP-0d / Lot A — incrémentalité avancée : features, contexte de compilation, chemins

> **Observation bornée, pas un certificat.** Les mesures portent sur la machine de
> référence locale (Windows, rustc 1.95.0, ≈16 Go), sur des fixtures du dépôt.
> Aucun ADR n'est accepté, aucune supériorité générale n'est revendiquée.

## 1. Problème réel corrigé

Le MVP-0c vérifiait déjà `content_id`, `context_id`, `externs_digest` et
`verdict_key`, mais un paquet **déclarant des features** tombait systématiquement
en repli Cargo (`fallback_reason = features_declared`) : le graphe ne connaissait
pas l'ensemble activé, donc ne pouvait pas garantir que l'unité compilée était
celle que Cargo compile.

Deux défauts d'identité ont été trouvés en écrivant ce lot :

1. `context_id` contenait `"features": []` **en dur** : une activation de feature
   ne changeait pas l'identité de contexte.
2. `cargo metadata` renvoie parfois les chemins avec le préfixe verbatim Windows
   `\\?\` et parfois sans. rustc enregistre le chemin source dans le `.rmeta` :
   deux découvertes du même arbre produisaient donc des `interface_id` différents.
   Le `content_id` devenait de surcroît **absolu** quand `strip_prefix(root)`
   échouait, ce qui faisait croire que toutes les unités avaient changé.

## 2. Ce qui est implémenté

| Élément | Comportement |
|---|---|
| Découverte | `--no-deps` (bon marché) d'abord ; `cargo metadata` **résolu** seulement si un membre déclare une feature ou si une dépendance normale est optionnelle/demande des features |
| Activation inconnue | `Unit::features_resolved = false` → repli Cargo conservateur, **jamais** un ensemble vide supposé |
| `Unit` | `package_id`, `activated_features`, `features_resolved`, `crate_types` |
| `context_id` v2 | toolchain, cible, édition, nom de crate, **features activées**, **crate types**, build script, kind, flags |
| Compilation ciblée | `--cfg feature="…"` passé à rustc, exactement comme Cargo |
| Chemins | normalisation (canonicalisation + suppression du préfixe verbatim) de la racine, des manifestes, des sources et des dépendances |
| Dépendances optionnelles | **repli conservateur maintenu** : tant que l'ensemble d'arêtes résolu n'est pas utilisé pour filtrer les arêtes inactives, une dépendance optionnelle force Cargo |

### Coût de la résolution (mesuré sur ce dépôt)

| Appel | Durée |
|---|---:|
| `cargo metadata --no-deps --offline` | 372 ms |
| `cargo metadata --offline` (résolu) | 1 096 ms |

D'où la découverte en deux temps : un workspace sans features garde le coût du
MVP-0c (372 ms) ; la résolution (+724 ms sur ce dépôt) n'est payée que lorsqu'elle
apporte une information réelle. Le prix de la fonctionnalité est donc **explicite et
conditionnel**, pas imposé à tous les projets.

Le point important est le dernier : il aurait été possible de « gagner » en
supprimant ce repli, au prix d'une compilation d'arêtes que Cargo n'active pas.
Ce n'est pas fait. Le lot reste donc conservateur.

## 3. Tests ajoutés (exécutés)

Dans `crates/ravum-unit-graph/tests/negative_cases.rs` :

| Test | Ce qu'il prouve |
|---|---|
| `case_06_feature_package_is_targeted_when_the_activation_set_is_resolved` | le paquet à features est compilé par la chaîne ciblée ; activer `extra` change `context_id` **et** `interface_id` — donc le `--cfg` atteint réellement rustc, pas seulement la clé de cache |
| `case_06b_feature_package_falls_back_when_the_activation_set_is_unknown` | métadonnées `--no-deps` (pas de section `resolve`) ⇒ `features_resolved = false` et repli `FeaturesDeclared` |
| `case_15_toolchain_change_invalidates_the_cache` (mis à jour) | 4 unités ciblées sont HIT à l'identique, et un changement d'identité de toolchain n'en réutilise aucune |
| `hit_is_only_granted_when_everything_matches` (mis à jour) | idem, compteur porté à 4 |

La mise à jour de deux compteurs (3 → 4) n'affaiblit aucun contrat : elle constate
qu'une unité de plus est désormais vérifiée de façon ciblée au lieu de tomber en
repli. Le cas de repli correspondant est ajouté explicitement.

## 4. Déterminisme retrouvé

La normalisation des chemins a été validée par le scénario de déterminisme de
`central_evidence` : deux campagnes successives sur le même arbre produisent des
`.rmeta` identiques (`sha256` identiques pour `core`, `api`, `consumer`).

## 5. Limites

- Les **build scripts** restent en repli (`BuildScript`) : leur effet sur `OUT_DIR`
  n'est pas modélisé.
- Les **dépendances optionnelles** restent en repli (voir §2).
- Les **cibles multiples** d'un paquet (exemple, test, bench) ne sont pas des
  unités séparées : seule la cible `lib` (ou `bin` à défaut) est modélisée.
- Aucun gain de performance n'est revendiqué pour ce lot : le gain est
  **qualitatif** (une classe d'unités n'est plus aveuglément reléguée à Cargo) et
  **de sûreté** (identités stables, activation explicite).

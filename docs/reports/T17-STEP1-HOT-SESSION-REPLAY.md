# T17 étape 1 — replay déterministe d'une session de hot swap (2026-11-19)

> Gate **T17** (ADR-016/007, V1). Passe de `NOT_STARTED` à **`PARTIAL`**.
> Aucun `PROVED` : propriété **testée** sur une plateforme.

## Objectif

Une session réelle d'édition à chaud n'est pas reproductible en la relançant :
l'instant où le thread temps réel adopte une version dépend de
l'ordonnancement des threads. Une IA qui voit un défaut en direct doit pouvoir
le **rejouer à l'identique**. Ce qui est reproductible, c'est le **journal** de
la session :
- le signal d'entrée ;
- le module de départ ;
- pour chaque version réellement adoptée, son IR et l'index du premier
  échantillon qu'elle a traité.

## Modification

`crates/ravum-lang-jit/src/replay.rs`, sans `unsafe` :
- `SessionLog`, format `ravum-hot-session-1`, sérialisé en JSON ; une version
  de format inconnue est refusée ;
- `replay(log)` pilote le même couple `HotEngine`/`RtRunner` : même retour
  arrière sur trap (HR-1.5), même état possédé par l'hôte, mêmes refus ;
- `digest(sortie)` : SHA-256 des bits de la sortie.

Un seul crate est modifié (`ravum-lang-jit`), avec trois dépendances déjà
présentes dans le workspace (`serde`, `serde_json`, `sha2`).

## Tests (`crates/ravum-lang-jit/tests/replay.rs`)

| Test | Résultat |
|---|---|
| Session réelle : thread temps réel (blocs de 64 échantillons, période 1,333 ms), biquad à état, 20 éditions dont une qui déclenche un trap (retour arrière) ; journal sérialisé puis relu | 25 600 échantillons, 20 versions adoptées ; **replay = sortie réelle, bit à bit** |
| 100 replays supplémentaires | **101/101 digests identiques** (forme de preuve d'ADR-016), en debug et en release |
| **négatif** (puissance) : décaler une version d'un bloc | le digest change ; l'égalité n'est donc pas vide |
| **négatif** : versions dans le désordre, ou au-delà du signal | `BadLog` |
| **négatif** : version qui change le schéma d'état | `Refused { swap: 1, … }` : même refus qu'en direct |
| **négatif** : format de journal inconnu | refusé |

## Limites

- **Entrées couvertes :** sans état `fn(f32) -> f32`, et bloc DSP
  `process_sample` ; pas l'entrée sans paramètre de `watch`.
- **Versions absentes du journal :** une version publiée puis remplacée avant
  d'être prise n'a jamais tourné.
- **Ce sur quoi repose le déterminisme :**
  - la sémantique de l'IR : aucune E/S n'est permise sur une entrée temps réel
    (INV-05), et l'IR n'a ni horloge ni aléa ;
  - un code Cranelift IEEE-754 sans fast-math.

  Ces deux points sont vérifiés par tests, pas prouvés. Une seule plateforme
  (Linux x86_64) a été testée : le replay inter-plateformes n'est pas mesuré.
- **Reste de T17 :** le profil déterministe du langage (horloge, aléa,
  ordonnancement virtualisés, replay par seed) n'est pas fait. Il touche la
  stdlib et le checker, partagés avec l'autre ligne de travail.

## Décision requise (non tranchée) : `migrate` (HR-1.7) sur la voie JIT

AGENTS.md interdit « migration » au callback audio. Deux options, à trancher
par le responsable :

1. **Migrer côté éditeur sur un instantané de l'état**, puis publier état et
   code ensemble. Le thread temps réel ne fait qu'une copie de la taille de
   l'état. Mais les échantillons traités entre l'instantané et l'adoption sont
   **perdus de l'histoire de l'état** : le filtre revient de quelques blocs en
   arrière. La transition n'est pas exacte, et il faudrait le documenter comme
   un écart de HR-1.2.
2. **Exécuter `migrate` sur le thread temps réel**, une fois, au point sûr.
   C'est exact, mais cela contredit AGENTS.md, et le coût de `migrate` n'est
   pas borné.

Sans décision, la voie JIT continue de **refuser** tout changement de schéma
(`StateSchemaChanged`, HR-1.3). C'est le comportement fail-closed.

## Commandes

```
cargo test -p ravum-lang-jit --test replay -- --nocapture
cargo test -p ravum-lang-jit --release --test replay -- --nocapture
```

## Contexte

Linux x86_64 (conteneur cloud, 4 cœurs), `rustc 1.95.0`, Cranelift 0.116.1,
branche `claude/trusting-cerf-4hhv5a`. État : `TESTED`.

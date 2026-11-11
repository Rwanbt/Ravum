# MVP-0d / Lot C — fenêtre de groupement adaptative

> Mesure du **collecteur seul**, sur une trace synthétique déclarée. Aucun rustc,
> aucun `sleep` : les horodatages sont injectés, donc la mesure est reproductible
> et ne dépend pas de la charge machine.

## 1. Ce qui existait

`ravum-batch` implémentait déjà une fenêtre fixe (0, 50 par défaut, 100 maximum),
avec un collecteur de lots, un pré-vol de conflits, un repli N+1 et une télémétrie
de latence. Le MVP-0c avait mesuré un gain de −74 à −83 % sur six éditions, et
−69,1 % de bout en bout avec un vrai `cargo test`.

## 2. Ce qui est ajouté

`AdaptiveWindow { min_ms, max_ms, step_ms }`, bornée par `MAX_WINDOW_MS = 100` :

- la fenêtre **grandit** quand une proposition rejoint un lot déjà ouvert (densité
  observée : regrouper davantage ne retarde personne) ;
- la fenêtre **rétrécit** quand une fenêtre expire sans qu'aucune proposition ne
  l'ait rejointe (rareté : un développeur seul ne doit pas attendre 100 ms) ;
- bornes strictes `min_ms ≤ fenêtre ≤ max_ms ≤ 100` ;
- télémétrie : `window_growths`, `window_shrinks`, `max_effective_window_ms`, et
  `effective_window_ms()` sur le collecteur.

La fenêtre fixe historique reste le comportement par défaut (`BatchConfig::new`) :
aucun appelant existant ne change de comportement.

## 3. Mesure (trace synthétique)

12 propositions, quatre rafales de trois propositions espacées de 5 ms, silences
de 200 ms entre rafales.

| Configuration | Lots formés | Analyses | Attente cumulée |
|---|---:|---:|---:|
| Fixe 0 ms | 12 | 12 | 200 ms |
| Fixe 50 ms | 4 | 4 | 360 ms |
| Fixe 100 ms | 4 | 4 | 510 ms |
| **Adaptative (10–100, pas 20)** | **4** | **4** | **450 ms** |

Lecture honnête :

- l'adaptative **égale** la fenêtre fixe 100 ms sur le nombre de lots (4) ;
- elle **réduit l'attente** de 12 % face à la fenêtre 100 ms sur cette trace
  (450 ms contre 510 ms), parce qu'elle rétrécit dans les silences ;
- elle ne bat pas la fenêtre 50 ms sur l'attente (450 contre 360) : le pas de 20 ms
  et le plafond à 100 la laissent plus longtemps ouverte. **Ce résultat
  défavorable est conservé.**

Une trace synthétique ne prouve rien sur un usage réel : elle montre seulement que
la politique fait ce qu'elle annonce, et fournit un point de comparaison.

## 4. Limites

- Aucune mesure de bout en bout avec un vrai `cargo test` pour l'adaptative.
- Aucune mesure CPU/RAM de l'adaptation (coût négligeable en théorie, non mesuré).
- Le paramétrage (min, max, pas) n'est pas auto-calibré : il est explicite.
- Le gain dépend de la distribution réelle des arrivées, inconnue à ce jour.

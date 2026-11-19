# RFC-E11-HOT-RELOAD — Hot reload `.rv` avec préservation d'état

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-HOT-RELOAD` |
| Statut | **`PROPOSED`** — aucune implémentation avant décision du responsable sur §7 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) ; ferme T8 avec RFC-E11-DSP-BLOCK |
| S'appuie sur | [`RFC-P2-CONTINUOUS-COMPILATION`](RFC-P2-CONTINUOUS-COMPILATION.md), [`RFC-RV-DSP-BLOCK`](RFC-RV-DSP-BLOCK.md), `crates/ravum-patch-host` |
| Portée | `crates/ravum-lang-cli` (`watch`, `--emit reloadable`), `crates/ravum-lang-codegen` (point d'entrée de module), `crates/ravum-patch-host` (hôte générique), `spec/language/` (nouveau `reload.md`) |

> **Ce que cette RFC ne promet pas.** Ni migration d'état arbitraire, ni
> remplacement de fonctions au milieu d'un appel, ni préservation de threads ou
> de connexions. L'état préservé est **une valeur déclarée**, de type fixé.

## 1. État mesuré

Gate T8 : `watch` reconstruit et **redémarre** ; aucun état n'est préservé.
`ravum-patch-host` sait charger un `cdylib`, valider son ABI, garder l'ancien
module vivant pendant un fondu et refuser un module incompatible
(`validate_loaded`, `RejectReason`) — pour l'ABI DSP seulement.

## 2. Modèle proposé : programme rechargeable = état + étape

Un programme rechargeable déclare, dans son module racine :

```rust
struct State { count: i64, items: Vec<String> }

fn init() -> State effects() { ... }
fn step(state: State, event: Event) -> State effects(io) { ... }
```

`Event` est un enum fourni par l'hôte (`Tick`, `Line(String)`, `Stop`).
`ravum-lang watch --reload` compile en `cdylib`, et l'hôte possède `State`
entre deux appels de `step`. À chaque édition acceptée :

1. le nouveau module est compilé hors du chemin chaud ;
2. son **digest de schéma d'état** est comparé à celui du module courant ;
3. **égal** → le prochain `step` appelle le nouveau code avec l'état actuel ;
4. **différent** → refus (`RejectReason::StateSchemaChanged`), l'ancien module
   continue, le diagnostic est affiché ; `--reload=restart` permet de choisir
   un redémarrage explicite avec `init()`.

| ID | Règle proposée | Niveau |
|---|---|---|
| HR-1.1 | Le schéma d'état est le type `State` développé (noms et types des champs, variantes, dans l'ordre) ; son digest est SHA-256 du texte canonique. | MUST |
| HR-1.2 | Un remplacement n'a lieu **qu'entre deux** `step` ; jamais pendant. | MUST |
| HR-1.3 | Un module dont le digest diffère est refusé, l'ancien continue ; aucune conversion implicite. | MUST |
| HR-1.4 | Un module qui ne compile pas, ou dont l'ABI diffère, est refusé ; l'ancien continue (fail-closed, P0-C). | MUST |
| HR-1.5 | Une panique dans `step` du nouveau module restaure l'état d'avant l'appel et revient à l'ancien module. | MUST |
| HR-1.6 | `State` doit être transférable (sans référence, CONC-2.1) et sans ressource (`Stream`, `Db`, `Handle` refusés : `E-HR-STATE-RESOURCE`). | MUST |
| HR-1.7 | Migration explicite (optionnelle) : `fn migrate(old: OldState) -> State` où `OldState` reprend le schéma précédent ; sans elle, HR-1.3 s'applique. | SHOULD |

## 3. Transport de l'état entre deux modules

Deux modules compilés séparément n'ont pas la même disposition mémoire garantie
pour `State` (pas de `repr(C)` en Ravum). Proposé : **sérialisation canonique**
de `State` en texte (format interne, champs dans l'ordre du schéma) au moment
du remplacement, et seulement à ce moment. Le coût est payé à l'édition, pas à
chaque `step`. Le format est le même que pour le digest, ce qui rend HR-1.1 et
le transport cohérents par construction.

## 4. Qualification (« 0 perte » de l'issue #96)

- Test : un compteur reçoit 10 000 événements pendant que 20 éditions
  compatibles sont appliquées ; la valeur finale doit être exactement 10 000.
- Test : une édition qui change `State` est refusée, la valeur continue.
- Test : une édition qui panique dans `step` revient à l'ancien module, sans
  perte d'événement.
- Test : une édition qui ne compile pas ne change rien.
- Trois OS (chargement de `cdylib` : `.so`, `.dylib`, `.dll`) ; les fichiers
  des anciens modules sont libérés (`reclaimer.rs`) et comptés.

## 5. Alternatives écartées

| Alternative | Pourquoi écartée |
|---|---|
| Remplacer des fonctions isolées (patch de symboles) | exige une ABI de fonction stable et des pointeurs de fonction ; Ravum n'a ni l'un ni l'autre |
| Partager `State` par pointeur sans sérialisation | disposition mémoire non garantie entre compilations ; comportement indéfini possible |
| Redémarrage + rejeu des événements | coût non borné, et les E/S rejouées ne sont pas idempotentes |

## 6. Lien avec le bloc DSP

Le bloc DSP (RFC-E11-DSP-BLOCK) est un **cas particulier** : état numérique fixé,
appel sur un thread temps réel, pas de sérialisation sur le callback. Les deux
RFC partagent le digest de schéma (HR-1.1) et la politique de refus.

## 7. Décisions attendues du responsable

1. Accepter le modèle `init`/`step` plutôt qu'un hot patch général.
2. Accepter la sérialisation au remplacement (§3).
3. `migrate` (HR-1.7) dans la 1.0 ou non.
4. Les quatre tests de §4 comme définition de « 0 perte » pour T8.

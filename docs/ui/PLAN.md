# Ravum UI — plan de réalisation

Statut : **PROPOSED** (plan de travail). Il n'accepte aucune RFC ni aucun
ADR et n'adopte aucune dépendance. Décisions de cadrage :
[`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md)
(UI-D1 à UI-D5).

## Objectif

Un système d'interface **natif**, déclaratif, multiplateforme (Linux,
Windows, macOS, puis Android et iOS), modifiable **à chaud** sans
redémarrer, avec un vérificateur **algorithmique** (sans IA). Il est validé
par de petites applications réelles avant toute adoption plus large.

« Abouti » veut dire : chaque étape est **complète** dans son périmètre, avec
ses tests positifs et négatifs et sa spécification ; aucun raccourci n'est
laissé en dette. Ça ne veut pas dire « tout d'un coup » : les étapes restent
ordonnées, et chacune est livrée finie.

## Principes retenus du plan GOAL externe

- Le cœur du langage ne dépend **pas** de l'UI. L'UI est un paquet et un
  runtime à part (`ravum-ui-*`) ; un programme sans UI ne change pas.
- UI **déclarative** : le programme décrit l'arbre ; le moteur natif
  calcule la mise en page et dessine. Le programme n'a aucun accès direct
  aux fenêtres ni au GPU.
- **Fail-closed** : un patch qui échoue (compilation, contrat, test,
  migration) laisse la version saine active ; un patch périmé est rejeté.
- **Vérification sans IA**, déterministe à entrées fixées ; les mesures de
  performance sont statistiques et séparées de la vérification.
- Statuts honnêtes : `PASS`, `FAIL`, `UNVERIFIED`, `UNSUPPORTED`, `STALE` ;
  jamais de `SKIP` compté comme `PASS`.
- Les plugins non fiables ne s'exécutent pas en natif dans le processus
  principal (WASM ou processus séparé). Ce point est traité à l'étape U6.

Ne sont **pas** repris : la voie Web (UI-D3), Unifia (UI-D1), un score
esthétique global, et le format de jetons DTCG tant qu'aucun besoin réel ne
le demande (à réexaminer à l'étape U5).

## Modèle d'application (UI-D2)

```rv
enum Msg { Increment, Decrement, Rename(String) }

struct Model { count: i64, name: String }

fn update(model: &mut Model, msg: Msg) { ... }

fn view(model: &Model) -> ui::Node<Msg> { ... }
```

- `view` est **pure** (`effects()`) : elle rend un arbre de nœuds qui porte
  des **messages** (valeurs) au lieu de fonctions.
- `update` est la **seule** à modifier l'état ; les effets (fichiers,
  réseau) passent par des commandes rendues par `update`, exécutées par le
  runtime (étape U3).
- Hot reload : `view` et `update` sont des fonctions ordinaires, que le
  moteur voie C remplace déjà fonction par fonction. L'état (`Model`) migre
  avec le mécanisme existant de migration de disposition.

## Étapes

| Étape | Contenu | Sortie vérifiable | Bloqué par |
|---|---|---|---|
| **U0** | Prérequis du langage : [`RFC-UI-LANG-1`](../rfc/RFC-UI-LANG-1.md) | RFC acceptée, puis implémentée dans les deux voies (`rustc` et IR/JIT), avec des cas de conformance | votre décision sur la RFC |
| **U1** | Banc d'essai des bibliothèques de rendu (UI-D4) | rapport mesuré : licences, plateformes, mémoire, texte, accessibilité, maintenance | — (peut se faire en parallèle de U0) |
| **U2** | Spécification `spec/ui/` : arbre de nœuds, mise en page, événements, styles, cycle de vie, hot reload L0, L1 et L2 | spec avec règles numérotées et exemples positifs et négatifs | U0, U1 |
| **U3** | Runtime natif : fenêtre, boucle d'événements, mise en page, texte, rendu, accessibilité, commandes asynchrones | petites applications réelles (compteur, liste de tâches, formulaire, lecteur de journaux) sous Linux, puis Windows et macOS | U2 |
| **U4** | Hot reload L0 (style), L1 (structure), L2 (comportement), avec conservation de l'état, du focus et du défilement, et retour en arrière | tests : patch cassé refusé, patch périmé rejeté, 10 000 événements et 20 éditions sans perte d'état | U3 |
| **U5** | Verify : géométrie (débordement, chevauchement, cibles tactiles), contraste, navigation clavier, profils responsive, contrats d'animation | rapports JSON déterministes, un test positif et un négatif par règle | U3 (sur les vraies applications) |
| **U6** | Composants et plugins : registre de services façon Cordis, dépendances, retrait propre, isolation des plugins non fiables | un plugin ajoute un panneau à une application ouverte, et son retrait la laisse intacte | U4 |
| **U7** | Qualification : multi-OS, fuzzing des patchs, mesures de performance, documentation, dossier de décision | `READY_FOR_HUMAN_GO_NO_GO`, jamais plus sans vous | U4, U5, U6 |

Android et iOS suivent U7 : ils dépendent du choix de U1 et de chaînes de
compilation que ce conteneur ne fournit pas.

## Décisions qui vous reviendront

1. Accepter, amender ou refuser `RFC-UI-LANG-1` (U0).
2. Choisir la pile de rendu à l'issue du banc d'essai (U1).
3. Ratifier la spécification `spec/ui/` (U2).
4. Le modèle d'isolation des plugins (U6).

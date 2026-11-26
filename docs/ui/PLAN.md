# Ravum UI — plan directeur

Statut : **PROPOSED** (révision 2, 2026-11-24). Ce plan n'accepte aucune RFC
ni aucun ADR et n'adopte aucune dépendance. Décisions de cadrage :
[`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md)
(UI-D1 à UI-D14).

> **Règle directrice** : on réduit le périmètre de chaque livraison, jamais
> l'ambition finale. Une fonctionnalité **reportée** reste dans ce plan avec
> son jalon ; rien n'est retiré en silence. Chaque étape livre un résultat
> **exécutable**, testé et documenté, qui sert de fondation au produit final
> et n'est pas jetable.

## 1. Objectif final

Ravum UI est un framework graphique **natif**, professionnel et universel.
Il doit être :
- rapide, avec egui comme référence de mesure ;
- sobre en CPU, RAM et GPU ;
- riche en styles, au niveau des propriétés CSS utiles ;
- extensible à chaud, par des plugins façon Cordis ;
- vérifiable de manière **déterministe**, sans IA.

Ses cibles :
- les applications classiques ;
- les logiciels exigeants : DAW, plugins audio, graphisme, vidéo, streaming, 3D, outils de développement ;
- desktop, tablette et mobile.

Il n'est déclaré « universel et abouti » qu'après une qualification réelle
**desktop et mobile**.

## 2. Architecture cible : responsabilités, pas forcément des crates

| Responsabilité | Rôle |
|---|---|
| UI Core | composants, messages, état, réactivité incrémentale |
| UI IR | représentation versionnée de l'interface : identifiants stables, arbre sémantique, propriétés calculées |
| Design | contrat de charte (jetons, variantes, densité, thèmes) |
| Layout | Flex, Grid, Block, positionnement, responsive |
| Graphics | rendu natif interchangeable, texte, composition GPU, surfaces spécialisées |
| Motion | animations sous contrat |
| Verify | règles déterministes, diagnostics, quality gates, sous-scores |
| Live | rechargement transactionnel L0, L1, L2 |
| Components | services, dépendances, plugins, isolation |

Règles d'architecture :
- Le cœur du langage ne dépend **jamais** de l'UI. Un programme sans UI ne change pas.
- Aucune dépendance graphique n'est normative : le renderer est
  interchangeable derrière les contrats Ravum.
- Un crate n'est créé que s'il a un consommateur réel (AGENTS.md).
- Le Design Contract, l'UI IR et Verify restent **indépendants du
  renderer**, ce qui préserve une future voie Web (W0 à W2) sans DOM ni
  navigateur pendant le chantier natif.

## 3. Modèle d'application (UI-D2)

```rv
enum Msg { Increment, Rename(String) }
struct Model { count: i64, name: String }
fn update(model: &mut Model, msg: Msg) -> ui::Command<Msg> { ... }
fn view(model: &Model) -> ui::Node<Msg> { ... }
```

- `view` est **déterministe**, sans être `pure` au sens du profil d'effets
  (UI-D18) : elle peut allouer, avec l'effet `alloc`, pour construire un
  arbre dynamique, mais n'a aucun autre effet (pas d'`io`, de réseau,
  d'horloge ni d'aléa). Ce contrat de vue déterministe est à spécifier
  (`spec/ui/`, U2). Le profil `pure` actuel n'est **pas** affaibli.
- L'arbre porte des **messages** (des valeurs), pas des fonctions.
- `update` est la seule à modifier l'état. Les effets (fichiers, réseau,
  minuteries) sont des **commandes** exécutées par le runtime, ce qui les
  rend rejouables et vérifiables.
- Le runtime garde un arbre **persistant** (retained) avec des identifiants
  stables. Il compare la nouvelle vue à l'ancienne et n'invalide que ce qui a
  changé (style, texte, layout, peinture, composition, sémantique). Pas de
  reconstruction complète à chaque frame.
- Le modèle à messages est le **premier** modèle, pas une limite : les
  closures, les traits et les handles ont leurs jalons (RFC-UI-LANG-2 à 5).

### 3.1 Réactivité : deux modèles mesurés avant de choisir (UI-D13)

- **A** : `view` reconstruit l'arbre, puis un diff avec l'arbre persistant.
- **B** : réactivité granulaire. Les nœuds déclarent leurs dépendances
  envers l'état, et un changement ne réévalue que les nœuds dépendants.

Les deux sont prototypés et mesurés au banc (tour 2) sur les mêmes scènes :
- mise à jour d'un paramètre à haute fréquence ;
- liste de 10 000 éléments ;
- formulaire.

On mesure le coût du suivi des dépendances, la mémoire et le déterminisme.
Le choix revient au responsable. Dans les deux cas, l'arbre n'est **jamais**
reconstruit en entier à chaque événement sur un chemin chaud.

Dans les deux cas, le moteur prévoit :
- des frontières de layout ;
- des caches de shaping ;
- des invalidations séparées (structure, layout, peinture, composition, sémantique) ;
- la virtualisation, le rendu à la demande, des files d'événements bornées ;
- la gestion de la pression mémoire.

Il est mesuré avec Verify désactivé, puis incrémental, puis complet.

### 3.2 Portabilité du hot reload (UI-D14)

| Niveau | Développement | Distribution |
|---|---|---|
| L0 (style), L1 (structure) | données de l'UI IR, **sans JIT** | idem, sur toutes les plateformes |
| L2 (comportement) | JIT de la voie C là où l'OS le permet | compilation anticipée ; pas de code généré à l'exécution là où c'est interdit (iOS, magasins d'applications) |
| Plugins | processus isolé ou runtime sandboxé | selon les capacités de la plateforme |

**Compatibilité de l'état** (UI-D18) : garder l'ancien code vivant ne
suffit pas, si la structure d'un état partagé change.

| Modification | Activation |
|---|---|
| Style | transaction UI (L0) |
| Structure de composants | remplacement local contrôlé (L1) |
| Code, état compatible | remplacement par génération |
| Code, état incompatible | arrêt synchronisé des threads concernés, puis migration contrôlée (mécanisme de migration de disposition existant), puis reprise ; refus fail-closed si la migration n'est pas définie |

Les contrats UI et le modèle de composants sont communs à toutes les
plateformes ; seul le mécanisme d'activation de L2 varie. Les contraintes
Android et iOS sont évaluées explicitement au tour 4 du banc.

## 4. Verify grandit avec le moteur (UI-D7)

Dès le premier rendu (U2), le moteur expose ce que Verify consomme :
- identifiants stables, arbre sémantique, rôles et états ;
- rectangles calculés, transformations, clipping, ordre de superposition, visibilité ;
- métriques réelles du texte, zones interactives ;
- propriétés calculées, dépendances de layout, coût des invalidations.

Chaque capacité graphique arrive avec ses règles Verify. Le premier
vérificateur (U3) détecte déjà :
- un texte qui déborde ;
- un composant qui sort de son conteneur ;
- un chevauchement non autorisé.

Garanties de Verify :
- **Aucun modèle IA**, aucun jugement probabiliste, aucun service externe.
- Mêmes entrées, règles, polices, versions et profil : mêmes diagnostics et
  mêmes scores. L'arithmétique est canonique, les règles d'arrondi et de
  tolérance sont explicites, l'ordre de sortie est stable.
- Statuts : `PASS`, `FAIL`, `UNVERIFIED`, `UNSUPPORTED`, `STALE`. Une
  entrée non vérifiée n'améliore jamais un score.
- **Sous-scores** : géométrie, visibilité, débordements, espacements et
  alignements, lisibilité et contraste, conformité à la charte,
  accessibilité, responsive, motion, coût structurel. Un score global n'est
  qu'une agrégation configurable.
- **Une erreur critique bloque quel que soit le score** : une interface à
  99/100 dont une fonction essentielle est inaccessible donne `FAIL`.
- Anti-contournement : masquer un élément, retirer une fonction, réduire un
  texte sous la taille lisible, exclure des états ou modifier la charte
  pour passer sont détectés ou interdits. Les règles et pondérations sont
  versionnées, et un agent ne peut pas les modifier en silence.
- Exécution incrémentale, en cache, annulable, hors du thread de rendu.

## 5. Étapes

| Étape | Contenu | Critère de sortie exécutable |
|---|---|---|
| **U0** | Baseline : Ravum (compilation, hot swap, existant) ; protocole et scripts de mesure pour vos machines (UI-D9) | rapport lié au SHA ; scripts reproductibles ; aucun chiffre inventé |
| **U1** | Langage : [`RFC-UI-LANG-1`](../rfc/RFC-UI-LANG-1.md) (emprunts bornés, `Map`, texte, clone), puis RFC-UI-LANG-2 (handles générationnels) | conformance positive, négative et adversariale ; accord `rustc` et IR/JIT ; fuzzing d'ownership étendu |
| **U1-B** | Banc d'essai graphique (UI-D4, UI-D8), en parallèle | rapport comparatif mesuré ; décision du responsable avant toute adoption |
| **U2** | Première fenêtre native en Ravum : arbre retained, layout, texte, rendu, événements, accessibilité de base, données Verify exposées | une application Ravum réelle interactive sous Linux, puis Windows et macOS |
| **U3** | Hot reload UI L0 (style), L1 (structure), L2 (comportement) et Verify minimal (débordement, sortie de conteneur, chevauchement) | patch cassé refusé, patch périmé rejeté, état, focus et défilement conservés ; 10 000 événements et 20 éditions sans perte ; diagnostics déterministes |
| **U4** | Composants façon Cordis : services typés, dépendances, cycle de vie, panneaux et commandes, nettoyage automatique, permissions, retrait et rollback transactionnels, plugins non fiables isolés (WASM ou processus) | un plugin ajoute, modifie et retire un panneau dans une application ouverte |
| **U5** | Design Contract et styles riches : jetons, variantes, thèmes ; bordures et arrondis indépendants, ombres, dégradés (linéaire, radial, conique), transformations, masques, filtres sous budget ; import et export DTCG | un composant d'agent ou de plugin hérite de la charte ; écarts détectés par Verify |
| **U6** | Layout et responsive avancés : profils PC, tablette et mobile (orientation, DPI, taille de texte, zones système, entrées) ; points de rupture et intervalles (analyse exacte ou balayage borné, couverture publiée) | gates stricts sur les profils ; sous-score responsive sur le domaine déclaré |
| **U7** | Motion Contracts : rôles, durée, easing, amplitude, interruption, retargeting, réduction du mouvement, suspension hors écran, budgets ; horloge virtuelle pour les tests | trajectoires vérifiées (états intermédiaires, dépassement d'un spring) ; zones interactives cohérentes pendant les transitions |
| **U8** | Verify complet : toutes les règles, sous-scores, anti-contournement, rapports JSON et SARIF, intégration aux gates d'activation à chaud | un test positif et un négatif par règle ; sorties identiques sur N exécutions |
| **U9** | Surfaces spécialisées : waveform, spectrogramme, timeline, graphe de nœuds, canevas vectoriel, vidéo, viewport 3D ; textures GPU partagées ; audio et vidéo indépendants de l'UI | applications de qualification 4 à 6 (§6) |
| **U10** | Qualification professionnelle : desktop puis tablette et mobile (Android, iOS), performance, stabilité sur de longues sessions, sécurité, fuzzing, documentation, dossier de décision | `READY_FOR_HUMAN_GO_NO_GO`, jamais plus sans vous |
| **W0 à W2** | Différé (UI-D3) : adaptateur Web (W0), optimisations Web mesurées (W1), contributions aux navigateurs sur preuve seulement (W2) | conditionné à U10 |
| **Unifia** | Différé (UI-D1) : un panneau natif derrière un drapeau de fonctionnalité | conditionné à U10 |

Le jalon « desktop qualifié » peut être atteint avant le mobile, mais il ne
vaut pas « abouti ».

## 6. Applications de qualification

Ce sont de vrais programmes Ravum, et non des maquettes ou des interfaces
codées dans l'hôte Rust. Leurs fonctions métier sont limitées, mais elles
sont complètes sur les capacités techniques qu'elles qualifient.

| # | Application | Qualifie | Étape |
|---|---|---|---|
| 0 | Compteur, liste de tâches | boucle de base, hot reload | U2, U3 |
| 1 | Gestionnaire de paramètres | formulaires, focus, thèmes, responsive, accessibilité | U3, U5, U6 |
| 2 | Kanban | glisser-déposer, listes virtualisées, animations de réorganisation | U6, U7 |
| 3 | Éditeur de texte et de code | Unicode, graphèmes, sélection, IME, grands textes, latence | U5, U8 |
| 4 | Analyseur audio | visualisation temps réel, isolation du thread audio (INV-05) | U9 |
| 5 | Lecteur vidéo avec timeline | surfaces vidéo, zoom, défilement, textures | U9 |
| 6 | Inspecteur 3D | viewport GPU et composition UI | U9 |
| 7 | Application extensible | ajout, retrait et remplacement de plugins UI à chaud | U4 |

## 7. Performance (UI-D9)

- Mesures :
  - temps de frame CPU et GPU (p50, p95, p99) ;
  - latence de l'entrée à l'affichage ;
  - consommation au repos ;
  - démarrage ;
  - allocations ;
  - nœuds recalculés ;
  - taux de cache ;
  - coût du layout, du texte, de la peinture et de la composition ;
  - délai d'une édition à son aperçu et à son activation vérifiée ;
  - coût de Verify ;
  - longues sessions ;
  - temps total jusqu'à une interface validée, humain ou agent compris.
- Cibles de 60 et 120 FPS sur les profils compatibles. Les seuils sont
  fixés **à partir de la baseline et avant** les campagnes de qualification,
  puis ne sont jamais affaiblis.
- Matériel : celui du responsable (portable i7 13e génération et RTX 4070
  8 Go, fixe RTX 3060 12 Go, Xiaomi 14 Ultra), plus des GPU intégrés et des
  machines modestes. Ce conteneur n'a ni GPU ni écran : ses mesures se
  limitent au mode headless (layout, texte, Verify, rendu CPU).
- Aucune supériorité sur egui ou un autre framework n'est revendiquée sans
  banc comparable.

### 7.1 Première frame : chantier fermé, reprise différée

Fermé par le responsable le 2026-11-26 (UI-D21), à reprendre **une fois le
plan Ravum UI entièrement réalisé**. État et chiffres :
[`BENCH-U1B-ROUND3.md`](BENCH-U1B-ROUND3.md), section « Première frame ».

- Acquis (r1-pulse, Intel iGPU, à chaud) : 199 → **124 ms** ; préparation
  du GPU, des pipelines et des polices en parallèle de la fenêtre ; instance
  wgpu sur l'API native seule ; anticrénelage « zone » seul ; cache de
  pipelines wgpu sur disque. À reporter dans le runtime réel.
- Reste, par gain attendu :
  1. **Froid réel** (≈ 620 ms après installation ou mise à jour du pilote,
     dont ≈ 410 ms de compilation par le pilote) : première image légère
     (fond et cadre, sans les shaders de calcul de Vello) affichée pendant
     la compilation, ou compilation des pipelines Vello en arrière-plan
     après une première frame dessinée autrement.
  2. **Compilation différée au premier rendu** (≈ 48 ms) : le
     préchauffage hors écran est une perte tant que le thread de préparation
     est le chemin critique ; à revoir si l'instance et le device
     deviennent plus courts.
  3. **Instance et device wgpu** (≈ 64 ms) : ouvrir directement l'adaptateur
     retenu la fois précédente au lieu d'énumérer ; vérifier le coût du
     chargement des pilotes Vulkan de toutes les cartes.
  4. Shaders précompilés à la construction (supprime naga du démarrage ;
     demande une modification de Vello ou un fork).
- À mesurer d'abord : Windows (DX12), macOS (Metal), GPU dédié (non
  sélectionnable sous Linux sur r1-pulse), machines modestes.
- Écart de référence : Slint 89 ms, egui 176 ms (échelle 100 %).

## 8. Gouvernance

Pour chaque étape :
- les RFC et ADR nécessaires, sans excès de documentation ;
- des issues créées au fil des dépendances confirmées, et non un backlog spéculatif ;
- des critères mesurables, une implémentation réelle, des tests positifs et négatifs ;
- un rapport lié au SHA et une reprise possible par un autre agent : [`GOAL.md`](GOAL.md).

Zéro dette signifie :
- aucun contournement laissé comme solution finale ;
- aucun mock présenté comme une implémentation ;
- aucun raccourci de sécurité ;
- aucune source de vérité dupliquée ;
- aucun test désactivé ;
- aucun défaut critique connu laissé sans traitement ;
- aucune dépendance sans justification.

Les limites restent explicites.

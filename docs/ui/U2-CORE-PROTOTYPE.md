# U2/U3 — Cœur indépendant de la pile graphique (prototype)

Statut : **prototype de banc**, `TESTED`, rien d'adopté. Code dans
[`bench/ui-candidates/ui-core`](../../bench/ui-candidates/ui-core/src/lib.rs),
workspace séparé, **aucune dépendance**. Il prépare U2 et U3 sans attendre le
choix de la pile (U1-B, UI-D4) : le rendu, le vrai moteur de texte et la
fenêtre viendront avec la pile choisie.

## Contenu
- Arbre retained à clés stables ; réconciliation par clé : un nœud dont la
  clé survit garde son identité, son focus et son défilement.
- Patch **fail-closed** : clé dupliquée entre frères, taille négative, NaN ou
  infinie → refus, arbre précédent conservé ; patch construit sur une
  génération dépassée → `Stale`.
- Layout déterministe : rangée, colonne, `Fixed`, `Fill` (poids), `Auto`,
  marge interne, écart, décalage relatif ; texte mesuré en monospace fixe
  (8 × 16 px) par défaut ; la pile fournit la vraie mesure
  (`set_measure`, U2 : Parley, voir [`U2-SHELL.md`](U2-SHELL.md)).
- Événements : clic sur le nœud le plus profond **visible** (découpage par
  les ancêtres), focus au clic et au clavier dans l'ordre de l'arbre,
  défilement borné.
- Verify minimal (U3) : débordement de texte, sortie de conteneur (l'axe
  principal d'un conteneur défilant est exempté), chevauchement entre
  frères ; diagnostics triés.

## Hot reload U3 (`ui-core/src/hot.rs`)
- Niveaux classés : L0 (styles seuls), L1 (structure ou texte), L2
  (comportement : table de gestionnaires versionnée).
- Rechargement **transactionnel** : vue, liaisons et comportement sont tous
  contrôlés avant activation ; un refus laisse la version active entière.
  Refus : liaison vers un nœud absent, gestionnaire absent, comportement
  dont la version ne croît pas, vue cassée, patch périmé, et gate Verify
  (un rechargement qui **ajoute** des diagnostics est refusé).
- L'état de l'application n'est jamais touché par un rechargement.

## Plugins U4 (`ui-core/src/plugin.rs`)
- Services nommés, dépendances, ordre d'activation (fournisseur d'abord),
  cycle de vie journalisé (activation, désactivation).
- Permissions déclarées par ce que le plugin apporte (panneau, commande,
  service fourni ou utilisé) ; tout ce qui n'est pas accordé est refusé.
- Panneaux montés dans un emplacement de l'application, commandes préfixées
  par le plugin ; tout disparaît avec le plugin (nettoyage automatique).
- Installation, mise à jour et retrait **transactionnels** : l'ensemble
  candidat est validé en entier (service manquant ou en double, cycle,
  collision de clé, gate Verify, vue cassée) ; un refus ne change rien.
  Retrait refusé tant qu'un autre plugin utilise un service du plugin.
- Pas d'isolation des plugins non fiables (WASM ou processus) : dépend du
  runtime retenu.

## Motion Contracts U7 (`ui-core/src/motion.rs`)
- Rôles (`Feedback`, `Transition`, `Emphasis`) avec contrat : durée et
  amplitude maximales, interruptible ou non, dépassement maximal.
- Courbes : linéaire, Bézier cubique, ressort amorti (intégré au pas de
  1 ms) ; durée du ressort = temps de stabilisation.
- Refus avant démarrage : trop long, trop ample, dépassement hors borne,
  courbe invalide, animation non interruptible, budget d'animations
  simultanées ; un refus ne change rien.
- Reciblage continu en valeur **et** en vitesse ; réduction du mouvement
  (cible atteinte tout de suite, contrat toujours vérifié) ; suspension hors
  écran ; horloge virtuelle, trajectoires déterministes.
- Zones interactives cohérentes pendant les transitions : `motion::drive`
  recopie la valeur animée dans le décalage du nœud ; layout, test de
  frappe et Verify voient la position affichée.

## Design Contract U5 (`ui-core/src/tokens.rs`)
- Jetons couleur et dimension, références `{nom}`, thèmes qui remplacent
  des jetons de base ; résolution refusée si jeton inconnu, cycle ou type
  inattendu, dans la base comme dans chaque thème.
- Styles de composant en **noms** de jetons, avec variantes : un panneau de
  plugin hérite de la charte et du thème de l'hôte.
- Audit (Verify) : toute marge, tout écart, toute largeur ou hauteur fixe
  qui n'est pas une valeur de la charte est signalé ; sortie triée.
- Import et export DTCG (sous-ensemble : `$type` `color` et `dimension`,
  références, groupes de thèmes) ; sortie stable, aller-retour exact ;
  import fail-closed (13 documents invalides refusés, profondeur bornée).
- Pas encore : bordures, ombres, dégradés, transformations, masques et
  filtres (ils demandent le rendu de la pile).

## Responsive U6 (`ui-core/src/responsive.rs`)
- Profils PC, tablette, téléphone : taille logique, échelle du texte, zones
  système (encoches, barres), type d'entrée ; points de rupture par largeur.
- Gates stricts par profil : aucun diagnostic Verify, contenu de premier
  niveau dans la zone sûre, cibles tactiles ≥ 44 px logiques en tactile.
- Balayage **borné** (pas une analyse exacte) : pas fixe sur le domaine
  déclaré, plus chaque frontière de point de rupture et ses voisines à
  ±1 px ; couverture publiée (largeurs testées) et sous-score (fraction
  sans échec), déterministe.
- Pas encore : orientation, DPI physique, taille de texte appliquée à la
  mesure (la mesure vient de la pile, mais l'échelle du texte des profils
  ne lui est pas encore transmise).

## Verify complet U8 (`ui-core/src/verify.rs`)
- Registre de 8 règles (layout, design, accessibilité), chacune avec
  version, sévérité et poids. L'empreinte du ruleset est **figée** dans le
  code (`RULESET_LOCK`) : affaiblir, rétrograder ou retirer une règle sans
  mettre l'empreinte à jour dans le même changement fait refuser l'analyse.
- Anti-contournement : contenu caché dans une boîte de taille nulle,
  contenu poussé hors de la fenêtre (hors conteneur défilant).
- Sous-scores par catégorie (0 à 100, pénalité pondérée rapportée à la
  taille de l'arbre), score total = le plus faible.
- Rapports JSON et SARIF 2.1.0, triés, identiques d'une exécution à l'autre.
- Gate d'activation : refus si le candidat ajoute une **erreur** ; les
  avertissements et les constats déjà présents ne bloquent pas.
- Pas encore : contraste des couleurs (demande le rendu), ordre de lecture
  pour l'accessibilité.
- Ce gate est **branché** dans le hot reload (`hot`) et les plugins
  (`plugin`) à la place du gate minimal : avertissements non bloquants,
  contournements refusés, entrée tactile prise en compte, et rechargement
  refusé si le ruleset a été modifié sans son empreinte.

## Surfaces U9, côté données (`ui-core/src/surfaces.rs`)
- File audio → UI à un producteur et un consommateur, capacité fixe :
  `push` n'alloue pas et ne bloque pas (INV-05), refuse et compte la perte
  si la file est pleine ; sans `unsafe`.
- Pyramide min/max pour la forme d'onde : plage en O(log n), colonnes par
  pixel à tout zoom, égales au calcul direct.
- Timeline temps ↔ pixels, zoom borné autour du curseur (le temps sous le
  curseur ne bouge pas).
- Pas encore : spectrogramme, graphe de nœuds, canevas, vidéo, 3D, textures
  GPU partagées (demandent le rendu).

## Tests (`cargo test -p ui-core`, 43 tests)
| Critère du plan | Test |
|---|---|
| patch cassé refusé, état conservé | `broken_patches_are_refused_and_the_tree_is_kept` |
| patch périmé rejeté | idem (`Stale`) |
| état, focus et défilement conservés | `keyed_patch_keeps_identity_focus_and_scroll` |
| 10 000 événements et 20 éditions sans perte | `ten_thousand_events_and_twenty_edits_lose_nothing` |
| Verify : un positif et un négatif par règle | `verify_flags_overflow_out_of_container_and_overlap` |
| diagnostics déterministes | `verify_is_deterministic` (21 exécutions identiques) |
| L0/L1/L2 classés, état conservé | `hot::levels_are_classified`, `hot::l0_l1_l2_keep_state_and_focus` |
| rechargement refusé → version active intacte (5 causes) | `hot::refused_reloads_keep_the_whole_active_version` |
| 20 rechargements L2 et 10 000 événements sans perte | `hot::twenty_reloads_and_ten_thousand_events_lose_nothing` |
| U4 : un plugin ajoute, modifie et retire un panneau dans l'application ouverte | `plugin::a_plugin_adds_updates_and_removes_a_panel_in_a_running_app` |
| services, dépendances, cycles | `plugin::services_and_dependencies`, `plugin::dependency_cycles_are_refused` |
| refus sans effet (9 causes) | `plugin::refusals_change_nothing` |
| U7 : trajectoires vérifiées (états intermédiaires, dépassement d'un ressort) | `motion::tweens_follow_their_curve_on_the_virtual_clock`, `motion::springs_settle_and_respect_the_overshoot_bound` |
| U7 : contrats, reciblage, réduction, hors écran, budget | `motion::contracts_refuse_and_change_nothing`, `motion::retargeting_keeps_value_and_velocity_continuous`, `motion::interruption_budget_reduced_motion_and_offscreen` |
| U7 : zones interactives cohérentes pendant une transition | `motion::hit_zones_follow_the_animation` |
| U7 : déterminisme | `motion::trajectories_are_deterministic` |
| U5 : un composant de plugin hérite de la charte ; écarts détectés | `tokens::a_plugin_component_inherits_the_charter_and_deviations_are_flagged` |
| U5 : références, thèmes, chartes cassées refusées | `tokens::references_and_themes_resolve`, `tokens::broken_charters_are_refused` |
| U5 : DTCG aller-retour et refus | `tokens::dtcg_round_trip_and_refusals` |
| U6 : gates stricts sur les profils | `responsive::strict_gates_pass_on_every_profile`, `responsive::gates_catch_safe_area_touch_and_overflow` |
| U6 : sous-score responsive sur le domaine déclaré | `responsive::bounded_sweep_publishes_coverage_and_score` |
| U8 : un test positif et un négatif par règle | `verify::every_rule_has_a_positive_and_a_negative_case` |
| U8 : anti-contournement du ruleset | `verify::ruleset_lock_matches`, `verify::a_silently_weakened_rule_is_refused` |
| U8 : sorties identiques sur N exécutions, gate | `verify::scores_gate_and_reports_are_deterministic` |
| U9 : audio indépendant de l'UI, `push` sans allocation (allocateur compteur) | `surfaces::ring_push_never_allocates_and_drops_when_full`, `surfaces::ring_crosses_threads_in_order` |
| U8 : gate complet branché au hot reload et aux plugins | `hot::reloads_go_through_the_full_verify_gate`, `plugin::plugins_cannot_hide_content_to_pass_the_gate` |
| U9 : forme d'onde et timeline | `surfaces::peaks_match_a_direct_scan_at_every_zoom`, `surfaces::timeline_zoom_keeps_the_cursor_time_fixed` |
| découpage, ordre du focus | `clipped_children_are_not_hit`, `focus_cycles_in_tree_order` |

L'application de qualification 0 (compteur et liste de tâches, PLAN §6) sert
de scène aux tests.

## Limites
- Écrit en Rust, pas en Ravum : l'écrire en Ravum demande RFC-UI-LANG-2
  (handles générationnels), en attente de décision.
- Pas de rendu, pas de fenêtre, pas de vrai texte (IME, bidi), pas
  d'accessibilité : ils dépendent de la pile.
- Le comportement L2 est une table de fonctions Rust compilées : le
  rechargement de code réel passera par le hot swap Ravum (IR/JIT).
- Le critère de sortie d'U2 (application interactive sous Linux, Windows,
  macOS) et celui d'U3 (hot reload L0 à L2) ne sont **pas** atteints : ce
  prototype n'en couvre que la partie logique.

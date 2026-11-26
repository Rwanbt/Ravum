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
  (8 × 16 px), substitut assumé.
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

## Tests (`cargo test -p ui-core`, 12 tests)
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

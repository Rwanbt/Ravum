# U2 — `ui-core` dans une vraie fenêtre (pile modulaire, UI-D21)

Statut : **banc**, `TESTED` sous Linux (Xvfb, Vulkan logiciel). Code :
[`bench/ui-candidates/ui-shell`](../../bench/ui-candidates/ui-shell/src/lib.rs).
Rien n'entre dans un crate Ravum.

## Ce qui existe
- `ui-core` garde l'arbre, le layout, le focus, le défilement et la
  logique ; la coquille ne fait que traduire les entrées et exécuter des
  **ordres de dessin** (`paint`) : fonds, cadre de focus, texte découpé par
  ses ancêtres. Les ordres sont calculés sans GPU, donc testés partout.
- Application de qualification 0 (PLAN §6) : compteur (+1, −1), liste de
  30 tâches défilante, ajout, bascule d'une tâche. Vue = fonction pure de
  l'état ; chaque clic → patch par clé ; une vue refusée laisse l'arbre
  actif intact (fail-closed).
- Fenêtre winit, rendu Vello sur wgpu, texte Parley (mise en forme gardée
  en cache par texte et échelle) ; souris, molette (lignes et pixels),
  Tab, redimensionnement, facteur d'échelle de l'écran.
- Démarrage repris du banc (première frame, BENCH-U1B-ROUND3) : GPU,
  pipelines et polices préparés dans un thread pendant la fenêtre ; API
  graphique native seule ; anticrénelage « zone ».

## Tests (`cargo test -p ui-shell`, 7 tests)
| Critère | Test |
|---|---|
| aucune erreur Verify à 1280 × 800, 800 × 600, 640 × 480 | `the_view_passes_verify_on_every_size` |
| boutons, identité et focus conservés à travers les patchs | `buttons_change_the_counter_and_keep_identity` |
| bascule, ajout ; défilement conservé après un patch | `tasks_toggle_add_and_scroll_keeps_state` |
| lignes défilées hors vue non dessinées, lignes partielles découpées | `scrolled_out_rows_are_not_painted_and_partial_rows_are_clipped` |
| clics hors nœud ou sur du texte seul sans effet (négatif) | `clicks_outside_any_node_and_on_text_only_areas_change_nothing` |
| Tab et cadre de focus | `tab_cycles_focus_and_paint_marks_it` |
| 1 000 événements aléatoires : état et dessin identiques d'une exécution à l'autre | `a_thousand_events_are_deterministic` |

Essai réel (Xvfb, lavapipe, entrées X11 par `xdotool`) : la fenêtre
s'ouvre, trois clics sur +1 et un sur −1 donnent « compteur : 2 », la
molette fait défiler la liste avec découpage, le clic sur « ajouter » lui
donne le focus. Captures : [avant](img/u2-app0.png),
[après](img/u2-app0-input.png).

## Pas encore (critère de sortie d'U2 non atteint)
- Windows et macOS : jamais lancés.
- Texte : `ui-core` mesure en monospace fixe (8 × 16 px) alors que Parley
  dessine une police proportionnelle ; le layout doit utiliser la mesure de
  Parley (prochaine étape).
- Le focus clavier ne fait pas défiler son nœud dans la vue.
- Pas d'IME, de bidi, d'accessibilité (AccessKit), de survol ni d'état
  pressé ; pas de styles issus des jetons U5.
- Hot reload L0 à L2 (U3) pas encore branché sur la fenêtre.

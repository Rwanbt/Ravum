# U3 — Rechargement à chaud branché sur la fenêtre (L0, L1)

Statut : **banc**, `TESTED` sous Linux (Xvfb, Vulkan logiciel). Code :
[`bench/ui-candidates/ui-shell`](../../bench/ui-candidates/ui-shell/src/lib.rs)
(`Skin`, `Shell::reload_skin`) et `src/main.rs` (`UI_SHELL_SKIN`). Rien
n'entre dans un crate Ravum.

## Ce qui existe
- **Habillage** (`Skin`) de l'application 0 : dimensions (barre, boutons,
  lignes, espacements) et libellés, dans un texte `clé = valeur` (`#`
  commente, clé absente = valeur par défaut).
- **Lecture fail-closed** : la moindre ligne fautive refuse tout le texte,
  avec la ligne et la clé : syntaxe, clé inconnue, clé en double, nombre
  non fini, négatif ou au-delà de 4096, libellé vide ou avec un caractère
  de contrôle.
- **Rechargement transactionnel** (`Shell::reload_skin`) : la vue candidate
  est construite à part, **avec la mesure du texte active** (Parley dans la
  fenêtre), les états (focus, défilement) y sont recopiés, puis le gate
  Verify complet compare candidat et vue active. Le candidat n'est activé
  qu'après tous les contrôles ; au moindre refus, vue, habillage, état de
  l'application, focus et défilement restent tels quels. Le niveau rendu
  vient de `hot::classify` : `None`, `Style` (L0) ou `Structure` (L1).
- **Fenêtre** : `UI_SHELL_SKIN=fichier` relit le fichier toutes les 250 ms
  (pas de dépendance de notification ; un éditeur qui remplace le fichier
  est vu comme une écriture). Un contenu nouveau est rechargé une fois ;
  acceptation et refus s'affichent sur stderr. La relecture ne commence
  qu'une fois la mesure Parley installée.
- **Correction dans `ui-core`** : le gate de `hot::Session` construisait sa
  sonde avec la mesure monospace par défaut, donc jugeait un candidat sur
  d'autres métriques que celles affichées. La sonde reprend désormais la
  mesure active (`Ui::measure`), et `Session::set_measure` existe.

## Tests
`ui-shell` (13 tests, dont 6 nouveaux) :

| Critère | Test |
|---|---|
| L0 puis L1 : état, focus et défilement conservés ; le nouvel habillage sert aux patchs suivants | `l0_and_l1_reloads_keep_state_focus_and_scroll` |
| habillage identique : niveau `None`, aucune génération consommée | `an_identical_skin_is_level_none_and_changes_nothing` |
| dix textes fautifs refusés avec ligne et clé (négatif) | `bad_skins_are_refused_line_by_line` |
| refus de lecture puis refus Verify (libellés qui débordent) : rien ne change, dessin identique (négatif) | `a_refused_reload_leaves_everything_active` |
| 400 habillages arbitraires : jamais de panique ; soit refus sans effet, soit vue sans diagnostic (adversarial) | `adversarial_skins_never_break_the_active_view` |
| critère de sortie U3 : 10 000 événements et 20 éditions (acceptées et refusées) ; état et focus identiques à un jumeau sans rechargement | `ten_thousand_events_and_twenty_edits_lose_nothing` |

`ui-core` (44 tests) : `the_gate_measures_candidates_like_the_active_view`
(un libellé qui tient en monospace mais pas avec la mesure active est
refusé). Vérifié : ce test échoue si la correction est retirée.

Essai réel (Xvfb, lavapipe) : fenêtre lancée avec un habillage ; écriture
d'un habillage L1 (lignes plus hautes, bouton renommé « nouvelle tâche »)
→ « habillage activé : Structure » et l'écran change
([capture](img/u3-skin-reload.png)) ; puis `button.width = 12` → refus
Verify (`layout.overflow` sur les trois boutons) et `bar.gap = -3` → refus
de lecture ; la capture après ces deux refus est identique octet pour
octet à celle d'avant.

## Pas encore
- **L2** (comportement) dans la fenêtre : `hot::Session` sait remplacer une
  table de gestionnaires versionnée, mais charger du comportement depuis
  l'extérieur du binaire demande du code chargé à chaud (langage Ravum ou
  bibliothèque dynamique) ; pas branché.
- La structure de la vue reste dans le code ; seul l'habillage est
  rechargeable. Une description de vue externe viendra avec le langage.
- Pas de couleurs dans l'habillage (les teintes ne sont pas encore des
  styles `ui-core` ; elles viendront des jetons U5).
- Windows et macOS : jamais lancés.

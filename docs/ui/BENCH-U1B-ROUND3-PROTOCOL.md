# U1-B — Tour 3 : protocole à lancer sur vos machines (UI-D9)

Statut : scripts **prêts**, mesures **`NOT_RUN`**. Le tour 3 demande une
fenêtre et un GPU réels ; le conteneur de développement n'en a pas. Aucune
pile n'est adoptée ; le choix reste au responsable (UI-D4).

## 1. Ce qui est mesuré

La même scène dans chaque pile complète : une liste de 10 000 lignes
virtualisée (libellé `param i: v` et slider, 16 px par ligne, 50 lignes
visibles dans une fenêtre de 1280 × 800). À chaque frame, un paramètre
visible change, comme pendant un glissement de curseur, et un redessin est
demandé. Après 600 frames, l'application se ferme seule.

| Pile | Application | Rôle |
|---|---|---|
| egui (eframe, wgpu) | [`round3/egui`](../../bench/ui-candidates/round3/egui/src/main.rs) | référence de réactivité |
| Slint | [`round3/slint`](../../bench/ui-candidates/round3/slint/src/main.rs) | référence seulement (UI-D12, licence) |
| Modulaire (winit, wgpu, Vello, Parley) | [`round3/modular`](../../bench/ui-candidates/round3/modular/src/main.rs) | candidate ; runtime retained minimal : mise en forme du texte en cache, seule la ligne changée est refaite |

Par exécution :
- mesuré par l'application ([`round3/common`](../../bench/ui-candidates/round3/common/src/lib.rs)) :
  délai du début de `main` à la première frame, intervalles entre frames
  (p50, p95, p99, max) ;
- mesuré par le lanceur, de l'extérieur et de la même façon pour toutes
  les piles : temps écoulé, temps CPU (utilisateur et système, donc CPU par
  frame), pic de mémoire résidente ; taille du binaire `release`.

**Blitz** n'est pas inclus : le banc ne contient que `blitz-dom`, pas une
pile applicative, et Blitz est en bêta avec des dépendances MPL-2.0
(tour 2, §4.4). Il peut être ajouté si vous le souhaitez.

## 2. Lancer

Prérequis : Rust 1.95 (`rust-toolchain.toml` du dépôt), Python 3.9 ou plus,
le dépôt à jour. Sous Linux, les bibliothèques de fenêtrage habituelles
(`libxkbcommon-x11`, Wayland ou X11) et un pilote Vulkan ou GL. Branchez
l'ordinateur portable sur secteur, fermez les autres applications, laissez
la fenêtre au premier plan.

```bash
cd bench/ui-candidates
python3 round3/run_round3.py --runs 5 --note "GPU, écran, Hz, secteur"
```

Sous Windows (PowerShell) :

```powershell
cd bench\ui-candidates
py round3\run_round3.py --runs 5 --note "GPU, écran, Hz, secteur"
```

Le lanceur compile chaque pile (`cargo build --release --locked`), lance
5 exécutions par pile et écrit un répertoire `round3-<date>/` avec
`report.json`, les journaux de compilation et d'exécution, et les rapports
bruts. Options : `--stacks modular,egui`, `--frames`, `--rows`, `--out`,
`--no-build`.

**À renvoyer** : le répertoire `round3-<date>/` complet, avec dans `--note`
le GPU, la définition et la fréquence de l'écran, et l'alimentation.

## 3. Règles du lanceur (fail-closed)

- Une exécution compte seulement si le processus sort avec le code 0 **et**
  a écrit un rapport valide (bon schéma, bonne pile, bon nombre de frames)
  **et** si toutes les mesures externes ont été prises.
- Verdict par pile : `PASS` (toutes les exécutions valides), `FAIL` (au
  moins une invalide, avec la raison), `NOT_RUN` (compilation impossible ou
  interruption). Code de sortie 0 seulement si toutes les piles sont `PASS`.
- `PASS` veut dire « mesures prises », pas « pile gagnante ».
- Tests : `python3 -m unittest round3/test_run_round3.py` (8 tests :
  sortie non nulle, absence de rapport, mauvais schéma, mauvais nombre de
  frames, mesure manquante, délai dépassé).

## 4. Vérifié dans le conteneur (essai fonctionnel, pas une mesure)

Sous Xvfb avec Vulkan **logiciel** (lavapipe), le 2026-11-25 :
- les trois applications compilent, s'ouvrent, affichent la scène et se
  ferment seules ; des captures d'écran ont confirmé 50 lignes visibles de
  16 px dans chaque pile, texte compris ;
- `run_round3.py --runs 2 --frames 60` donne `PASS` pour les trois piles.

Les chiffres de cet essai **ne sont pas publiés** : avec lavapipe, le
« GPU » est exécuté par le CPU, ce qui fausse le temps CPU par frame et les
intervalles. Seules vos machines donnent des mesures.

## 5. Pas encore couvert par ces scripts

- Repos (CPU et réveils quand rien ne change), latence d'entrée, VRAM et
  temps GPU par frame : ils demandent des outils propres à chaque OS (voir
  `PLAN.md` §7) ; à ajouter après un premier passage.
- Texte long, IME, texte bidirectionnel, accessibilité : tour 4.
- Android et iOS.

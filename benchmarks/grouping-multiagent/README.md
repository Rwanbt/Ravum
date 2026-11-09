# grouping-multiagent — fréquence des conflits multi-agents et rentabilité du groupement

Harnais **Node 26 zéro dépendance npm** du lot L2.2 (MVP-0c). Il répond à une
question laissée ouverte par la mesure L2 phase 1 : *quelle fréquence réelle de
conflits multi-agents rend le groupement de patches non rentable ?*

Le harnais fait deux choses, séparées explicitement :

1. **décisions réelles** : il invoque le binaire `ravum-collab` (coordinateur
   COLLAB-1→5 + vérification sémantique `rustc` opt-in) sur quatre familles de
   scénarios multi-agents, ≥ 5 répétitions, ordre alterné, et vérifie les
   sorties (décisions attendues, chaîne de snapshots, digests de contenu
   identiques et zéro changement perdu) ;
2. **modèle d'économie calibré et étiqueté MODÈLE** : les coûts unitaires
   (cycle d'analyse 191 ms, refus de lot 184 ms, reprise ~1596 ms) sont lus dans
   la preuve de la campagne L2 phase 1
   (`evidence/mvp0c-grouping-2026-11-09.json`) ; les fréquences de conflit
   injectées dans le modèle sont celles mesurées à l'étape 1. Le groupement
   n'est **pas implémenté** : il est modélisé, et le modèle ne remplace pas une
   mesure bout en bout.

## Scénarios (fixtures sous `fixtures/`)

| ID | Agents | Situation | Décisions attendues |
|---|---:|---|---|
| `a-distinct-files` | 3 | un agent par fichier (`src/alpha.rs`, `src/beta.rs`, `src/gamma.rs`) | `AUTO_SAFE` ×3 |
| `b-distinct-symbols-one-file` | 5 | 5 fonctions distinctes du même `src/lib.rs` | `AUTO_SAFE` ×5 |
| `c-same-function` | 3 | 3 agents modifient la même fonction `shared` | `AUTO_SAFE`, `CONFLICT`, `CONFLICT` |
| `d-undeclared-dependency` | 3 | agent B modifie `compose` qui appelle `helper` modifié par l'agent A, sans le déclarer | `AUTO_SAFE`, `MANUAL_REQUIRED`, `AUTO_SAFE` |

Les propositions JSON sont **générées depuis la base réelle** : le harnais
appelle `ravum-collab base --workspace …` pour obtenir `snapshot_id` et les
hashs, construit les hunks unified-diff exacts, puis soumet chaque proposition
dans l'ordre des fichiers `01-…json`.

## Usage

Depuis la racine du dépôt :

```powershell
# binaire déjà construit (sinon une seule fois : cargo build -p ravum-workspace-coordinator --bins)
node benchmarks/grouping-multiagent/run.mjs --reps 5
# options : --keep (conserve target/grouping-multiagent), --bin <path>, --out <path>
```

Preuve par défaut : `evidence/mvp0c-grouping-multiagent-2026-11-09.json`.
Code de sortie : 0 si toutes les décisions sont celles attendues, que les
digests correspondent, qu'aucun changement n'est perdu et que le modèle a pu
être calculé ; 1 sinon.

## Investigation L4.5 — échec transitoire de vérification sous charge

`investigate-transient.mjs` caractérise l'incident « 1 run/20 en `REJECT` sous
charge, sans E-code » : 20 runs réels du scénario d sous 3 conditions (repos,
charge CPU bornée, charge mémoire/I/O bornée) + 12 compilations `rustc` directes
avec les arguments exacts du vérificateur par condition, et capture décisions,
stderr, code de sortie, durées, RAM. Il n'applique aucun correctif et ne touche
pas à `crates/**`.

```powershell
node benchmarks/grouping-multiagent/investigate-transient.mjs
# preuve : evidence/mvp0c-verification-transient-2026-11-09.json
```

## Vérifications de contenu

- le digest de snapshot est recalculé côté harnais avec **la même formule que
  `ravum-snapshot`** (`ravum-snapshot-v1\n` + par chemin trié `path\0len\0content\0`)
  et comparé à l'identifiant réel imprimé par `ravum-collab base` puis au
  `snapshot_after` du run ;
- la chaîne des `accepted_changes` doit être contiguë (`before(i)=after(i−1)`),
  le premier `before` égal à la base et le dernier `after` égal au snapshot final ;
- chaque édition acceptée est présente dans le contenu final attendu, le
  marqueur d'une édition refusée est absent (zéro changement perdu, zéro
  changement refusé appliqué malgré tout).

## Limites

Scénarios générés par des **éditeurs scriptés déterministes** (pas de LLM), une
seule petite workspace par scénario, N fixés (3/5/3/3), deux familles de
scénarios conflictuelles **par construction** : la fréquence de conflits mesurée
(50 % des lots) est un artefact du corpus, pas une estimation de production.
Le modèle utilise le coût de reprise mesuré à N=6 comme constante (conservateur
pour N<6) et ne prédit pas au-delà de N=6 ni des fenêtres > 100 ms. Machine de
référence partagée avec d'autres workers.

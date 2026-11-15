# RAVUM — Audit architectural ciblé, seconde passe (2026-11-14)

> **Statut : rapport d'audit, non normatif.** Il ne ratifie aucun ADR ni aucune RFC. Les décisions DA…DG sont des recommandations `PROPOSED`, à soumettre au responsable. Il fait suite à [`2026-11-13-architecture-audit.md`](2026-11-13-architecture-audit.md).

Périmètre : branche `goal/production-ready`, 83 commits devant `dev`, PR brouillon #110. Neuf fichiers consultés : le rapport final du GOAL, `LANG-TIMINGS.md`, et par extraits `differential_oracle.rs`, `emit.rs`, `parser.rs`, `loader.rs`, `source.rs`, `traps.rs`, plus la taille des exemples. Aucun build ni test lancé : les chiffres viennent du rapport de l'agent, seul le code qui les produit a été vérifié.

## 1. Synthèse exécutive

L'agent a exécuté le GOAL précédent honnêtement sur l'essentiel : il déclare lui-même trois chantiers non faits et ne revendique aucune preuve formelle. La gouvernance est corrigée, la boucle `.rv` est mesurée, le profil `rt` ferme les traps, le LSP met le parse en cache, le filet `forbid(unsafe_code)` est gardé par des tests.

Deux conclusions du rapport ne tiennent pas à la lecture du code :

1. **L'oracle différentiel n'est pas génératif.** Il concatène 42 gabarits d'instructions fixes et indépendantes. Les « 40 200 programmes, 0 divergence » explorent environ 42 formes. La classe « `check` accepté, `build` refusé » n'est pas mieux fermée qu'avant.
2. **Toutes les mesures portent sur des programmes de 5 à 338 lignes.** « Incrémental sans gain », « diagnostic LSP p95 2,8 ms » et « cible P0-A atteinte » ne disent rien d'un programme réel.

Une erreur du premier audit est à corriger : les `NodeId` étaient bien par fichier. Le vrai défaut est ailleurs : plafond dur de 200 fichiers sources par programme, et `FileId` attribué dans l'ordre de chargement.

Restent ouverts : le backend re-dérive toujours les moves (`MovePlanner`), le profil `rt` s'applique au paquet entier et rend l'exemple DSP incompilable, et les deux lignes (langage, hot swap) ne sont toujours pas reliées.

Recommandation : faire relire et intégrer la PR #110 avant d'empiler, puis traiter dans l'ordre le vrai générateur, un corpus à l'échelle, et le profil `rt` par point d'entrée.

## 2. État réel et niveau de confiance

| Sujet du premier audit | État au 14 novembre | Preuve lue | Confiance |
|---|---|---|---|
| C1 mandat périmé | Corrigé ; `AGENTS.md` en `PENDING_HUMAN_APPROVAL` | `AGENTS.md` révisé | Élevée |
| C3 rôle de `rustc` | Corrigé, deux tests de garde | rapport §2 phase 1 (tests non lus) | Moyenne |
| C2 divergences `check`/`build` | **Non fermé** : oracle par gabarits | `differential_oracle.rs:145-268` | Élevée (constat) |
| C2 cause : double modèle de moves | **Confirmé** : `MovePlanner` toujours dans le backend | `emit.rs:1832`, `emit.rs:3893` | Élevée |
| `OwnFacts` | Étapes A et B ; garde actif en debug seulement | `emit.rs:1003` | Élevée |
| C4 mesure de la boucle | Fait, sur exemples de 5 à 338 lignes | `LANG-TIMINGS.md`, tailles de `examples/` | Élevée (constat) |
| `-C incremental` | Opt-in, désactivé : gain nul sur ces exemples | rapport §1 | Faible à l'échelle |
| C5 `NodeId` globaux | **Hypothèse du premier audit fausse** : octet haut = fichier | `parser.rs:68-86` | Élevée |
| C6 traps en `rt` | Implémenté, `PENDING_HUMAN_APPROVAL` | `traps.rs:1-15` | Moyenne |
| Exemple DSP en `rt` | Non fait : trois obstacles | rapport §2 phase 5 | Élevée (constat) |
| Cache de parse LSP | Fait ; la sémantique reste globale | `loader.rs:185-196` | Moyenne |
| Verticale DSP `.rv` → hot swap | Non faite, RFC seule | rapport §2 phase 8 | Élevée (constat) |
| C9 répertoire `C:` | Supprimé après inspection | rapport §2 phase 1 | Moyenne |

Chiffres rapportés par l'agent, non rejoués : conformance 476 cas, 0 échec ; 1 605 tests ; couverture 89,47 / 88,87 ; mutation 183 mutants, 150 tués, 24 survivants.

## 3. Architecture actuelle et écarts avec la vision

La chaîne n'a pas changé de forme : un fichier Rust unique par programme, compilé par `rustc`. Trois ajouts : une passe `traps.rs` pour `rt`, des faits d'ownership enregistrés mais non consommés, un cache de parse dans le LSP.

| Vision | Réalité au 14 novembre | Écart |
|---|---|---|
| Compilation incrémentale rapide | `rustc` représente 97 à 99 % d'un build de 179 à 331 ms, sur des programmes minuscules. Aucune mesure à 5 000 lignes ou plus. | Inconnu à l'échelle |
| Mise à jour à chaud | Redémarrage. Bloc DSP non commencé. | Inchangé |
| Sûreté mémoire | `forbid(unsafe_code)` gardé par tests. | Faible |
| Prévention des crashs | Fermée en `rt` seulement ; `core` et `pure` inchangés par choix. | Partiel |
| Temps réel DSP | Le profil `rt` refuse l'exemple DSP du dépôt. | Le profil n'a aucun consommateur réel |
| Passage à l'échelle | 200 fichiers sources maximum, manifestes et dépendances compris. | Plafond dur |

## 4. Problèmes critiques

**N1 — RISQUE AVÉRÉ (P0). L'oracle différentiel surestime sa preuve.**
- Preuves : `differential_oracle.rs:145` déclare `TEMPLATES`, 42 chaînes. `generate_program` (ligne 257) tire un gabarit et deux littéraux par instruction. Chaque gabarit déclare ses propres variables suffixées, donc les instructions n'interagissent que par un accumulateur entier.
- Indice concordant : 100 % des programmes sont acceptés par `check`. Un générateur qui explore produit des refus.
- Impact : le critère R2 est écrit « fait sur le périmètre mesuré » et le gate T2 « preuve renforcée ». La valeur réelle est celle de 42 cas de conformance. Les 13 formes historiques venaient toutes de compositions (imbrication, branches, shadowing) que ce générateur ne produit pas.
- Correction : générateur dirigé par les types avec environnement (décision DA). Requalifier R2 en `PARTIAL` d'ici là.

**N2 — RISQUE AVÉRÉ (P0). Aucune mesure à l'échelle.**
- Preuves : le plus gros exemple fait 338 lignes (`examples/file-store`), les neuf autres 5 à 44.
- Impact : trois conclusions sont non démontrées au-delà du jouet : incrémental `rustc` inutile, latence LSP sous la cible, coût de `check`. La vérification sémantique du LSP reste globale, donc linéaire dans la taille du paquet.
- Correction : corpus synthétique paramétrable et un programme réel (décision DB).

**N3 — BUG CONFIRMÉ (P1). Plafond de 200 fichiers et `FileId` par ordre de chargement.**
- Preuves : `loader.rs:71` (`MAX_SOURCE_FILES = 200`), refus à `loader.rs:2502`. `source.rs:16` attribue `FileId = files.len()`, manifestes `ravum.toml` compris. `parser.rs:77` masque le fichier sur 8 bits.
- Impact : un programme avec ses dépendances ne peut pas dépasser 200 fichiers. Ajouter un fichier décale le `FileId` des suivants, donc leurs `NodeId`, et invalide le cache de parse, indexé par (hash, `FileId`). Le test de stabilité de l'agent ne couvre que l'édition, pas l'ajout.
- Garde-fou existant : le loader refuse avant le masque, donc pas de collision silencieuse aujourd'hui.
- Correction : décision DC.

**N4 — RISQUE AVÉRÉ (P1). Le backend re-dérive toujours les moves.**
- Preuves : `MovePlanner` (`emit.rs:3893`), utilisé ligne 1832. Le garde `OwnFacts` sort immédiatement hors debug (`emit.rs:1003`). L'agent liste ses exclusions : identité de liaison, récepteurs de méthode, arguments de référence.
- Impact : la cause structurelle de C2 est intacte. Le garde ne voit que les programmes des tests, donc ceux de l'oracle N1.
- Correction : décision DE, après DA.

**N5 — RISQUE AVÉRÉ (P1). Le profil `rt` est inutilisable pour du DSP.**
- Preuves : `traps.rs:24-44` parcourt toutes les fonctions du programme. Le rapport cite trois obstacles pour l'exemple DSP : indexation dynamique sans forme sûre, `alloc`, `io`.
- Cause : le profil s'applique au paquet, `main` compris. Un programme audio a toujours une partie non temps réel (chargement, affichage) et un callback temps réel.
- Impact : la règle de langage attend une approbation humaine alors qu'aucun programme utile ne peut la satisfaire.
- Correction : décision DD, à joindre à la RFC avant approbation.

**N6 — RISQUE AVÉRÉ (P1). Langage et hot swap toujours disjoints.** Phase 8 non commencée. L'agent a relu l'ABI réelle et écrit un découpage minimal dans `RFC-RV-DSP-BLOCK.md` §7 (non lu).

**N7 — RISQUE AVÉRÉ (P2). Dérive documentaire.**
- Preuves : sur les 45 derniers commits, une trentaine sont documentaires, dont plusieurs corrigent des chiffres périmés (`aa88479`, `09474d0`). Le journal fait 1 018 lignes. Les mêmes chiffres sont recopiés dans au moins quatre documents.
- Impact : chaque mesure rejouée exige plusieurs corrections manuelles ; la relecture humaine de la PR est noyée.
- Correction : décision DF.

**N8 — NON VÉRIFIÉ. Les 24 mutants survivants « triés ».** Sur un borrow checker, chaque survivant est une règle non testée. Le tri n'a pas été lu.

**N9 — Processus (P0, humain).** 83 commits non relus, `AGENTS.md` en attente, cinq RFC en attente, décision D10 en attente (l'agent documente que 100 % de régions est inatteignable par tests). Le workflow `linux-qual.yml` ne se déclenche pas sur `goal/**`.

## 5. Décisions architecturales recommandées

**DA — Générateur dirigé par les types.**
- Options : (a) ajouter des gabarits ; (b) générateur à environnement typé ; (c) outil externe de fuzz structuré.
- Choix : (b). (a) ne compose pas, (c) ajoute une dépendance.
- Conception :
  - un environnement de variables vivantes avec type et état (initialisée, déplacée, partiellement déplacée, empruntée) ;
  - `gen_expr(type, profondeur)` choisit parmi les règles qui produisent ce type : littéral, variable vivante, projection de champ ou d'index, appel, constructeur, `if`/`match` en expression, bloc ;
  - `gen_stmt` choisit `let`, affectation, `if`, `match`, boucle, bloc imbriqué avec shadowing, et met l'environnement à jour ;
  - à chaque jonction de branches, l'état est l'intersection, comme dans `own.rs` ;
  - une fraction contrôlée de programmes est rendue volontairement invalide (usage après move) pour vérifier que `check` refuse.
- Validation : le taux d'acceptation par `check` est publié et strictement inférieur à 100 % ; un histogramme des constructions générées prouve que chaque règle est tirée ; **le générateur retrouve au moins 10 des 13 formes historiques quand on annule leur correctif** (test de puissance, à faire sur une copie de travail).
- Conséquence : seul ce test de puissance autorise à réécrire R2.

**DB — Corpus à l'échelle.**
- Choix : deux sources. Un générateur de paquets synthétiques (M modules × F fonctions, types partagés, graphe d'appels) à 1 000, 5 000, 20 000 et 50 000 lignes. Un programme réel d'au moins 3 000 lignes écrit en Ravum.
- Compromis : le synthétique mesure, le réel révèle les manques du langage.
- Validation : courbes de `check`, `build`, LSP en fonction de la taille ; RAM maximale ; décision sur `-C incremental` reprise sur ces courbes.

**DC — Identité de fichier.**
- Options : (a) relever la limite en gardant 8 bits (impossible au-delà de 254) ; (b) `NodeId` sur 64 bits, 32 pour le fichier et 32 pour l'index local ; (c) structure à deux champs.
- Choix : (b). `NodeId` est un alias (`ast.rs:14`) et les décalages sont concentrés dans `parser.rs` et `loader.rs`.
- Complément : dans le LSP, un interneur de chemins conservé pour la session, qui ne fait que grandir. Un chemin garde son `FileId` quand on ajoute un fichier.
- Compromis : les clés des tables de faits doublent de taille. À mesurer avec DB.
- Validation : paquet de 1 000 fichiers accepté ; ajout d'un fichier sans changement des `NodeId` des autres dans une session LSP ; Rust généré identique sur le corpus.

**DD — Profil `rt` par point d'entrée, et indexation bornée.**
- Options : (a) statu quo, paquet entier ; (b) option `--rt-entry <fonction>` : les règles `rt` s'appliquent aux fonctions atteignables depuis ce point d'entrée ; (c) nouveau mot-clé sur la fonction.
- Choix : (b). Aucun changement de syntaxe, compatible avec le gel de surface, et c'est exactement ce dont la verticale DSP a besoin.
- Règle d'indexation à ajouter : `a[i]` est sûr si `i` est la variable d'un `for i in 0..K` avec `K` littéral inférieur ou égal à la taille du tableau, ou `0..a.len()` quand `a` est un tableau ou une tranche non réaffecté dans le corps. La longueur d'un tableau ou d'une tranche ne change pas, la règle est donc sûre sans analyse d'intervalles.
- Validation : `examples/dsp-gain-iir` compile avec son `main` qui affiche et son traitement en `rt` ; cas négatifs pour un index hors de la forme reconnue.
- Statut : amendement de `RFC-RT-TRAPS.md`, décision du responsable.

**DE — Finir la source unique des moves.**
- Choix : trois temps. Étendre le garde aux trois exclusions. Le faire tourner sous le générateur DA. Remplacer ensuite `MovePlanner` par la lecture des faits.
- Conséquence : tant que DA n'existe pas, supprimer `MovePlanner` est un pari sans filet. Le dernier temps reste un travail pour un modèle fort.
- Validation : zéro désaccord sur le corpus et le fuzz long ; Rust généré identique.

**DF — Une seule source pour les chiffres.**
- Choix : un script produit `evidence/current.json` (SHA, commandes, résultats). Les documents citent ce fichier ou sont régénérés depuis lui. `report_consistency.py` échoue si un document porte un chiffre différent.
- Validation : rejouer une mesure modifie un fichier, pas quatre.

**DG — Intégration (décision du responsable).** Relire et intégrer la PR #110 avant tout nouveau travail. Ordre de relecture conseillé : `traps.rs` et sa RFC, le garde dans `emit.rs`, `ParseCache` dans `loader.rs`, l'option incrémentale dans `pipeline.rs`, puis `AGENTS.md`.

## 6. Plan de correction

Complexité : F faible, M moyenne, É élevée.

### P0

| ID | Problème | Modules | Solution | Dépend de | Tests | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|---|---|
| Q0-1 | PR #110 non relue (N9) | PR, `AGENTS.md`, 5 RFC, D10 | Relecture et décisions | — | CI de la PR | PR intégrée ou renvoyée | — | Non : humain |
| Q0-2 | Preuve surestimée (N1) | Rapport final, matrice des gates, `PRODUCTION-READINESS.md` | R2 en `PARTIAL`, décrire l'oracle comme « 42 gabarits » | — | `report_consistency.py` | Plus aucune phrase « 40 200 programmes » sans la réserve | F | Oui |
| Q0-3 | Oracle non génératif (N1) | `differential_oracle.rs` | Décision DA | Q0-1 | Test de puissance sur les formes historiques | Taux d'acceptation < 100 %, histogramme, ≥ 10 formes sur 13 retrouvées | É | Partiel |
| Q0-4 | Pas de mesure à l'échelle (N2) | `scripts/`, `tools/`, `LANG-TIMINGS.md` | Générateur de paquets synthétiques, 4 tailles | — | Lui-même | Courbes publiées avec RAM maximale | M | Oui |

### P1

| ID | Problème | Modules | Solution | Dépend de | Tests | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|---|---|
| Q1-1 | Plafond de 200 fichiers (N3) | `ast.rs`, `parser.rs`, `loader.rs`, `source.rs` | DC : `NodeId` 64 bits | Q0-4 | Paquet de 1 000 fichiers | Accepté, Rust généré identique | M | Oui |
| Q1-2 | `FileId` instable (N3) | `ravum-lang-lsp`, `loader.rs` | Interneur de chemins de session | Q1-1 | Ajout de fichier en session | `NodeId` des autres fichiers inchangés | M | Oui |
| Q1-3 | `rt` par paquet (N5) | `traps.rs`, `check.rs`, `effects.rs`, CLI, RFC | DD après approbation | Q0-1 | Cas positifs et négatifs | Exemple DSP compilé en `rt` | M | Oui après RFC |
| Q1-4 | Garde `OwnFacts` partiel (N4) | `emit.rs`, `own.rs` | Étendre aux 3 exclusions | Q0-3 | Tests négatifs par exclusion | 0 désaccord sous DA | M | Partiel |
| Q1-5 | Dérive documentaire (N7) | `scripts/`, `tools/report_consistency.py` | DF | — | Test du contrôleur | Chiffre divergent ⇒ échec | M | Oui |
| Q1-6 | Mutants survivants (N8) | `own.rs`, `typeck.rs`, `resolve.rs` | Un test par survivant ou justification écrite | — | Campagne rejouée | Survivants en baisse, liste publiée | M | Oui |
| Q1-7 | `linux-qual.yml` muet sur `goal/**` | `.github/workflows/` | Ajouter le motif de branche | Q0-1 | Run CI | Déclenchement automatique | F | Oui |

### P2

| ID | Problème | Solution | Dépend de | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|
| Q2-1 | Sémantique LSP globale | Décider sur les courbes Q0-4 ; si besoin, revérifier seulement les modules dépendants | Q0-4, Q1-2 | p95 < 50 ms à 20 000 lignes, ou écart publié | É | Non |
| Q2-2 | `MovePlanner` (N4) | Dernier temps de DE | Q0-3, Q1-4 | Rust généré identique, planner supprimé | É | Non |
| Q2-3 | Lignes disjointes (N6) | Verticale DSP selon la RFC §7 | Q1-3 | Swap sans allocation, refus sur schéma différent | É | Non pour l'ABI |
| Q2-4 | Incrémental | Reprendre la décision sur les courbes ; un crate par module si `rustc` domine | Q0-4 | Gain mesuré à 20 000 lignes | É | Non |
| Q2-5 | Programme réel | Écrire ≥ 3 000 lignes en Ravum et consigner chaque manque | Q1-3 | Liste des manques du langage | M | Oui |

## 7. Matrice des tests et validations

| Garantie | Validation actuelle | Valeur réelle | Manque | Tâche |
|---|---|---|---|---|
| `check` ⇒ `build` | Conformance 203 cas + 42 gabarits | Corpus fini | Composition, test de puissance | Q0-3 |
| Cohérence checker/backend | Garde debug, 2 tests négatifs | Partielle, 3 exclusions | Exclusions, exécution sous DA | Q1-4 |
| Sûreté du code généré | `forbid`, 2 gardes rouges prouvés | Bonne | — | — |
| Déterminisme | Garde `HashMap`, repro release sur une machine | Bonne localement | Seconde machine | Humain |
| Latence | 10 exemples ≤ 338 lignes | Non représentative | 4 tailles, RAM | Q0-4 |
| Passage à l'échelle | Aucune | — | 1 000 fichiers | Q1-1 |
| Stabilité des identifiants | Édition d'un fichier | Partielle | Ajout et suppression de fichier | Q1-2 |
| Profil `rt` | 10 tests, 5 cas | Règle testée, aucun programme utile | Exemple DSP | Q1-3 |
| Qualité des tests | Mutation 150 tués sur 183 | 24 survivants non lus | Liste et traitement | Q1-6 |
| Honnêteté des chiffres | Relecture manuelle | Fragile | Source unique | Q1-5 |

## 8. Risques résiduels et éléments non vérifiés

- **NON VÉRIFIÉ** : contenu des cinq RFC, journal, tri des mutants, tests de garde de la phase 1, fonction de hash du cache de parse (`loader.rs:250`), traitement en `rt` des appels à des méthodes qui paniquent (`unwrap`, indexation de `Vec`), détail de l'option incrémentale.
- **Aucune exécution pendant l'audit** : les 476 cas et les 1 605 tests n'ont pas été rejoués. Les constats portent sur ce que le code des tests peut prouver, pas sur leur résultat.
- **Erreur du premier audit** : C5 supposait des `NodeId` globaux sur la foi du document de conception. Le code les encodait par fichier. L'agent a eu raison de classer la migration « sans objet » ; il a manqué le plafond et l'ordre de chargement.
- **Expressivité** : toujours ni références stockées, ni traits, ni closures. Q2-5 dira ce que cela coûte en pratique.
- **Décisions humaines** : T4, T12, D9, D10, cinq RFC, `AGENTS.md`. Aucun travail d'agent ne les remplace.

## 9. Prompt GOAL pour un agent moins coûteux

```
GOAL — Ravum : seconde vague de consolidation (suite de l'audit du 2026-11-14)

CONTEXTE ÉTABLI (ne pas re-auditer)
- Dépôt D:\App\Ravum, Windows 11, PowerShell, rustc 1.95, 16 Go de RAM.
- Lis dans l'ordre : AGENTS.md ; docs/audit/2026-11-14-architecture-audit.md ;
  docs/reports/GOAL-PRODUCTION-READINESS-REPORT.md §1 et §2 ;
  docs/operations/sessions/GOAL-JOURNAL.md (dernières entrées).
- La branche goal/production-ready porte 83 commits, PR brouillon #110 vers
  dev. Demande au responsable si elle est intégrée. Si oui, crée
  goal/consolidation-2 depuis dev. Sinon, crée-la depuis goal/production-ready
  et écris-le dans la PR.
- Constats de l'audit, tenus pour acquis :
  1. crates/ravum-lang-codegen/tests/differential_oracle.rs n'est PAS un
     générateur : il concatène 42 gabarits d'instructions indépendantes
     (TEMPLATES, ligne 145). « 40 200 programmes » = environ 42 formes.
  2. Tous les exemples font de 5 à 338 lignes. Aucune mesure à l'échelle.
  3. crates/ravum-lang-sema/src/loader.rs : MAX_SOURCE_FILES = 200, et FileId
     = ordre de chargement (ravum-lang-syntax/src/source.rs:16). NodeId = u32,
     octet haut = fichier (ravum-lang-syntax/src/parser.rs:77).
  4. emit.rs contient toujours MovePlanner ; le garde OwnFacts ne tourne qu'en
     debug et exclut : identité de liaison, récepteurs de méthode, arguments
     de référence.
  5. Le profil rt s'applique à tout le paquet (traps.rs parcourt toutes les
     fonctions), donc examples/dsp-gain-iir ne peut pas compiler en rt.

DÉCISIONS À RESPECTER
- Ne supprime pas MovePlanner (réservé). Ne commence pas la verticale DSP
  (--emit dsp-block). Pas de voie C, pas de traits, closures, async.
- Toute règle de langage nouvelle : RFC dans docs/rfc/, statut PROPOSED,
  marquée PENDING_HUMAN_APPROVAL. Tu peux l'implémenter sur ta branche pour
  la démontrer.
- Pas de HashMap/HashSet dans crates/ravum-lang-*. Builds --release
  reproductibles. Tout Rust généré sans `extern` garde #![forbid(unsafe_code)].
- Pas de nouvelle dépendance externe.

TÂCHES, DANS L'ORDRE

T1 — Corriger les affirmations (Q0-2). Dans le rapport final, la matrice des
  gates et PRODUCTION-READINESS.md : le critère R2 redevient PARTIAL ; chaque
  mention des « 40 200 programmes » précise « 42 gabarits d'instructions
  indépendantes, sans composition ». Aucun autre changement.

T2 — Vrai générateur (Q0-3). Dans differential_oracle.rs, AJOUTE un second
  générateur (garde l'ancien et ses tests) :
  - Env : liste de variables { nom, type, état } avec état ∈ { Init, Moved,
    PartMoved(champs), Uninit }, et pile de portées.
  - Types générés : i32, bool, String, (T, U), [T; 2], [T; 3], Vec<i32>,
    Option<T>, deux structs et deux enums déclarés dans l'en-tête.
  - gen_expr(ty, profondeur) : choisit au hasard parmi les règles qui
    PRODUISENT ty : littéral ; variable vivante de ce type (move si non Copy,
    mettre l'état à jour) ; projection x.f, x.0, x[littéral] d'une variable
    vivante ; appel d'une fonction de l'en-tête ; constructeur ; `if c {…}
    else {…}` ; `match` avec liaisons ; bloc `{ let …; expr }`.
    Profondeur maximale 4 ; à 0, seulement littéral ou variable.
  - gen_stmt : let (avec shadowing possible), affectation, if/else, match,
    while borné, for i in 0..K, bloc imbriqué. Après un if/else ou un match,
    l'état de chaque variable est l'INTERSECTION des branches : déplacée dans
    une branche ⇒ considérée déplacée.
  - 10 % des programmes reçoivent une faute volontaire (usage après move,
    double move, variable non initialisée). Pour eux la propriété est :
    `check` REFUSE avec un code E-OWN-*.
  - Publie : taux d'acceptation, histogramme du nombre de fois où chaque
    règle a été tirée, profondeur moyenne.
  Test de puissance obligatoire : dans une copie de travail jetable
  (git worktree), annule un par un les correctifs des divergences historiques
  (liste dans docs/qualification/T2-FINDINGS.md §22 à §24 et
  DECISIONS_REQUIRED.md D12) et vérifie si le générateur retrouve la
  divergence en 3 × 2 000 programmes. Rapporte le score exact sur 13. Ne
  modifie jamais la branche de travail pour ce test.
  Acceptation : taux d'acceptation des programmes sans faute publié ; 100 %
  des programmes fautifs refusés ; score de puissance publié tel quel, même
  s'il est faible. Toute divergence trouvée : cas réduit dans
  conformance/cases/, entrée dans T2-FINDINGS.md, correction dans un commit
  séparé. Si la spec ne tranche pas : dossier dans DECISIONS_REQUIRED.md.
  INTERDIT : retirer une règle du générateur pour faire disparaître une
  divergence.

T3 — Corpus à l'échelle (Q0-4). Écris tools/gen_scale_corpus.py :
  paramètres --modules M --fns F ; produit un paquet .rv valide où chaque
  module déclare une struct, F fonctions qui s'appellent entre elles et
  appellent le module précédent. Tailles visées : environ 1 000, 5 000,
  20 000, 50 000 lignes (garde M ≤ 150 tant que T4 n'est pas faite). Étends
  scripts/lang-timings.ps1 pour mesurer check, build (avec et sans
  --incremental si l'option existe : vérifie son nom dans `ravum-lang build
  --help`), et la latence LSP de bout en bout, plus la mémoire maximale du
  processus. Publie les courbes dans docs/reports/LANG-TIMINGS.md, valeurs
  défavorables en tête. Ne conclus rien au-delà de ce qui est mesuré.

T4 — Plafond de fichiers (Q1-1). NodeId passe de u32 à u64 : 32 bits hauts =
  FileId, 32 bits bas = index local. Change l'alias dans
  ravum-lang-syntax/src/ast.rs, puis chaque décalage `<< 24`, `>> 24`,
  `& 0xff` dans parser.rs et loader.rs (y compris SYNTHETIC_HIGH et
  next_synthetic), puis les tests qui les lisent. Relève MAX_SOURCE_FILES à
  65 536. Avant et après : génère le Rust de tout examples/ et de tout le
  corpus de conformance, compare octet pour octet. Toute différence = arrêt.
  Tests : paquet de 1 000 fichiers accepté (utilise T3 avec --modules 1000) ;
  paquet au-delà de la limite refusé avec le diagnostic existant.

T5 — FileId stable en session (Q1-2). Dans ravum-lang-lsp, garde pour la
  session une table chemin canonique → FileId qui ne fait que grandir, et
  passe-la au loader (nouveau champ optionnel de LoadOptions). Test : ouvrir
  un paquet de 3 fichiers, en ajouter un quatrième déclaré AVANT les autres,
  vérifier que les NodeId des trois premiers n'ont pas changé et que le cache
  de parse a 3 succès. Hors LSP, comportement inchangé.

T6 — Profil rt par point d'entrée (Q1-3). Amende docs/rfc/RFC-RT-TRAPS.md :
  (a) option CLI `--rt-entry <chemin::de::fonction>` : les règles rt
  (effets interdits ET traps) s'appliquent aux fonctions atteignables depuis
  ce point d'entrée, pas au paquet entier ; `--profile rt` sans --rt-entry
  garde le comportement actuel ;
  (b) indexation bornée : `a[i]` est sûr si `i` est la variable d'un
  `for i in 0..K` avec K littéral ≤ taille connue du tableau, ou
  `0..a.len()` quand `a` est un tableau ou une tranche qui n'est pas
  réaffecté dans le corps de la boucle. Rien d'autre.
  Implémente, avec cas de conformance positifs et négatifs (index calculé,
  variable réaffectée, borne trop grande, point d'entrée inexistant).
  Puis fais compiler examples/dsp-gain-iir avec --rt-entry sur sa fonction
  de traitement, SANS changer sa sortie (le smoke doit rester identique).
  Statut : PENDING_HUMAN_APPROVAL.

T7 — Garde OwnFacts (Q1-4). Étends le garde de l'étape B (emit.rs, autour de
  la ligne 996) aux trois exclusions, une par commit : récepteurs de méthode,
  arguments de référence, identité de liaison. Pour chacune : un test négatif
  qui injecte un fait faux et attend CodegenError. Lance le générateur T2
  avec le garde actif. Chaque désaccord est classé (checker ou backend),
  consigné, corrigé avec un cas. Si une exclusion provoque plus de 10
  désaccords, arrête-la et rapporte.

T8 — Mutants survivants (Q1-6). Liste les 24 survivants du dernier run
  (docs/qualification/, journal). Pour chacun : un test qui le tue, ou une
  justification écrite d'équivalence. Rejoue la campagne par le workflow.

T9 — Source unique des chiffres (Q1-5). Écris scripts/evidence.ps1 qui lance
  les contrôles rapides et écrit evidence/current.json (SHA, date, commande,
  résultat de chaque contrôle). Étends tools/report_consistency.py : il échoue
  si un document de docs/reports/ ou docs/roadmap/ cite, pour l'état courant,
  un nombre différent de ce fichier. Ne réécris pas l'historique des
  documents.

T10 — CI (Q1-7). Ajoute le motif `goal/**` aux déclencheurs de
  .github/workflows/linux-qual.yml.

RÈGLES D'EXÉCUTION
- Test qui échoue d'abord, puis le plus petit changement. Tests positifs ET
  négatifs.
- Avant chaque commit touchant crates/ravum-lang-* :
    cargo fmt --all -- --check
    cargo test -p <crate> -- --test-threads=2
    cargo run -p ravum-conformance -- run --verify-build   (0 FAIL exigé,
    nombre de cas jamais en baisse)
- Fin de tâche : cargo test --workspace --all-targets -- --test-threads=2
- Ne jamais supprimer, ignorer ou affaiblir un test, un cas, un seuil.
- Trois tentatives différentes par échec, puis annule TES commits de la
  tâche, marque BLOCKED avec la sortie brute, passe à la suivante.
- Commits Conventional Commits, atomiques. UN commit documentaire par tâche
  au maximum : ne commite pas « record the green CI run » à chaque poussée.
- Pas de force-push, pas de fusion, pas de tag, pas d'acceptation de RFC.
- Statuts autorisés : TESTED, PARTIAL, NOT_RUN, UNKNOWN, BLOCKED,
  PENDING_HUMAN_APPROVAL. Jamais PROVED ni « production ready ».
- Un nombre de programmes générés n'est pas une preuve : rapporte toujours
  le nombre de FORMES distinctes et le taux d'acceptation avec.
- Arrêt immédiat si : un test préexistant échoue avant tes changements ; la
  spec admet deux lectures ; le Rust généré change sans explication.

RAPPORT FINAL
docs/reports/GOAL-CONSOLIDATION-2-REPORT.md : par tâche, statut, commits,
tests ajoutés, commandes et résultats bruts, SHA. En tête : toute régression
et tout score défavorable (score de puissance de T2, courbes de T3). En fin :
éléments PENDING_HUMAN_APPROVAL, tâches BLOCKED, questions pour le
responsable.
```

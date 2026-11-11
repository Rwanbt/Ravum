# RFC-P2-GATE — Surface expérimentale isolée pour capacités non ratifiées

> **Statut : PROPOSED — non ratifié. RIEN N'EST ACTIVÉ PAR CE DOCUMENT.**
> Ce texte décrit un **contrat d'isolation** qui *permettrait* de prototyper
> `net`/`http`/`sqlite` sans les activer par défaut et sans toucher à l'ensemble
> d'effets stable. Aucun fichier de code n'est modifié, aucun drapeau n'existe
> aujourd'hui.
>
> | Élément | Valeur |
> |---|---|
> | Date | 2026-11-11 |
> | Branche / HEAD | `feat/lang-core-1.0-rc` @ `5d88ecb` |
> | Spécification de référence | `core-1.0-draft.1` |
> | Dépend de | [`RFC-P2-VERTICALS-BLOCKERS.md`](RFC-P2-VERTICALS-BLOCKERS.md), D6/D7 (`DECISIONS_REQUIRED.md`), EFF-1.1, UNS-9.4 |
> | Public visé | responsable (décision), `spec-architecture` (surface normative), agents d'implémentation |

## 1. Le problème que ce contrat résout

[`RFC-PERIMETRE-UNIVERSEL-D6.md`](RFC-PERIMETRE-UNIVERSEL-D6.md) §4 recommande un
« pilote 1.x » : réseau bloquant minimal, testable avec la chaîne de preuve
existante. Ce contrat est le **véhicule d'isolation** de ce pilote : il permet de
l'écrire sans que la surface stable, la conformance stable ou les rapports
puissent le confondre avec une capacité ratifiée.

Aujourd'hui, prototyper `net` exige de modifier l'ensemble d'effets **stable**
(`facts.rs:61`, `is_unsupported()`), la table des builtins publique
(`builtins.rs:126`) et le prélude (`prelude.rs`). Trois conséquences :

1. un prototype devient visible par tous les programmes et par la conformance
   stable (386 cas) ;
2. le retour arrière est manuel et non vérifié ;
3. rien n'empêche un rapport de présenter une capacité expérimentale comme
   stable — ce que UNS-9.4 (« une fonctionnalité **MUST NOT** être activée
   temporairement ou en interne ; le checker est unique ») et INV-01 interdisent.

Le contrat ci-dessous rend l'expérimentation **possible et réversible**, et
surtout **observable** : un artefact expérimental est identifiable comme tel.

## 2. Principes non négociables

| ID | Règle | Niveau |
|---|---|---|
| GATE-1 | Une capacité expérimentale n'est utilisable que si elle est **nommée explicitement** sur la ligne de commande (`--experimental <liste>`). Aucune activation par défaut, par variable d'environnement seule, par fichier de configuration implicite, ni par une dépendance. | MUST |
| GATE-2 | **Fail-closed** : en l'absence du drapeau, les diagnostics sont **exactement** ceux d'aujourd'hui. La sonde `effects(net)` doit continuer à rendre `E-UNSUPPORTED-EFFECT` et `sqlite::open` à rendre `E-RES-UNKNOWN`. | MUST |
| GATE-3 | L'ensemble d'effets **stable** (les 10 groupes de `facts.rs:15`) ne gagne **aucun** variant tant que la capacité n'est pas ratifiée. | MUST |
| GATE-4 | Une capacité expérimentale est **mappée** sur un groupe stable existant pour rester vérifiable (TCP/HTTP → `net`, stockage natif → `fs`) ; elle n'invente pas d'effet. | MUST |
| GATE-5 | Le drapeau **ne réduit aucune vérification** : `E-EFF-UNDECLARED`, `E-EFF-PROFILE`, `E-EFF-UNKNOWN` s'appliquent identiquement. `pure` et `rt` continuent d'interdire l'effet mappé. | MUST |
| GATE-6 | Toute sortie produite avec le drapeau le déclare : bannière dans le fichier généré, champ JSON `"experimental"`, et libellé `EXPERIMENTAL` dans tout rapport. | MUST |
| GATE-7 | Un artefact expérimental **ne peut pas** servir de dépendance à un paquet non expérimental (refus à la résolution). | MUST |
| GATE-8 | Aucun cas de conformance expérimental ne compte dans le total stable ; le corpus est séparé et rapporté séparément. | MUST |
| GATE-9 | Un agent **ne peut pas** promouvoir une capacité : la promotion est une ratification (D2) avec les preuves de §6. | MUST |
| GATE-10 | Retirer le drapeau doit être **testable** : un test vérifie qu'avec le drapeau retiré, le compilateur retrouve exactement le comportement de référence (mêmes codes, mêmes sorties). | MUST |

## 3. Mécanisme proposé (conception, non implémentée)

### 3.1 Forme de la surface

```
ravum-lang check <PATH> [--profile core|pure|rt] [--experimental net[,http,sqlite]]
ravum-lang build <PATH> [--experimental …]
ravum-lang run   <PATH> [--experimental …] [-- ARGS…]
ravum-lang test  <PATH> [--experimental …]
```

- Le drapeau prend une **liste de noms de capacités**, pas des groupes d'effets :
  `net`, `http`, `sqlite`. Un nom inconnu est une **erreur d'usage** (exit `2`,
  comme `--profile hard_rt`, EFF-4.10), jamais un avertissement.
- `ravum-lang doctor --json` expose l'état : `"experimental_available": [...]`,
  et le champ n'est **jamais** vide par défaut.
- Les commandes qui n'ont pas de sens hors compilateur (`fmt`, `doc`, `lsp`)
  **refusent** le drapeau (exit 2) au lieu de l'ignorer : un drapeau silencieusement
  ignoré est un mensonge d'interface.

### 3.2 Portage interne

| Élément | Où (conception) | Contrainte |
|---|---|---|
| Représentation | **nouveau** module `crates/ravum-lang-sema/src/experimental.rs` : `enum ExperimentalCapability { Net, Http, Sqlite }` + `struct Gate(BTreeSet<…>)` | ne touche pas `facts.rs:15` (GATE-3) |
| Passage | `CheckOptions` (`facts.rs:360`) et `GenOptions` (`crates/ravum-lang-codegen/src/lib.rs:20`) gagnent un champ `gate` | défaut = `Gate::empty()` |
| Effets | la table `builtins.rs` fournit **déjà** les effets d'un builtin (`effects.rs:353`) ; les entrées expérimentales déclarent `Effect::Net` | aucun nouveau variant d'effet |
| Refus | `is_unsupported()` devient `is_unsupported(&Gate)`, ou reste vrai et c'est la **résolution** qui consulte le gate avant d'insérer l'entrée | préférer le second : `facts.rs:61` inchangé, décision localisée dans la résolution |
| Builtins | les entrées expérimentales vivent dans une **table séparée** `EXP_BUILTIN_FNS`, consultée seulement si le gate contient la capacité | `BUILTIN_FNS` (table stable) inchangée |
| Codegen | `FreeBuiltin` (`emit.rs`, `enum FreeBuiltin`) gagne des variantes ; elles ne sont atteignables que par les chemins de la table expérimentale | prélude : fonctions `rv_exp_*` dans une constante séparée, pour que la taille du prélude stable ne change pas |
| Identité de build | le fichier généré commence par `// EXPERIMENTAL: net, http` et/ou `// EXPERIMENTAL: none` | mesurable par `--emit-rust` |

### 3.3 Alternative écartée

**Option B : activer `Effect::Net` en changeant `is_unsupported()` selon le
drapeau.** Écartée parce que la réponse du checker dépendrait d'un état global
invisible à la lecture de `facts.rs`, et parce qu'un `Effect::Net` produit dans
`facts.effects` serait indistinguable d'un `net` stable. L'option A (module et
table séparés) garde la frontière **structurelle**, donc auditable par diff.

### 3.4 Matrice fail-closed

| Situation | Comportement attendu | Statut aujourd'hui |
|---|---|---|
| `check` sans `--experimental` | identique à `5d88ecb` : `E-UNSUPPORTED-EFFECT` / `E-RES-UNKNOWN` | **mesuré** (sondes #1–#6 du dossier verticales) |
| `--experimental net` mais `net` non prototypé | `E-RES-UNKNOWN` (l'API n'existe pas) — le drapeau seul n'ajoute rien | n'existe pas |
| `--experimental http` sans `net` | erreur d'usage (exit 2) : dépendance de capacité manquante | n'existe pas |
| `--experimental bogus` | erreur d'usage (exit 2) | n'existe pas |
| `--experimental net` + `--profile pure` | `E-EFF-PROFILE` (l'effet mappé `net` reste interdit) | cohérent avec EFF-4.2 |
| `--experimental net` + `--profile rt` | `E-EFF-PROFILE` | cohérent avec EFF-4.7 |
| `--experimental net` + `-C dylib`/lib | refusé : le gate est interdit pour un artefact destiné à être dépendance (GATE-7) | à implémenter |

## 4. Enregistrement dans les registres de gates

Le dépôt a deux registres et **aucun des deux n'a d'état `EXPERIMENTAL`** :

1. `spec/language/features.md` a une colonne `Statut` à trois valeurs :
   `candidate-stable | experimental | unsupported` (FEAT-1.x). C'est **le seul
   endroit** où `experimental` existe déjà : **8 lignes** au 2026-11-11
   (`F-SYN-004`, `F-SYN-022`, `F-TYP-020`, `F-EXPR-018`, `F-EFF-006`,
   `F-STD-010`, `F-STD-031`, `F-META-006`). À noter : FEAT-12.5 n'en cite que
   **6** — `F-STD-031` et `F-META-006` manquent à sa liste, écart de rédaction à
   corriger par `spec-architecture`.
2. `docs/roadmap/CORE-1.0-GATE-MATRIX.md` n'admet que
   `PASS | PARTIAL | FAIL | BLOCKED | NOT_RUN`.

Règle d'enregistrement proposée (à ratifier avec la surface) :

| ID | Règle | Niveau |
|---|---|---|
| GATE-11 | Une capacité expérimentale obtient **un `FeatureId`** dans `features.md` avec `Statut = experimental`, une référence à ce RFC, et la mention explicite de son gate (`--experimental <nom>`). | MUST |
| GATE-12 | La matrice de gates ne reçoit **pas** de ligne « capacité expérimentale » : elle reçoit une ligne pour le **gate** (« surface expérimentale », état `PASS` si les cas passent **et** si la matrice fail-closed de §3.4 est testée). Un `PASS` de gate ne dit rien de la capacité. | MUST |
| GATE-13 | Le rapport de conformance sépare trois compteurs : stable (`conformance/cases/`), expérimental (`conformance/cases-experimental/`), et total. Aucun total mélangé n'est publié. | MUST |
| GATE-14 | `tools/features_coverage.py --strict` continue d'exiger 100 % de couverture **stable** ; les lignes expérimentales sont comptées à part et ne peuvent pas satisfaire une règle stable. | MUST |
| GATE-15 | Tout document, rapport ou README qui cite la capacité doit porter `EXPERIMENTAL` et le nom du gate, à côté de la mention, pas dans une note de bas de page. | MUST |

Le point 2 est le plus important : ne pas créer un faux état `EXPERIMENTAL` dans
la matrice. Une capacité expérimentale n'est **pas** un gate ; c'est une ligne de
`features.md` avec un statut non stable, et le gate porte sur l'**isolation**.

## 5. Ce que la surface expérimentale n'autorise pas

- ❌ activer quoi que ce soit par défaut, même « temporairement » (UNS-9.4) ;
- ❌ utiliser une capacité expérimentale depuis un paquet stable (GATE-7) ;
- ❌ produire un binaire de release présenté comme stable (GATE-6) ;
- ❌ compter un cas expérimental dans un total de conformance (GATE-13) ;
- ❌ élargir les types FFI (pointeurs/tampons) sous prétexte d'expérimentation :
  c'est D7, pas un gate (voir BL-8) ;
- ❌ modifier `spec/language/effects.md` pour décrire un groupe comme « supporté »
  tant que la ratification n'a pas eu lieu.

## 6. Preuves exigées pour promouvoir une capacité

Une promotion (`experimental → candidate-stable`) est une **ratification**
(D2), jamais une décision d'agent (GATE-9). Dossier minimal :

| # | Preuve | Vérifiable par |
|---|---|---|
| P1 | Décision humaine explicite (D6 pour le périmètre, D7 pour FFI/lien si concerné) | `DECISIONS_REQUIRED.md` mis à jour, ADR/RFC accepté |
| P2 | Cas positifs **et** négatifs dans la conformance, exécutés | `ravum-conformance run` sur le corpus expérimental, sortie liée à un SHA |
| P3 | Inférence d'effets prouvée : transitivité, point fixe, récursion, `E-EFF-UNDECLARED` | tests unitaires de `effects.rs` + cas de conformance |
| P4 | Profils : `pure` et `rt` refusent toujours la capacité | cas `E-EFF-PROFILE` |
| P5 | Isolation : matrice fail-closed de §3.4 entièrement testée, y compris le retrait du drapeau (GATE-10) | test d'isolation dédié |
| P6 | Tests hermétiques : loopback + port éphémère pour réseau ; base temporaire pour stockage ; aucun accès réseau externe | conception de §4.4 du dossier verticales |
| P7 | Sûreté : aucun `unsafe` ajouté au prélude ; si la capacité exige du natif, audit de la frontière (D7) | `prelude.rs` (test `prelude_is_embedded_and_safe`) + audit |
| P8 | Honnêteté allocation/blocage : la documentation dit ce que la capacité alloue et ce qu'elle bloque ; aucune promesse RT | doc + mesure (allocateur compteur, `ravum-dsp-harness`) |
| P9 | Plateformes : Windows **et** Linux (le dépôt a une qualification Linux par CI) | `docs/qualification/LINUX-QUALIFICATION.md` |
| P10 | Retrait / rollback : procédure écrite et testée pour désactiver la capacité | test + documentation |

Tant que P1–P10 ne sont pas tous satisfaits, la ligne reste `experimental` et
l'artefact reste marqué.

## 7. Coût de mise en place (surface comptée, pas un délai)

| Élément | Fichiers | Ordre de grandeur |
|---|---|---|
| Module `experimental.rs` (types + parse du drapeau) | 1 nouveau | ≈ 150–250 lignes |
| Câblage des options | `facts.rs` (`CheckOptions`), `codegen/lib.rs` (`GenOptions`), `pipeline.rs` (`analyze`/`compile_program`) | 3 fichiers, ≈ 30 lignes |
| Table de builtins séparée + résolution | `builtins.rs`, `resolve.rs` | 2 fichiers, ≈ 80–150 lignes |
| Émission + prélude séparé | `emit.rs`, `prelude.rs` | 2 fichiers, ≈ 100–200 lignes |
| Bannière + JSON + `doctor` | `emit.rs`, `pipeline.rs`, `commands/doctor.rs`, `args.rs` | 4 fichiers, ≈ 60 lignes |
| Tests d'isolation (fail-closed + retrait) | 1 nouveau fichier de test + cas de conformance séparés | ≈ 10–15 cas |

Total : **≈ 500–800 lignes** et un corpus séparé. C'est **plus cher** que
d'activer `net` directement — et c'est le but : le coût de l'isolation est le prix
de la réversibilité, payé une fois, avant toute capacité future.

## 8. État actuel : rien n'est activé

- `--experimental` **n'existe pas** : `crates/ravum-lang-cli/src/args.rs` ne
  contient **aucune** occurrence du mot, et
  `ravum-lang check examples/hello --experimental net` répond
  « unknown option `--experimental` for `check` » avec le code de sortie **2**
  (mesuré le 2026-11-11).
- `experimental.rs` **n'existe pas**.
- `net` reste `E-UNSUPPORTED-EFFECT`, `http`/`sqlite`/`time` restent
  `E-RES-UNKNOWN` — mesuré le 2026-11-11, voir le dossier verticales §1.
- Aucune ligne de `features.md` n'a été ajoutée par ce document.

## 9. Questions ouvertes

- **Q-GATE-1** — Le gate doit-il être un drapeau CLI, ou un **profil** au sens de
  `Profile` (`core|pure|rt`) ? Recommandation : un drapeau. Un profil est une
  *vérification* (EFF-4.9 : « le profil ne change jamais la sémantique ») ; le
  gate change la **surface acceptée**, ce qui est d'une autre nature et ne doit
  pas être mélangé dans la même énumération.
- **Q-GATE-2** — Faut-il un gate par capacité (`net`, `http`, `sqlite`) ou un
  seul gate `experimental` global ? Recommandation : par capacité — un gate
  global rend impossible de savoir ce qui a été activé en relisant la commande.
- **Q-GATE-3** — Une capacité expérimentale peut-elle être utilisée dans un
  `test fn` d'un paquet stable ? Recommandation : non (GATE-7 s'applique aux
  tests aussi), sinon la conformance stable dépend d'un état expérimental.

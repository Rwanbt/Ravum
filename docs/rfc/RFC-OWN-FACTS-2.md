# RFC-OWN-FACTS-2 — supprimer la re-dérivation d'ownership dans le backend

- **Statut** : `ACCEPTED` **sous délégation** (plan `PRODUCTION-PLAN-2026-11-15`,
  décision **P-7**, 2026-11-15).
- **Implémentation** : **C1 à C5 faites**. C1 (`copy_type`) : PR #134, merge
  `b7b534e`. C2–C5 : voir le §6 (mesures, écarts trouvés, limites). C1 avait
  immédiatement révélé une divergence réelle : `sema::own::is_copy_type` classait
  `Slice(I32)` comme `Copy` alors qu'OWN-2.1 ne l'y met pas et qu'OWN-2.2 rend
  mobile tout le reste ; le backend avait raison, le classifieur a été corrigé et
  un test le verrouille.
- **Motif** : deux implémentations de la même connaissance (le checker et le
  backend) sont la cause de toutes les divergences `check → build` trouvées
  depuis la passe 6. Tant qu'elles existent, une divergence est possible par
  construction.
- **Portée** : `ravum-lang-sema` (émet les faits) et `ravum-lang-codegen`
  (cesse de les recalculer). Aucune règle de spécification n'est modifiée.

## 1. État actuel

`OwnFacts` (RFC-OWN-FACTS-1, étape A) publie trois tables :

| Champ | Contenu | Consommateur |
|---|---|---|
| `use_kind: BTreeMap<NodeId, UseKind>` | move / copy / borrow par expression utilisée | étape B (garde de cohérence) |
| `place: BTreeMap<NodeId, OwnPlace>` | liaison + projections par expression-place | étape B |
| `element_reinit: BTreeSet<NodeId>` | cibles d'affectation qui ré-initialisent un élément de tableau | `emit::paths` |

Le backend **re-dérive** encore trois décisions :

| Site | Ce qu'il décide | Pourquoi c'est un risque |
|---|---|---|
| `Emitter::is_copy_type` (`emitter_items.rs`) | un type est-il `Copy` ? | miroir de `sema::own::is_copy_type` ; un désaccord change un move en copie |
| `Emitter::is_owned_position` | l'expression est-elle en position possédée ? | règle syntaxique dupliquée, indépendante du checker |
| `Emitter::consumed_by_checker` | l'expression a-t-elle été consommée ? | re-déduit ce que le checker a déjà décidé |

L'étape B (`verify_ownership_facts`) compare ces décisions **en build debug**
seulement : c'est un garde-fou de test, pas une source de vérité.

## 2. Extension de schéma proposée

Quatre tables supplémentaires, chacune répondant à un site de re-dérivation :

### 2.1 `copy_type: BTreeSet<NodeId>`

- **Clé** : identifiant du *nœud de type résolu* (`Facts::type_nodes`), pas du
  type structurel.
- **Sens** : le type désigné par ce nœud est `Copy` au sens du checker.
- **Remplace** : `Emitter::is_copy_type`.
- **Invariant** : pour tout nœud de type présent dans `type_nodes`, l'appartenance
  à `copy_type` est décidée une seule fois, par le checker.
- **Pourquoi par nœud** : le backend doit décider sur des types **sans** nœud
  (types de paramètres synthétiques, types de retour inférés). Ces cas sont
  couverts par `type_nodes` quand ils existent ; sinon la table porte une entrée
  par identifiant de type **de syntaxe** effectivement visité, et le backend
  n'invente rien : il consulte ou échoue explicitement (`E-CODEGEN-FACT-MISSING`).

### 2.2 `owned_position: BTreeSet<NodeId>`

- **Clé** : nœud d'expression.
- **Sens** : cette expression est utilisée en position possédée (déplacement
  possible), décision du checker.
- **Remplace** : `Emitter::is_owned_position`.
- **Invariant** : `owned_position` est cohérent avec `use_kind` — une expression
  en `UseKind::Move` est en position possédée ; l'inverse n'est pas vrai (une
  expression possédée peut être copiée).

### 2.3 `consumed: BTreeSet<NodeId>`

- **Clé** : nœud d'expression.
- **Sens** : le checker a consommé cette expression (déplacement effectif ou
  transfert à un appel).
- **Remplace** : `Emitter::consumed_by_checker`.
- **Propagation exigée** (P-7) : la décision est propagée **à travers `match`,
  `if` et bloc**, c'est-à-dire qu'un nœud consommé dans un bras de `match` ou une
  branche de `if` est marqué consommé au même titre qu'un nœud consommé dans le
  flux direct. Sans cette propagation, le backend devrait re-déduire la règle, ce
  qui est exactement ce que cette RFC supprime.

### 2.4 `binding_identity: BTreeMap<NodeId, BindingId>`

- **Clé** : nœud d'expression-path qui référence une liaison.
- **Sens** : identité **unique** de la liaison visée (`BindingId` opaque, stable
  par programme).
- **Remplace** : la re-dérivation du nom de liaison par projection textuelle
  (`emit::names` + comparaison de chemins), qui se trompe sur le shadowing
  imbriqué.
- **Invariant** (P-7) : **une identité de liaison unique par paramètre** ; deux
  paramètres homonymes de fonctions différentes ont des identités différentes,
  et un shadowing crée une nouvelle identité — jamais une réutilisation.

## 3. Ordre de migration

Chaque étape est indépendante, vérifiable, et **ne supprime un site de
re-dérivation qu'après** avoir prouvé l'équivalence sur le corpus :

1. **Étape C1 — `copy_type` (FAITE, 2026-11-15).** Émettre la table, l'utiliser dans `Emitter::is_copy_type`
   en repli (fait d'abord, puis re-dérivation supprimée), étendre l'étape B pour
   exiger l'égalité sur tout le corpus.
2. **Étape C2 — `owned_position` (FAITE).** Même schéma pour `is_owned_position`.
3. **Étape C3 — `consumed` (FAITE)** avec propagation `match`/`if`/bloc ;
   `consumed_by_checker` supprimée.
4. **Étape C4 — `binding_identity` (FAITE)** ; la comparaison de spans de
   déclaration est supprimée du planificateur.
5. **Étape C5 — retrait du miroir (FAITE)** : `sema::own::is_copy_type` reste la
   seule implémentation ; le backend n'a plus de fonction de classification.

## 4. Vérification exigée

Une étape n'est « faite » que si les cinq preuves suivantes existent :

| Preuve | Commande / artefact |
|---|---|
| Tests du crate | `cargo test -p ravum-lang-sema -p ravum-lang-codegen` |
| Garde de cohérence étape B, **non vide** | elle doit échouer si l'on injecte un désaccord (test négatif existant, à étendre par table) |
| Comportement identique | **S7** : Rust généré identique octet pour octet sur les exemples |
| Contrat `check ⇒ build` | `ravum-conformance run --verify-build`, 0 FAIL |
| Oracle différentiel | 0 divergence sur la campagne rapide (la nominale reste E7) |

## 5. Ce que cette RFC ne fait pas

- Elle **ne change aucune règle** de `spec/language/ownership.md` ; elle déplace
  la connaissance, pas la sémantique.
- Elle **ne promeut aucun gate** : `T2` reste `PARTIAL` (dénominateur MUST, D10)
  et `T4` reste `BLOCKED` (audit externe).
- Elle **n'autorise aucune suppression de test** : les gardes de l'étape B
  deviennent plus stricts, jamais plus lâches.
- L'acceptation est **déléguée** par P-7 ; la ratification humaine des ADR
  (D2) reste ouverte et distincte.

## 6. Mise en œuvre de C2 à C5 (2026-11-15)

Mesures locales, liées à la machine et au commit cités : conteneur Linux
x86_64, `rustc 1.95.0`. La CI de la PR rejoue les mêmes suites sur trois OS ;
ses résultats font foi pour les autres plateformes. Aucun gate n'est promu.

### 6.1 Deux commits, dans l'ordre exigé par le §3

| Commit | Contenu | Rôle |
|---|---|---|
| **A** `99d3e3a` | Le checker publie `owned_position`, `consumed`, `binding_identity` (+ `bindings`). Le backend **dérive encore** ses décisions ; le garde de l'étape B compare chaque décision dérivée au fait publié. | Preuve d'équivalence, avant tout retrait |
| **B** (commit suivant de la PR) | Le planificateur lit `consumed` et `binding_identity`. `Emitter::is_copy_type`, `Emitter::is_owned_position`, `Emitter::consumed_by_checker`, `pattern_consumes_value` et le paramètre `owned` propagé par `walk_expr`/`walk_block` sont **supprimés**. | Retrait de la re-dérivation |

### 6.2 Où les faits sont décidés

| Fait | Site unique dans `sema::own` | Remarque |
|---|---|---|
| `owned_position`, `consumed` | `consume_place` (valeur d'une place) et `consume_temporary` (projection d'un temporaire) | Un bras de `match`, une branche de `if`, une queue de bloc ou un corps de boucle passent par `eval_value` comme le flux direct : la propagation exigée par P-7 n'est pas une règle à part, c'est le parcours du checker |
| `binding_identity` | `eval_place_expr`, à la résolution d'un chemin local par les portées | `BindingId` interné par (span de déclaration, nom) dans `OwnFacts::bindings` |
| code inatteignable | `analyze_dead` | voir 6.3, écart 2 |

### 6.3 Écarts réels trouvés pendant la migration

1. **Projection d'un temporaire sans aucun fait** (`make()[i]`, `pair().0`).
   Le checker ne publiait rien pour une racine qui n'est pas une place ; le
   backend, lui, abaissait le déplacement. Trouvé par la comparaison S7 au
   premier essai de retrait : `run/array-move-descent` cessait de se construire
   (`rustc` `E0508`). **Le garde du commit A, dans sa première version, excluait
   ces chemins** (il ne comparait que les places) : il passait donc sans les
   avoir vérifiés. Corrigé dans A — `consume_temporary` publie la décision, le
   garde compare tous les chemins — puis seulement le retrait a été rejoué.
   C'est la famille `descents`, `BLOCKED` dans RFC-OWN-FACTS-1 §9.2.
2. **Code inatteignable sans aucun fait.** Après `return`/`break`/`continue`,
   le checker n'analysait plus rien, alors que le backend émet ce code. Le
   checker l'analyse maintenant en silence sur une copie de l'état
   (`analyze_dead`) : aucun diagnostic, aucun effet sur l'état atteignable.
   **Mesure honnête** : `rustc 1.95.0` accepte un `a[0]` nu après `return`
   (essai direct) ; ce point n'évite donc pas un échec de build, il rend
   l'abaissement indépendant de l'atteignabilité et permet au garde d'exiger une
   décision pour **chaque** chemin abaissé.
3. **Deux paramètres d'une même fonction avaient la même identité** (le span de
   l'item). Conséquence mesurée : dans `fn f(a: [String; 2], b: [String; 2])`,
   déplacer `a[0]` faisait traiter `b` comme partiellement déplacé, et `b[1].len()`
   était abaissé par motif (`(*{ let [_, ref __rv_el] = b; __rv_el }).len()`)
   au lieu de `b[1usize].len()`. Sans conséquence observable à l'exécution, mais
   c'est exactement l'erreur que C4 devait rendre impossible. **C'est le seul
   changement de Rust généré de cette étape**, et il ne touche aucun fichier du
   corpus S7 (0 fichier différent sur 264). Famille `field-projection` de
   RFC-OWN-FACTS-1 §9.2.
4. **La position « possédée » dérivée par le backend était une
   sous-approximation.** Le checker prend par valeur des opérandes `Copy` que le
   parcours syntaxique ne suivait pas (`a[0] + 1`). Sonde temporaire (hors
   dépôt) : 25 occurrences sur le corpus S7, 1 803 sur les suites de tests et
   l'oracle rapide ; **toutes de type `Copy`, aucune ne change `consumed`**.
   L'égalité n'est donc exigée que sur `consumed` ; pour `owned_position` le
   garde A exigeait l'inclusion.
5. **Les fixtures de codegen assemblées à la main ne publiaient aucun fait
   d'ownership.** Elles exécutent maintenant le vrai checker
   (`check_ownership`), comme la production. Trois inexactitudes de fixtures
   sont apparues et ont été corrigées dans les fixtures, pas dans le garde :
   `self` typé par valeur dans une méthode `&self`, résolution de `self`
   absente, type des liaisons `let` absent.

### 6.4 Ce que le garde de l'étape B vérifie désormais (builds debug)

- cohérence des tables entre elles : `consumed` = exactement les `UseKind::Move` ;
  `consumed ⊆ owned_position` ; une position possédée est un `Move` ou un `Copy` ;
- `use_kind` et `copy_type` contre l'unique classifieur (`ravum_lang_sema::is_copy_type`)
  appliqué aux types enregistrés : une table périmée ou assemblée à la main est
  refusée ;
- chaque `binding_identity` désigne la liaison que la résolution donne
  (span de déclaration **et** nom) ;
- **complétude** : tout chemin que le backend abaisse a une décision du checker
  (une place et une identité de racine, ou un usage pour un temporaire) ;
- les places enregistrées contre la syntaxe (inchangé).

Non-vacuité : `crates/ravum-lang-codegen/src/emit/tests/own_facts_guard.rs`
corrompt une table à la fois sur un programme réellement vérifié et exige le
refus (mouvement absent de `consumed`, copie présente dans `consumed`, mouvement
hors position possédée, emprunt en position possédée, chemin sans aucun fait,
temporaire sans aucun fait, identité fausse, identité non déclarée, identité
absente). Les tests de non-vacuité antérieurs sont conservés tels quels.

### 6.5 Preuves (§4), au commit B

| Preuve | Commande | Résultat |
|---|---|---|
| Tests des deux crates | `cargo test -p ravum-lang-sema -p ravum-lang-codegen` | voir le tableau de la PR (chiffres du run final) |
| Garde non vide | tests `own_facts_guard` ci-dessus | chaque corruption est refusée |
| S7 | `tools/s7_snapshot.sh <binaire> <dossier>` sur `dev` (`3c7adb2`), A et B | **522 cibles, 264 fichiers Rust émis, identiques octet pour octet** ; même liste de cibles construites ; sha256 du concaténé `f125ab3b97b4ba8c9b68b653df22d49e827c731676122dbb53b327035d2b0e8b` aux trois points |
| `check ⇒ build` | `ravum-conformance run --verify-build` | **503 cas : 501 PASS / 0 FAIL / 2 SKIP / 0 NOT_RUN** sous Linux ; `perimeter=222 covered=222 verified=124 implicit=98 failed=0` ; `build_verified: true` (A et B) |
| Oracle différentiel | `differential_oracle_fast`, `typed_oracle_fast` (dans la suite codegen) | 0 divergence ; la campagne nominale reste E7 |

Les 2 `SKIP` sont ceux de la plateforme Linux ; la mesure de référence Windows
(502 PASS / 1 SKIP) n'a pas été rejouée ici et n'est pas modifiée.

### 6.6 Limites et dette déclarée

- **`owned_position` n'a pas de consommateur dans le backend.** La décision
  d'abaissement ne dépend que de `consumed` ; `owned_position` ne sert qu'au
  garde (cohérence) et aux tests. La table est publiée parce que cette RFC la
  spécifie ; si aucun consommateur n'apparaît (analyse d'effets, LSP), la
  retirer est un changement local à `consume_place`/`consume_temporary`.
- **`copy_type` non plus** depuis C5 : le seul appelant de `Emitter::is_copy_type`
  était `is_owned_position`. Elle reste vérifiée par le garde.
- **Ce qui reste syntaxique dans le planificateur, et n'est pas une décision
  d'ownership** : trouver les chemins, lire la valeur d'un index littéral
  (`constant_index`), et le drapeau `exclusive` (cible d'affectation ou `&mut`
  explicite, complété par `UseKind::BorrowMut`). La famille `literal-index` de
  RFC-OWN-FACTS-1 n'est pas migrée : c'est une lecture de la syntaxe, pas une
  seconde implémentation d'une règle.
- **`OwnPlace::binding` reste un span** : deux paramètres y sont toujours
  indiscernables. Le backend ne s'en sert plus pour identifier une liaison ; le
  garde, si (comparaison place enregistrée / place syntaxique).
- **Aucun cas de conformance ajouté** : le corpus a des compteurs publiés dans
  une douzaine de documents et dans `evidence/current.json`, régénéré par
  `scripts/evidence.ps1` sur la machine de référence Windows, indisponible ici.
  Les comportements nouveaux sont verrouillés par des tests Rust (dont un test
  de bout en bout compilé et exécuté). Ajouter les cas au corpus est à faire
  sur la machine de référence.
- Le garde ne tourne qu'en build debug : ce n'est pas une preuve, c'est un
  garde-fou de test. `T2` reste `PARTIAL`, `T4` reste `BLOCKED`.

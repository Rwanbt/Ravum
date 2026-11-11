# Traçabilité des MUST — deux métriques, deux dénominateurs

> **Statut : mesure, pas certificat.** Date : 2026-11-11, SHA `5d88ecb`,
> Windows 11 x86-64, Python 3.13.7. Ce document mesure une affirmation qui
> circule dans les dossiers : « 188 FeatureIds » n'est pas « 188 règles MUST ».
> Il ne modifie aucun seuil, aucun gate, aucune spécification.

## 1. Résultat en une phrase

**Confirmé, et l'écart est grand :** `spec/language/` contient **807 énoncés
MUST** (805 identifiants uniques), tandis que `features.md` contient **188
FeatureIds**. **463 / 807 MUST (57,4 %)** sont atteignables par un FeatureId
déclaré ; **344 / 807 (42,6 %)** ne le sont pas. La couverture `188/188` publiée
par le gate T2 reste vraie — mais elle porte sur les FeatureIds, **pas** sur les
MUST.

## 2. Deux métriques, deux oracles

| | Métrique 1 — FeatureId | Métrique 2 — énoncé MUST |
|---|---|---|
| Question | chaque FeatureId cite-t-il un cas existant qui satisfait le critère strict ? | chaque énoncé MUST est-il atteignable par un FeatureId, et par un cas ? |
| Dénominateur | **188** FeatureIds (`features.md`) | **807** lignes de tableau `| ID | … | Niveau |` avec `Niveau = MUST` dans `spec/language/*.md` |
| Oracle | `tools/features_coverage.py --strict` (existant, non modifié) | `tools/must_audit.py` (nouveau, ce document) |
| Résultat | **188/188** satisfaits (187 par cas + 1 exemption `F-META-006`), exit 0 | **463/807** atteignables (dont **463/807** avec un cas existant), **344/807** non atteignables |
| Ce que ça ne prouve pas | que le cas exerce réellement la règle (lien déclaré par la spec) | idem ; et un lien de groupe (`TYP-11`) est plus grossier qu'un lien de règle |

Les deux oracles partagent **la même bibliothèque** pour la métrique 1 :
`must_audit.py` importe `features_coverage.py` et réutilise `parse_rows`,
`classify`, `load_exemptions`, `validate_exemptions` et `strict_check`. Il n'y a
donc pas deux définitions concurrentes de « FeatureId couvert ».

## 3. Métrique 1 — couverture des FeatureIds (rappel mesuré)

```
$ python tools/features_coverage.py --strict
rules            : 188
covered          : 187
exempt           : 1
uncovered        : 0
dangling         : 0
strict traceability: 188/188 rules satisfied (1 justified exemption)
EXIT=0
```

C'est la mesure qui fonde le `PASS` de T2. Elle est reproduite à l'identique par
`must_audit.py` (même résultat, même code). **Elle ne dit rien sur les 807 MUST.**

## 4. Métrique 2 — traçabilité des énoncés MUST

### 4.1 Dénominateur exact

- **807** lignes de règles de niveau `MUST` dans les tableaux de
  `spec/language/*.md` ;
- **5** lignes de niveau `SHOULD`, **exclues** du dénominateur (aucune n'est une
  obligation) ;
- **19** lignes formulent explicitement « MUST NOT » (les autres négations sont
  exprimées en français : « aucun », « jamais », « ne … pas ») ;
- **805** identifiants uniques pour 807 lignes : `G-5.11` et `G-6.40` sont
  **dupliqués** (deux règles distinctes portent le même identifiant). Voir §6.3.

### 4.2 Résultat global

```
$ python tools/must_audit.py
MUST statements : 807 (denominator: rule-table rows with level MUST)
SHOULD rows     : 5 (excluded from the denominator)
explicit MUST NOT wording: 19

reached through a rule id    : 110
reached through a rule range : 72
reached through a minor group: 328
reached through a section    : 40
UNMAPPED (no FeatureId)      : 344

with an existing case        : 463/807
strict-satisfied through one : 463/807
EXIT=0
```

Lecture : 463 énoncés (57,4 %) sont atteints par au moins un FeatureId ; 344
(42,6 %) ne le sont par aucun. « Atteint » n'est pas « prouvé » : le lien est
**déclaré** par la colonne `Syntaxe`/`Typage`/`Runtime` de `features.md`, et
vérifié ici pour l'existence du FeatureId, de son cas et de son verdict strict.
Aucun cas ne peut déclarer la règle qu'il couvre (le harnais rejette les champs
inconnus de `case.json`) : la traçabilité est unidirectionnelle par construction.

Les quatre colonnes de liaison se recouvrent (un même énoncé peut être atteint
par un identifiant exact **et** par une section). Répartition par lien le plus
fort :

| Lien le plus fort | Énoncés |
|---|---|
| identifiant de règle exact (`G-3.1`) | 110 |
| plage de règles (`G-4.1–4.2`) | 43 |
| groupe de section (`TYP-11` → `TYP-11.x`) | **273** |
| section de document (`lexical §8.1`) | 37 |
| **aucun** | **344** |

**Le lien de groupe est le plus fréquent (273).** Il est légitime mais grossier :
un FeatureId qui cite `TYP-11` revendique **toutes** les règles `TYP-11.x`, sans
les nommer une par une. Une réduction au lien de règle exact (110 aujourd'hui)
serait un travail d'écriture de spec, pas une mesure.

### 4.3 Par document

| Document | MUST | atteints (tous liens) | dont **non atteints** |
|---|---|---|---|
| `abi.md` | 33 | 0 | **33** |
| `compatibility.md` | 39 | 0 | **39** |
| `concurrency.md` | 20 | 0 | **20** |
| `core-machine.md` | 40 | 0 | **40** |
| `diagnostics.md` | 43 | 0 | **43** |
| `effects.md` | 44 | 29 | 15 |
| `expressions.md` | 85 | 81 | 4 |
| `features.md` | 11 | 0 | **11** |
| `grammar.md` | 92 | 58 | 34 |
| `lexical.md` | 70 | 27 | 43 |
| `modules.md` | 67 | 46 | 21 |
| `ownership.md` | 71 | 60 | 11 |
| `stdlib.md` | 91 | 75 | 16 |
| `types.md` | 93 | 87 | 6 |
| `unsupported.md` | 8 | 0 | **8** |

Deux familles très différentes :

1. **Documents entiers sans FeatureId** — ABI, COMPAT, CONC, MACH, DIAG, FEAT,
   UNS : **194 énoncés MUST** (33+39+20+40+43+11+8) qu'aucun FeatureId ne
   revendique. Ce n'est pas nécessairement un défaut : ces documents décrivent
   des **contrats de processus** (ABI, compatibilité, diagnostic, machine
   abstraite), pas des fonctionnalités du langage. Mais alors il faut le dire :
   `features.md` ne prétend pas les couvrir, et T2 ne les mesure pas.
2. **Documents de fonctionnalités avec des trous** — grammar (34),
   lexical (43), modules (21), stdlib (16), ownership (11), effects (15) : ces
   règles **sont** censées être couvertes par un FeatureId, et ne le sont pas.
   C'est là que la métrique 2 révèle un déficit réel, indépendant de l'exemption
   `F-META-006`.

### 4.4 Exemples vérifiables

| Énoncé | Document | Pourquoi non atteint |
|---|---|---|
| `MACH-1.4` (état `NOT_IMPLEMENTED`, aucun résultat citable) | `core-machine.md` | aucun FeatureId ; c'est la règle qui fonde l'exemption `F-META-006` |
| `ABI-2.5` (aucun `unsafe` généré) | `abi.md` | aucun FeatureId — et **contredit** par le wrapper FFI généré (voir RFC-D7 §3) |
| `DIAG-1.1` (tout diagnostic porte un code de §3) | `diagnostics.md` | aucun FeatureId ; pourtant `registry_audit.py` le vérifie par un autre chemin |
| `LEX-8.30` (tout échappement absent de la table est refusé) | `lexical.md` | couvert seulement par une **section** (`lexical §8.4` dans F-LEX-008), pas par un lien de règle |
| `G-6.40` (deux règles distinctes sous le même identifiant) | `grammar.md` | ambiguïté d'identifiant : impossible d'écrire une liaison non ambiguë |

## 5. Ce que ces chiffres ne disent pas (limites explicites)

1. **Un lien déclaré n'est pas un test exécuté.** `features_coverage.py` vérifie
   l'existence du cas, son mode et l'accord sur les codes ; il ne prouve pas que
   le cas exerce l'énoncé. La spec le dit déjà pour la métrique 1 ; c'est vrai
   *a fortiori* pour un lien de groupe.
2. **Un énoncé non atteint n'est pas forcément un défaut.** ABI/COMPAT/MACH
   décrivent des contrats de processus ; ils ne sont pas « des fonctionnalités à
   tester » au sens de `features.md`. La confusion dangereuse serait de lire
   `188/188` comme « toutes les MUST sont traçables ».
3. **Les énoncés d'un document ne sont pas des programmes.** Une règle comme
   `DIAG-5.3` (comptage `CRLF`) est testable par un cas, mais aucune FeatureId
   ne la cite : le déficit est documentaire, pas nécessairement technique.
4. **Le décompte de 807 dépend du format des tableaux.** Une règle écrite en
   prose hors tableau n'est pas comptée ; `must_audit.py` refuse de « deviner ».
   Un changement de format est détecté par le test `RealRepositoryTests`
   (dénominateur borné 800–900).
5. **Cinq lignes de règles contiennent un `|` non échappé** (dans un code
   inline : `||`, `|=`, `|x|`, `p | q`). Elles sont **comptées** et signalées
   par l'outil (`problems`), leur texte peut être tronqué à l'affichage — pas le
   décompte.
6. **Aucun seuil n'est modifié.** `must_audit.py` est **report-only** (exit 0) ;
   son mode `--strict` (exit 1 si un MUST n'est pas atteint) **n'est pas branché
   en CI**. Il n'ajoute donc aucun gate ; il rend le débat mesurable.

## 6. Contradictions et défauts trouvés en écrivant l'outil (à porter au Lead)

### 6.1 Le récit `188/188` et le récit « toutes les MUST sont traçables » ne sont pas le même

La ligne T2 de la matrice dit `PASS` sur `188/188` FeatureIds. Ce document
mesure **463/807 MUST atteints**. Les deux sont vrais et ne se contredisent
pas — mais un lecteur qui convertit l'un en l'autre se trompe d'un facteur
**4,3** sur le dénominateur (807/188). La ligne T2 devrait nommer la métrique
utilisée.

### 6.2 Deux identifiants de règle dupliqués (dont un **nouveau**)

- `G-5.11` : déjà signalé dans `T2-FINDINGS.md` §20.3 (type `!` et
  `const_expr`), renumérotation = décision humaine (`rule_id` est contractuel,
  Q-DIAG-4 / DIAG-9.6).
- **`G-6.40` : non signalé à ma connaissance** — deux règles distinctes au même
  identifiant : (a) `unsafe { … }` est une expression de bloc (grammar.md:408),
  (b) raccourci de champ en **pattern** (grammar.md:461). Même problème de
  contrat public que `G-5.11` : renuméroter est une décision, pas un nettoyage.

Conséquence mesurée : **805 identifiants uniques pour 807 énoncés**, et une
liaison FeatureId ne peut pas désigner sans ambiguïté l'une des deux règles
`G-6.40`.

### 6.3 `abi.md` contredit la FFI implémentée (rappel croisé)

`ABI-2.5` et `ABI-3.3` interdisent tout `unsafe`/`extern` généré ; le wrapper
FFI en émet. Cette RFC ne la corrige pas (hors périmètre), mais la métrique 2 la
rend visible : `abi.md` est l'un des documents entièrement non tracés, donc
aucun cas ne peut révéler la contradiction. Détail : `RFC-D7-FFI-EFFECTS.md` §3.

## 7. Rapport machine-lisible et reproduction

```bash
python tools/must_audit.py                                  # résumé humain, exit 0
python tools/must_audit.py --json target/must-audit.json    # rapport machine
python tools/must_audit.py --strict                         # exit 1 si un MUST n'est pas atteint (non branché en CI)
python -m unittest discover -s tools/tests -v               # 33 tests (11 nouveaux)
```

Schéma du JSON (`schema_version: 1`) :

| Champ | Contenu |
|---|---|
| `denominators` | `featureids: 188`, `must_statements: 807` |
| `featureid_metric` | total, états, exemptés, `strict_satisfied`, oracle cité |
| `must_metric.by_doc` | par document : nombre de MUST et répartition des liens |
| `must_metric.mapping` | occurrences par type de lien (recouvrement compris) |
| `must_metric.strongest_only` | répartition par lien le plus fort (somme = 807) |
| `must_metric.case_linked` / `strict_linked` | énoncés atteints avec un cas existant / strictement satisfait |
| `statements[]` | une entrée par énoncé : `id`, `doc`, `line`, `level`, `mapping`, `features`, `case_linked`, `strict_linked` |
| `problems[]` | tableaux malformés, exemptions invalides |

Environnement de la mesure : `5d88ecb`, Windows 11 x86-64, Python 3.13.7,
`python tools/features_coverage.py --strict` = exit 0. Test du dépôt réel inclus
dans `tools/tests/test_must_audit.py::RealRepositoryTests` (dénominateur borné,
188 FeatureIds, exit 0 en mode rapport).

## 8. Recommandations (propositions, aucune n'est appliquée)

1. **Nommer la métrique dans T2** : la ligne T2 de la matrice devrait dire
   « 188/188 **FeatureIds** ; traçabilité MUST : 463/807 (voir
   MUST-TRACEABILITY.md) ». Sinon un `PASS` est lu comme plus large qu'il n'est.
2. **Classer les documents non-fonctionnels** : ABI, COMPAT, CONC, MACH, DIAG,
   FEAT, UNS (194 MUST) ne sont pas des fonctionnalités. Soit `features.md`
   gagne une colonne « non couvert par FeatureId, vérifié par X », soit un
   document séparé le déclare. Aujourd'hui le silence est ambigu.
3. **Réduire les liens de groupe** dans les documents de fonctionnalités
   (grammar/lexical/modules/stdlib/ownership/effects) : citer les identifiants
   exacts au lieu de `TYP-11`, et écrire les cas manquants là où ils manquent.
4. **Trancher les doublons `G-5.11` et `G-6.40`** avant toute ratification
   d'ADR (renumérotation = décision humaine, identifiant contractuel).
5. **Décider si la métrique 2 devient un gate** (avec un seuil écrit, pas
   implicite) ; tant qu'elle ne l'est pas, elle reste une mesure, pas un `PASS`.

## 9. Ce que ce document ne fait pas

- Il ne modifie ni `features.md`, ni `coverage_exemptions.json`, ni la matrice
  des gates (les corrections sont signalées au Lead).
- Il ne change aucun seuil existant : `features_coverage.py` est inchangé,
  `must_audit.py` est report-only.
- Il ne convertit aucun `NOT_RUN`/`NOT_COVERED` en `PASS` et ne présente pas
  un lien déclaré comme une preuve d'exécution.

# Décisions E6 — couverture produit et comportements (2026-11-17)

> **Statut : APPROUVÉ par le responsable (Rwanbt), en session le 2026-11-17.**
> Réponse textuelle : « A … F - ok on suit ta recommandation » puis
> « ok on suit tes recommandations » (décisions 1 à 9 ci-dessous).
> Ce document est la référence pour toute session qui reprend E6
> (`docs/roadmap/PRODUCTION-PLAN-2026-11-15.md`), locale ou cloud.

## 0. Objectif fixé

- **Zéro exemption de ligne.** À terme, `tools/coverage_exemptions.json`
  `line_coverage_exemptions.entries` doit être vide ; S5 = 100 % brut
  sur le code produit (`cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen`).
- Les 58 exemptions actuelles **ne sont pas approuvées** : elles restent
  `PENDING_HUMAN_APPROVAL` le temps d'être supprimées par les traitements
  ci-dessous. Aucune nouvelle exemption de ligne n'est admise sans approbation
  explicite.
- Seule exemption conservée : la règle F-META-006 (approuvée le 2026-11-12,
  machine abstraite non exécutable, révision au plus tard le 2027-02-12).
- Principe approuvé : un chemin défensif « impossible » se vérifie par un
  **test unitaire interne** (`#[cfg(test)]`) qui forge l'état incohérent, ou
  disparaît parce qu'un **type** le rend non représentable. Un `expect` /
  `unreachable!` avec un test `#[should_panic]` est admis là où la défense
  n'a pas de sens.

## 1. Traitement par famille d'exemptions (toutes approuvées)

| Famille | Contenu (≈ entrées) | Traitement approuvé |
|---|---|---|
| A | Formes que le parseur interdit (16) : plage ouverte hors `for` (6), chemin vide (2), `use` finissant sur un module, module en ligne sans nom, affectation non-place, membre d'`impl` non-fn (2), `?`/`return` dans une constante, motifs impossibles dans `coverage.rs` (2) | **Refonte des types de l'AST** : `Impl { items: Vec<FnItem> }`, plage de `for` à deux bornes, `Path` à segments non vides, etc. Les bras disparaissent. PR dédiée, S7 et conformance identiques. |
| B | Faits garantis par `resolve` (20) : signatures/structs/enums absents de `facts`, espace de noms, builtins inconnus, index de modules et de fonctions, cible d'`impl`, tables nominales, confidentialité du premier segment, repli `Some`/`None`, tables d'items utilisateur, ancres `use` | **Tests unitaires internes** forgeant un `Facts` incomplet ; diagnostic interne propre attendu. Sinon `expect("invariant: …")` + `#[should_panic]`. |
| C | Invariants internes ownership / émetteur (15) : pile de portées vide, slot sans état (9 lignes), `break` après en-tête divergent, `&&` à gauche divergente, pas de natures différentes, déplacement par slice, chemins d'éléments, `planned_path`, identité de liaison | Comme B ; pour « slot sans état », envisager un `SlotId` obtenu seulement par `lookup`. |
| D | Code release seulement (2) : `verify_ownership_facts`, contre-vérification des chemins d'éléments | Remplacer `cfg!(debug_assertions)` par `#[cfg(debug_assertions)]` sur l'élément. |
| E | Courses avec le système de fichiers (4, `loader`) | **Interface `Fs`** dans le loader + fausse implémentation pour l'injection de fautes ; sert aussi aux erreurs d'E/S de `fsutil.rs`, `files.rs`, `lockfile.rs`. |
| F | `fmt::Error` dans les `Display` (1) | Aide d'écriture infaillible dans un `String`, sans `?`. |

## 2. Décisions de comportement (toutes : recommandation approuvée)

| # | Sujet | Décision |
|---|---|---|
| 1 | Types imbriqués ≥ 128 niveaux (rustc E0275, accepté par le checker) | Nouveau diagnostic de profondeur au checker (ex. `E-TYP-DEPTH`), limite écrite dans la spec, cas de conformance. Pas de `recursion_limit` dans le Rust généré. |
| 2 | Bornes 1024 (`resolve_vars`, exhaustivité `MAX_COVERAGE_DEPTH`) | Une seule constante de profondeur normative, dérivée de la décision 1 ; toutes les bornes internes la suivent ; les gardes deviennent testables. |
| 3 | Motif de constante (`crate::T`) hors exhaustivité | Garder le refus (sûr) et l'écrire dans la spec ; évaluer les constantes est une extension future. |
| 4 | Locales homonymes d'une constante émises `<nom>_rv_local` | Garder ; réserver le motif `_rv_` dans la spec (identifiant utilisateur qui le contient refusé). |
| 5 | `stdlib.md:315` attend `E-TYP-EQ` pour `Vec::<i32>::new()` | Corriger la doc : G-5.9 fait foi (erreur de parse). |
| 6 | `p.m` sur méthode utilisateur sans appel → « no field » | Aligner sur les builtins : même code, indice « method, call it `p.m()` ». |
| 7 | G-6.1 en position d'instruction (`if c { } (1, 2);`) | L'instruction s'arrête à `}` (comme Rust), cas de conformance. |
| 8 | `for j in 1.. { }` lit `{` comme borne | Message dédié « plage ouverte interdite dans un `for` ». |
| 9 | PR #148 vers `dev` | Fusionner bientôt (sur accord explicite au moment de la fusion) ; la suite en une PR par étape. |

## 3. Ordre d'exécution

1. Les lignes de dette restantes (105 à `fa1935d`) : tests bout en bout ou suppression avec preuve.
2. Familles D et F.
3. Familles B et C (tests forgés).
4. Interface `Fs` (famille E + erreurs d'E/S du loader).
5. Famille A (refonte AST), PR dédiée.
6. Décisions 1 à 8 : un petit commit spec + test chacune.

Les règles non négociables d'E6 restent en vigueur : pas de promotion de gate
sans commande, sortie et SHA ; pas de tag, release, signature ni fusion vers
`main` sans approbation ; jamais affaiblir un test, un seuil ou un contrat ;
chaque chiffre écrit provient d'une sortie réelle.

## 4. Point de mesure au moment de la décision

Tête `fa1935d` (branche `test/e6-product-coverage`), Linux x86_64 :
`coverage_product_split.py` **97,80 %** brut, 291 lignes découvertes ; S5
**99,21 %** : 186 lignes exemptées (58 entrées), **105** lignes de dette,
0 exemption périmée ; `coverage_report.py --check` : seuils respectés ;
CI verte sur `fa1935d`.

## 5. Avancement

- 2026-11-18, tête `0dbcf40` : familles A–F exécutées. `line_coverage_exemptions.entries` est vide ;
  100,00 % brut sur 13 135 lignes produit (sema + codegen), 0 dette. Ordre §3 : étapes 1 à 5 faites.
  Reste l'étape 6 (décisions de comportement 1 à 8).
- 2026-11-18, tête `ec5afcc` : étape 6 faite, décisions 1 à 8 (un commit spec + test chacune) :
  1–2 `c9c6bed`, `b77de8b`, `d2c4414` (TYP-1.7, `MAX_TYPE_DEPTH` = 100, `E-TYP-DEPTH` ; rustc
  échoue mesuré à 129 `Option` imbriqués, E0320) ; 3 `9b0fffb` (EXPR-5.8) ; 4 `224707d`, `6b6eef6`,
  `be3ece6` (LEX-4.6, `E-LEX-RESERVED-IDENT`, boucles anti-collision devenues mortes supprimées) ;
  5 `7986cd9` ; 6 `9a6a21e` ; 7 `da3a399` (G-6.1) ; 8 `ec5afcc` (EXPR-6.9).
  Mesure locale Linux x86_64 à `ec5afcc` : 1 554 tests, 0 échec ; clippy propre ; conformance
  `--verify-build` 501 PASS / 0 FAIL / 2 SKIP ; traçabilité stricte 188/188 ; couverture produit
  100,00 % sur 13 172 lignes, 0 exemption, 0 dette ; seuils respectés.
  Écarts à arbitrer : pas de cas de conformance pour la décision 1 (règle « aucun cas ajouté depuis
  Linux ») ; tests existants dont l'attente a suivi une décision (audits backend profondeur 100,
  audits `_rv_`, deux tests de message `p.m`).
- 2026-11-18, arbitrage du propriétaire sur les écarts ci-dessus :
  1. Audits de profondeur : validés. Les audits qui construisent restent à la borne (99 `Option`,
     profondeur 100) ; un audit de bout en bout ajouté à la profondeur 128 d'origine exige un seul
     `E-TYP-DEPTH` (type écrit, type inféré, paramètre). Il a révélé deux défauts de récupération,
     corrigés : un type écrit refusé reste profond au lieu de devenir `Error` (cascade de
     `E-TYP-MISMATCH`) ; les motifs tuple, `Some`, `None`, `Ok`, `Err` refusent le type d'erreur
     récupéré, et une valeur vérifiée contre lui laisse ses variables ouvertes (`E-TYP-AMBIGUOUS`
     en plus d'`E-RES-UNKNOWN` pour `let x: Inconnu = None;`).
  2. Cas de conformance de la décision 1 : **action ouverte**, à ajouter depuis la plateforme de
     référence (pas depuis Linux).
  3. Interface `Fs` complète du loader : retenue, PR dédiée.
  4. `#[should_panic]` : l'état de slot de `write_place` était déjà couvert (`e3e3e91`) ;
     l'enregistrement du manifeste est extrait dans `register_manifest` et testé.
  Mesure locale Linux x86_64 après ces corrections : 1 560 tests, 0 échec ; clippy propre ;
  conformance `--verify-build` 501 PASS / 0 FAIL / 2 SKIP ; traçabilité stricte 188/188 ;
  couverture produit 100,00 % sur 13 220 lignes, 0 exemption, 0 dette ; seuils respectés.
  Exception `_` décidée par le propriétaire : DIAG-1.9, un nom préfixé par `_` ne produit jamais
  `W-OWN-UNUSED-VARIABLE` (avant : signalé, la spec ne prévoyait pas d'exception).

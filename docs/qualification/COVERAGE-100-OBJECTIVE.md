# Objectif D10 « 100 % lignes et régions par fichier » — ce qui est atteignable, et ce qui ne l'est pas

> Décision ratifiée le 2026-11-11 : 100 % de couverture de **lignes** ET de
> **régions** sur chaque fichier mesuré (`tools/coverage_thresholds.json`,
> champ `objective`). Ce document consigne une **limite structurelle** mesurée en
> 2026-11-14 (round 10 du GOAL). Il ne change aucun seuil et ne demande aucune
> exclusion : il documente pourquoi certains fichiers ne peuvent pas atteindre
> 100 % de régions **par des tests seuls**, et il soumet la décision.

## 1. Mesure

Après les rounds 4-10 (ajout d'`OwnFacts`, de `traps.rs`, des formes entières
explicites, et fermeture de `codegen/lib.rs` à 100/100), l'état est :

```
cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen \
  --json --output-path target/cov-round10.json
python tools/coverage_report.py --input target/cov-round10.json --check   # PASS
```

| Fichier | Lignes | Régions | Trou restant |
|---|---|---|---|
| `crates/ravum-lang-codegen/src/lib.rs` | **100,00** | **100,00** | clos par test au round 9 |
| `crates/ravum-lang-sema/src/types.rs` | **100,00** | **98,10** | 9 régions = chemins d'erreur de `write!` dans `impl Display` |
| `crates/ravum-lang-sema/src/builtins.rs` | **100,00** | **98,42** | 8 régions du même type |
| `crates/ravum-lang-sema/src/traps.rs` | 98,66 | 96,84 | bras défensifs inatteignables + marche d'expressions |
| Totaux sema+codegen | 89,41 | 88,81 | 10 fichiers sous l'objectif |

## 2. Pourquoi `types.rs` plafonne à 98,10 % de régions

Les segments `llvm-cov` du fichier montrent que le code est **exécuté** (compteur
non nul sur le chemin nominal) mais que les régions d'erreur des `write!(...)?`
restent à zéro :

```
line=219 col=31 count=0   entry=True   // le `?` de write!(f, "(")?
line=222 col=40 count=0   entry=True   // le `?` de write!(f, ", ")?
line=224 col=40 count=0   entry=True   // le `?` de write!(f, "{item}")?
line=235 col=36 count=0   entry=True   // idem pour le nom d'un type nominal
...
```

Ces régions ne s'exécutent que si le `Formatter` retourne `Err`, c'est-à-dire
jamais avec `format!`, `assert_eq!` sur `String`, ou un `Display` normal. Les
couvrir exigerait soit un `Formatter` factice qui échoue (non exposé par
`std::fmt`), soit de réécrire les `Display` en **ignorant** l'erreur de
formatage (`let _ = write!`), ce qui :
1. changerait le comportement en cas de formateur défaillant ;
2. ne rendrait pas la région « couverte » pour autant — elle disparaîtrait, ce
   qui n'est pas la même chose que la prouver.

`builtins.rs` est dans la même situation (8 régions de chemins d'erreur).

## 3. Ce que cela implique pour D10

- L'objectif 100 % **régions** n'est pas atteignable par des tests sur les
  fichiers riches en `Display`/`write!` : la métrique compte des branches
  d'erreur que l'API standard ne permet pas d'atteindre.
- Les seuils actuels (cliquet) restent inférieurs à 100 et **la CI reste
  verte** : aucun seuil n'a été abaissé, aucun fichier n'est exclu.
- `codegen/lib.rs` montre que les fichiers sans ces branches peuvent atteindre
  100/100 et que le cliquet peut alors être relevé à 100.
- `traps.rs` (98,66/96,84) est entré au cliquet à 97/95 ; ses trous restants
  sont des bras défensifs que ni le parser ni le typechecker ne produisent
  aujourd'hui (par exemple un `let` sans initialiseur, un décalage sur un type
  non entier, un type d'expression absent). Les supprimer rendrait le module
  fragile à la première évolution de l'AST ; ils sont conservés et documentés.

## 4. Décision demandée (humaine)

Trois options, aucune ne peut être tranchée par un agent :

1. **Autoriser une exclusion de mesure ciblée** pour les régions d'erreur
   `fmt::Error` des `impl Display` (liste fermée de fichiers, écrite dans
   `coverage_thresholds.json`), le reste restant à 100 %.
2. **Changer l'objectif** : 100 % lignes + seuils de régions par fichier, les
   régions d'erreur de `fmt` étant déclarées non mesurables.
3. **Laisser l'objectif tel quel** et accepter que certains fichiers restent
   sous 100 % de régions, avec ce document comme trace.

En attendant la décision : **aucun changement de code ni de seuil** ; D10 reste
`PARTIAL` ; la Couverture totale est publiée à **89,41/88,81** (round 10) et les
planchers par fichier sont tenus.

## 5. Re-mesure du 2026-11-14 (consolidation 2, après T6/T7)

La réécriture T6 de `traps.rs` (indexation bornée, `--rt-entry`) a fait
descendre le fichier à **86,70/78,02**, sous son plancher 97/95 : le gate de
couverture était rouge. Il a été remis au vert **sans toucher aucun seuil**, par
six tests de couverture qui exercent l'analyse AST pure (balayage de
réaffectation sur toutes les formes d'expression et de motif, refus de
`safe_loop`, corps de méthodes, blocs `unsafe`, boucles sur collection, aides de
type défensives) ; `facts.rs` a supprimé une branche `if let` inatteignable (la
table `FileInfo` est dimensionnée `max(id) + 1`).

| Fichier | Lignes | Régions | Plancher |
|---|---|---|---|
| `crates/ravum-lang-sema/src/traps.rs` | **97,61** | **97,13** | 97 / 95 |
| `crates/ravum-lang-sema/src/facts.rs` | **100,00** | **100,00** | 100 / 100 |
| Totaux sema+codegen | **89,79** | **89,23** | — |

`coverage_report.py --check` : **exit 0**, tous les seuils tenus ; 21 257
lignes instrumentées, 19 087 couvertes. Aucun fichier n'a été exclu et aucun
plancher n'a été abaissé (commit `1b76926`).

## 6. Campagne de fermeture du 2026-11-14 (soir)

**Tests loader (25 tests, `crates/ravum-lang-sema/tests/loader_contracts.rs`).**
Les chemins d'erreur du chargeur (manifeste invalide, dépendance absente/cyclique,
lockfile divergent, symlinks, limites de taille, UTF-8, collisions de modules,
cache de parse, digest) étaient le plus gros trou produit : **`loader.rs` passe
de 78,85 % à 83,46 % de lignes** (2 222 → 2 352 / 2 818) et de **82,19 % à
86,70 % de régions** ; aucun code produit n'a changé, aucun test n'a été
supprimé, aucun seuil n'a été abaissé. Les 25 tests n'utilisent que l'API
publique du loader et asserent le code d'erreur exact + l'état fail-closed.

**Effet sur la mesure officielle** (`cargo llvm-cov -p ravum-lang-sema
-p ravum-lang-codegen --json`, arbre de la campagne) :

| Métrique | Avant (`1b76926`) | Après (campagne) |
|---|---|---|
| Lignes sema+codegen | 89,79 % (19 087/21 257) | **90,44 % (19 237/21 271)** |
| Régions sema+codegen | 89,23 % | **89,88 %** |
| `loader.rs` lignes / régions | 78,85 / 82,19 | **83,46 / 86,70** |
| Fichiers sous l'objectif 100 % | 10 | 10 |

**Défaut d'outil corrigé — le split produit/test était faux pour `loader.rs`.**
`tools/coverage_product_split.py` prenait le **premier** `#[cfg(test)]` comme
frontière du code de test. Or `loader.rs` déclare deux helpers `#[cfg(test)]`
vers la ligne 93, avant son module `mod tests` (ligne 3 213) : tout le fichier
était donc classé « test », d'où un absurde « 0 ligne produit / 100 % ». Le
script cherche désormais un attribut qui introduit un `mod tests`, et cinq tests
le figent (`tools/tests/test_coverage_product_split.py`). Après correction :
**1 557 lignes produit non couvertes / 13 598 (88,55 %)**, **221** lignes dans
les modules de test. Les chiffres antérieurs « 1 715 / 86,41 % » (export
`cov-batch1`) et « 781 lignes de test » (dossier de décisions, même outil sur
`cov-all.json`) étaient **incohérents** : le premier venait d'un export
antérieur, le second du bug ci-dessus. Le chiffre à retenir est le nouveau,
reproductible.

**Reste, et c'est une décision humaine (D10).** L'objectif ratifié « 100 % de
lignes ET de régions par fichier » n'est pas atteint : 10 fichiers restent en
dessous, dont les gros `emit.rs` et `loader.rs` ; `--gap` compte 2 034 lignes
non couvertes par instanciation (le total produit+test fusionné en compte
1 778 ; l'écart est la duplication d'instanciations documentée en §7 bis de
`PRODUCTION-READINESS.md`). Aucun test n'est ajouté « pour la ligne » et aucun
plancher n'est relevé tant qu'un fichier n'atteint pas 100 %.

## 7. Campagne de fermeture « vague 3 » du 2026-11-14 (agent F, décision S5)

**Décision appliquée.** S5, APPROUVÉE par le responsable le 2026-11-14 : « 100 %
lignes et régions du CODE PRODUIT, hors modules `#[cfg(test)]` et hors chemins
inatteignables documentés un par un ; seconde métrique : mutation ciblée ».
Cette section remplace, pour la mesure, la formulation « par fichier toutes
lignes confondues » de §1 : le dénominateur est le code produit, et les chemins
inatteignables sont listés **un par un** dans `tools/coverage_exemptions.json`
(bloc `line_coverage_exemptions`, 14 entrées, statut
`PENDING_HUMAN_APPROVAL`). Aucun seuil n'a été abaissé ; aucun test existant
n'a été modifié, supprimé ni affaibli.

**Contexte mesuré.** Branche `wave3/f-coverage-mutation` créée de `goal/wave-3`
au SHA `3ca4d0e` (intégration 1 : agents A, C, D fusionnés), Windows 11 x86-64,
`rustc 1.95.0`, `cargo-llvm-cov 0.9.1`. Commande :

```
cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen --json --output-path target/cov-all.json
python tools/coverage_product_split.py --input target/cov-all.json
python tools/coverage_report.py --input target/cov-all.json --gap --check
```

**Tests ajoutés (65, tous de comportement, tous nouveaux fichiers).**
`crates/ravum-lang-codegen/tests/emit_constructs.rs` (39 : types, motifs,
constantes, échappements, builtins libres et méthodes, FFI, mode test, refus) ;
`crates/ravum-lang-sema/tests/checker_entry_contracts.rs` (16 :
`check_with_timings`, entrée `rt` valide et refusée, `self` hors méthode,
`use crate::`/`super::super`, `test fn`, motif de variante structure, refus du
`let` déstructurant et du `ModDecl` résiduel) ;
`crates/ravum-lang-sema/tests/loader_manifest_contracts.rs` (10 : sections et
clés strictes, erreurs lexicales TOML, formes de dépendance, deux racines,
inlining des modules, identifiants `CrateKind`/`LockfileState`). Chaque test
asserte un code de diagnostic exact, un texte émis exact ou un fait consommé par
le backend : aucun test ne se contente d'incrémenter un compteur.

**Mesure officielle (même JSON pour les deux chiffres).**

| Fichier | Lignes produit avant | Après | Régions avant | Après | Plancher avant → après |
|---|---|---|---|---|---|
| `codegen/emit.rs` | 88,00 | **90,61** | 87,26 | **90,69** | 86/85 → **90/90** |
| `sema/loader.rs` | 83,46 | **85,31** | 86,70 | **88,09** | 76/80 → **85/88** |
| `sema/typeck.rs` | 93,53 | 93,53 | 93,11 | 93,11 | 93/92 → **93/93** |
| `sema/own.rs` | 92,47 | 92,47 | 90,75 | 90,75 | 91/89 → **92/90** |
| `sema/resolve.rs` | 93,26 | 93,26 | 93,45 | 93,45 | 93/93 (inchangé) |
| `sema/effects.rs` | 92,61 | **96,59** | 94,26 | **96,98** | 92/94 → **96/96** |
| `sema/check.rs` | 80,52 | **81,30** | 73,16 | **73,72** | 80/72 → **81/73** |
| `sema/traps.rs` | 97,61 | 97,61 | 97,13 | 97,13 | 97/95 → **97/97** |
| `sema/types.rs` | 100,00 | 100,00 | 98,10 | 98,10 | 100/97 → **100/98** |
| `sema/builtins.rs` | 100,00 | 100,00 | 98,42 | **98,89** | 100/98 (inchangé) |
| `sema/facts.rs`, `codegen/lib.rs`, `codegen/prelude.rs` | 100,00 | 100,00 | 100,00 | 100,00 | 100/100, 100/100, 100/97 → **100/100** |

Totaux mesurés : **lignes 91,48 % (19 650/21 479), régions 91,23 %**.
**Code produit seul (S5)** : `coverage_product_split.py` donne **90,17 %
(12 259/13 596), 1 337 lignes produit non couvertes** — contre **88,61 %
(1 549/13 596)** au même SHA avant les tests. Progression :
**+1,56 point, 212 lignes produit fermées**, aucun code produit modifié.
`tools/coverage_report.py --check` : **exit 0** avec les planchers relevés.

**Chemins inatteignables déclarés (14 entrées, 140 lignes).** Chaque entrée de
`line_coverage_exemptions` porte : la plage de lignes, la raison, la preuve
(test exécuté qui montre le refus au bon endroit, ou argument statique sur le
parseur/checker), et une condition de réexamen. Elles couvrent : `builtin_fn_value`
(builtin en position de valeur ⇒ `E-TYP-CALL`), les arités `panic`/`assert`/
`assert_eq` (⇒ `E-TYP-ARITY`), les constantes builtin autres que `Option::None`
(⇒ `E-RES-UNKNOWN`), le repli `to_string` mort, la garde release-only
`verify_ownership_facts`, le bras `ModDecl` (le chargeur inline les modules),
les bras `PathRoot::Plain` `crate`/`self`/`super` et `super::super` de
`resolve.rs` (parseur), le `let` déstructurant et le repli de signature
`own.rs`, les arêtes `fmt::Error` de `types.rs`/`builtins.rs` (§2), et deux
formes TOML que le lecteur du manifeste ne peut pas produire. **140 lignes
seulement** sont ainsi couvertes par une exemption : le solde **1 197 lignes
produit (91,20 % hors exemptions)** reste une **dette comptée**, pas un achèvement.

**Ce qui reste, par fichier (résiduel non exempté).** `emit.rs` 406 lignes
(branches défensives de l'émetteur : rendu de types sans faits, chemins
d'erreur, planificateur de moves d'arrays) ; `loader.rs` 310 (E/S et
diagnostics de graphe de dépendances) ; `typeck.rs` 236 (bras défensifs de
contrôle et d'exhaustivité) ; `own.rs` 121 ; `resolve.rs` 108 ;
`effects.rs` 10 ; `traps.rs` 6 ; `check.rs` 0. **L'objectif S5 n'est pas
atteint** : 1 337 lignes brutes (1 197 hors exemptions proposées) restent non
couvertes, `10` fichiers sont sous 100 %, et aucune exemption n'est ratifiée par
un humain.

**Ce que cette campagne ne prouve pas.** Elle ne prouve pas que les 140 lignes
exemptées sont définitivement mortes (chaque entrée nomme sa condition de
réexamen) ; elle ne prouve pas l'absence de trous de comportement dans le code
couvert (la seconde métrique, la mutation ciblée, est publiée séparément dans
`MUTATION-EQUIVALENCE.md` §13) ; et elle ne change aucun gate : D10 et T12
restent des décisions humaines, et les seuils relevés sont un cliquet, pas une
preuve de maturité.


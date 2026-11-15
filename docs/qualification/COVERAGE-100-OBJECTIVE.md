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

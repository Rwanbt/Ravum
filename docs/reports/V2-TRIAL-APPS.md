# Applications d'essai écrites en Ravum (V2-G3) — 2026-11-24

Ce rapport couvre la première étape avant la mise en production : écrire de
vraies applications en Ravum Core 1.0 et relever ce qui manque ou gêne.

État des preuves : `TESTED`, sous Linux x86_64, voie B (`rustc`), sur le
commit de ce rapport. Ce n'est ni un `PROVED` ni un audit.

## Applications

| Application | Rôle | Ce qu'elle exerce | Tests |
|---|---|---|---|
| `apps/todo` | Liste de tâches persistée dans un fichier (`list`, `add`, `done`, `clear-done`) | structs, `Vec`, `Result` et `?`, `fs`, `env::args`, `process::exit`, texte Unicode | 3 `test fn`, plus des essais manuels de chaque commande et des cas d'erreur (codes 1 et 2) |
| `apps/log-report` | Comptage d'un journal par niveau et liste des erreurs ; code 1 s'il y a des erreurs | enum, `match`, `lines`, `starts_with`, `for` sur un `Vec` possédé | 2 `test fn`, plus un essai manuel |

Commandes :

```bash
ravum-lang test apps/todo
ravum-lang test apps/log-report
ravum-lang run apps/todo -- tasks.txt add "écrire le rapport"
ravum-lang run apps/log-report -- app.log
```

## Défauts trouvés et corrigés

1. **`==` sur un `String` le déplaçait** (contraire à TYP-7.3, qui compare
   par une vue `&str`) : `if s == "a" {} else if s == "b" {}` était refusé
   (`E-OWN-DOUBLE-MOVE`). Le vérificateur d'ownership lit désormais les
   opérandes d'une comparaison sans les consommer. Cas
   `run/string-compare-does-not-move`.
2. **Cascade de diagnostics** : une seule erreur sur `for` produisait aussi
   des `E-TYP-FIELD` et `E-RES-UNKNOWN` sur le type `<error>`. Un champ ou
   une méthode sur une valeur déjà en erreur n'ajoute plus de diagnostic.
   Cas `types/no-cascade-on-error-type`, qui exige exactement un code.
3. **Contradiction de la spec** : TYP-8.4 disait « `String` n'est pas
   comparable avec `==` », alors que TYP-7.3 (Q-TYP-7, résolue) et le
   compilateur l'admettent. TYP-8.4 et l'exemple négatif de §7 sont alignés
   sur TYP-7.3 ; l'ordre sur le texte reste refusé (`E-TYP-CMP`).

## Limites rencontrées (voulues par la spec, à arbitrer)

Ces points ne sont pas des bogues : ils découlent de règles `MUST` actuelles.
Les lever demande une RFC et une décision du responsable. Aucune n'est
acceptée ici.

| # | Limite | Règle | Effet sur les essais | Proposition |
|---|---|---|---|---|
| L1 | Pas de découpage de texte : ni `split`, ni `find`, ni sous-chaîne | STD-9.11 | `todo` stocke l'état et le titre sur deux lignes ; `log-report` ne peut pas extraire un champ (`ms=123`) | RFC : `split(sep) -> Vec<String>`, `find(s) -> Option<usize>`, `strip_prefix(s) -> Option<String>` |
| L2 | `for` impossible sur `&Vec<T>`, et une référence ne peut pas être stockée dans un `let` | EXPR-6.4, OWN-6 | toute lecture d'une liste empruntée passe par une boucle `while` sur un index | RFC : `for x in &v`, avec `x` limité au corps de la boucle |
| L3 | Pas de table associative | STD-1.3 | des compteurs fixes (`log-report`) ; pas de comptage par clé libre | RFC : `Map<String, V>` minimal |
| L4 | Pas de répertoire temporaire ni de `create_dir` | STD-3.6 | les tests écrivent dans `.ravum/` | RFC : `fs::create_dir`, ou un répertoire de test fourni par `ravum-lang test` |

## Non mesuré

- Les applications n'ont pas été lancées par la voie C (IR et JIT) ; seuls
  les cas de conformance passent par l'oracle `ir_corpus_oracle`.
- macOS et Windows : pas d'essai manuel ; la CI couvre les cas de conformance.

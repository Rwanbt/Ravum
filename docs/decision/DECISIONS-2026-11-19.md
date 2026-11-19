# Décisions du responsable — 2026-11-19

> Décision prise par le responsable du dépôt en réponse à la question posée sur
> la PR du lot `types.md` / `expressions.md`. Elle s'applique dès son
> enregistrement.

## D-19-1 — Une quantité de décalage négative est refusée à la compilation, même en variable

**Avant :** EXPR-3.4 refusait une quantité négative (`E-TYP-SHIFT`), mais le
checker ne le faisait que pour un littéral (`a << -1`). Avec `let s: i64 = -1;
a << s`, le programme compilait et s'arrêtait par un panic à l'exécution
(Q-EXPR-2 proposait ce panic pour une valeur dynamique).

**Maintenant :** le compilateur doit refuser toute quantité dont il ne prouve
pas qu'elle est positive ou nulle. Comme la valeur d'une variable n'est connue
qu'à l'exécution, la règle porte sur le type :

- une quantité de type **non signé** (`u8` … `u128`, `usize`) est acceptée ;
- une **constante de littéraux** (`3`, `1 + 2`, `(2 - 5)`) est évaluée : acceptée
  si sa valeur est `>= 0`, refusée sinon ;
- toute autre quantité de type **signé** est refusée (`E-TYP-SHIFT`), quelle que
  soit sa valeur à l'exécution.

La règle vaut pour `<<`, `>>`, `<<=` et `>>=`. Le dépassement de largeur
(`>= largeur_bits`) reste un panic contrôlé (EXPR-3.3).

**Motif :** une décision du responsable : la détection doit être faite à la
compilation, pas seulement pour les quantités écrites en dur.

**Migration :** convertir une quantité signée avec `as u32` après l'avoir
vérifiée, ou la déclarer non signée. Aucun exemple, application ni cas du
corpus n'utilisait une quantité signée en variable ; trois tests de contrat du
checker l'acceptaient et sont mis à jour.

**Q-EXPR-2 :** fermée par cette décision.

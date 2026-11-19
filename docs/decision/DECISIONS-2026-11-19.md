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

## D-19-2 — `MIN % -1` est un trap

**Constat :** la comparaison différentielle entre l'interpréteur de référence
(`crates/ravum-core-machine`) et le backend a trouvé une seule divergence sur
54 programmes : `core-machine.md` §5 disait `MIN % -1 = 0`, alors que le
binaire panique (`attempt to calculate the remainder with overflow`) et que
`RFC-RT-TRAPS` range `%` par `-1` parmi les opérations qui débordent.

**Décision :** `MIN % -1` est un **trap**, comme `MIN / -1` (EXPR-3.1).
`core-machine.md` §5 est aligné ; l'oracle et son corpus suivent. Le
compilateur ne change pas.

## D-19-3 — Concurrence : threads, canaux **et** `async` dans la 1.0

**Décision du responsable :** la 1.0 contient les deux phases de
[`RFC-E11-CONCURRENCY`](../rfc/RFC-E11-CONCURRENCY.md) : threads et canaux
(phase 1), puis `async`/`.await` sur un exécuteur déterministe (phase 2). La
phase 1 est livrée d'abord ; T6 n'est `PASS` qu'avec les deux.

## D-19-4 — SQLite : dépendances natives générales

**Décision du responsable :** au lieu d'un runtime natif fermé, Ravum gagne un
**mécanisme général de dépendances natives** (L1–L4 de
`RFC-P2-VERTICALS-BLOCKERS` §3.3). SQLite est la première dépendance qui
l'utilise. [`RFC-E11-SQLITE`](../rfc/RFC-E11-SQLITE.md) est réécrite en ce
sens ; cela élargit D7 (FFI), dont la frontière doit être réécrite dans la
même RFC.

## D-19-5 — Hot reload : `init`/`step` avec `migrate`

**Décision du responsable :** modèle `init`/`step` de
[`RFC-E11-HOT-RELOAD`](../rfc/RFC-E11-HOT-RELOAD.md), état sérialisé au
remplacement, refus si le schéma change **sauf** si le nouveau module fournit
`migrate` (HR-1.7 passe de SHOULD à MUST dans la 1.0).

## D-19-6 — Ordre d'implémentation d'E11

Réseau → threads → SQLite → hot reload → DSP. Les RFC restent `PROPOSED`
jusqu'à leur ratification ; ces décisions fixent les choix de conception.

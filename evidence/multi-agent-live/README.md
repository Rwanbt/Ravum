# Expérience multi-agents réelle — 2026-11-08

> **Ce dossier contient les traces d'une exécution réelle**, pas une simulation.
> Trois subagents ont travaillé simultanément sur le **même checkout partagé**
> (`D:\App\Ravum`), sans worktree, chacun dans un répertoire privé, puis un
> coordinateur unique a intégré les propositions et un seul écrivain a commité.

## 1. Dispositif

| Élément | Valeur |
|---|---|
| Branche partagée | `feat/3-mvp0a-continuous-loop` |
| Checkout | `D:\App\Ravum` (aucun worktree créé) |
| Agents | `agent-a`, `agent-b`, `agent-c` (subagents réels, contexte séparé) |
| Répertoires privés | `target/ravum/live/agent-{a,b,c}/` — exclusifs, hors arbre versionné |
| Base commune | `f6d572a5bf187c3450db37d77b294244a2969410db6313e042d685a4b2967374` |
| Autorité Git | Prime Agent uniquement ; les agents n'ont lancé aucune commande Git |
| Builds lancés par les agents | aucun (contrainte RAM : ~2 Go libres) |

Chaque agent a reçu une tâche, un fichier exclusif et l'interdiction de toucher au
reste. Ils ont produit :

| Agent | Tâche | Symbole | Modification |
|---|---|---|---|
| A | `refine-helper-offset` | `helper` | `x + 1.0` → `x + 20.0` |
| B | `scale-compute-output` | `ravum_compute` | `helper(x) * 2.0` → `helper(x) * 5.0` |
| C | `refine-helper-offset-alt` | `helper` | `x + 1.0` → `x + 30.0` (volontairement concurrent de A) |

## 2. Décisions du coordinateur

Journal complet : `journal.jsonl`. Résumé :

| # | Proposition | Décision | Raison enregistrée |
|---|---|---|---|
| 1 | `agent-a/refine-helper-offset` | `AUTO_SAFE` | préconditions satisfaites, patch exact, aucun chevauchement |
| 2 | `agent-b/scale-compute-output` | `AUTO_SAFE` | idem |
| 3 | `agent-c/refine-helper-offset-alt` | `CONFLICT` | `symbol "helper" is already modified by agent-a/refine-helper-offset` |

Résultat : snapshot `f6d572a5bf18…` → `b933ef1c31a0…` ; commit unique
`bd5968cf028b` (parent `7280dd9ac366`), message
`chore(collab): integrate 2 proposal(s) touching 1 path(s)`.

## 3. Preuves d'intégrité

- `integrated-two_functions.rs` contient **les deux intentions** :
  `x + 20.0` (A) et `helper(x) * 5.0` (B). Aucune n'a écrasé l'autre.
- La modification de C (`x + 30.0`) **n'apparaît nulle part** : elle a été refusée,
  pas fusionnée puis perdue.
- Le fichier intégré **compile réellement** :
  `ravum build target\ravum\live\workspace\two_functions.rs` →
  `artifact_id = bd499825f9f5530e12c70a99216fa8ef3c67f9d5001cfb414503f75e9bf2079d`,
  `build_ms = 334`.
- `agent-c-two_functions.rs` est conservé tel quel : le travail refusé n'est pas
  détruit, il reste consultable.

## 4. Constat technique issu de cette exécution

La première tentative a classé la proposition B en `CONFLICT` alors qu'elle touchait
une **autre fonction**. Cause : avec le contexte standard de 3 lignes, le diff de A
couvrait les lignes 23–29 et celui de B les lignes 29–33 — ils partageaient une
ligne de contexte.

Correction appliquée (`crates/ravum-workspace-coordinator/src/patch.rs`) : le
chevauchement se calcule désormais sur les **lignes réellement modifiées**, jamais
sur le contexte partagé. Deux agents qui éditent deux fonctions différentes du même
fichier ne sont plus bloqués ; deux agents qui éditent la même ligne le sont toujours.

C'est un cas concret où l'expérience a changé le code. Il est conservé ici parce
qu'il documente une limite réelle : la granularité « ligne modifiée » reste
textuelle, pas sémantique.

## 5. Limites de cette expérience

- Trois agents, une seule tâche chacun, un seul fichier : ce n'est pas une charge
  réaliste de production.
- Les agents n'ont pas compilé ; la validité du fichier intégré a été vérifiée
  **après** intégration, par le pipeline M1.
- Aucune mesure de « temps total agent » comparable à une baseline worktrees n'a été
  réalisée. C'est l'objet de S8-A/S8-B (#33, #68), non exécutés.
- Le commit a eu lieu dans un dépôt de démonstration temporaire
  (`target/ravum/live/workspace`), **jamais** dans le dépôt Ravum lui-même.

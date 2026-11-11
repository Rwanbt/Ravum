# Dossier d'exemption F-META-006 — `NOT_COVERED`, justifié et révocable

> **Statut : `PENDING_HUMAN_APPROVAL`.** Ce dossier documente une exemption
> **déjà machine-lisible** (`tools/coverage_exemptions.json`) et mesure ses
> conséquences. Il ne remplace pas une approbation humaine : aucun agent ne
> signe à la place du responsable. Date : 2026-11-11.

## 1. Ce qui est exempté, exactement

| Élément | Valeur |
|---|---|
| FeatureId exempté | `F-META-006` — « Machine abstraite du noyau pur (oracle) » |
| Ligne source | `spec/language/features.md:268` |
| Colonne `Pass / Fail` | « pass : aucun (oracle absent) ; fail : citer un résultat comme preuve » |
| Colonne `Conformance` | littéralement `NOT_COVERED` |
| Statut de la fonctionnalité | `experimental` |
| Artefact d'exemption | `tools/coverage_exemptions.json`, entrée unique, `rule = F-META-006` |
| Vérificateur | `tools/features_coverage.py --strict` (exit 0 si l'exemption est bien formée, connue, non périmée, et si la règle est littéralement `NOT_COVERED`) |
| Mesure courante | `188/188` règles satisfaites : **187 par cas + 1 exemption justifiée** |

## 2. Justification (normative, déjà écrite)

- `spec/language/core-machine.md` **MACH-1.4** (MUST) : « Son état est
  `NOT_IMPLEMENTED`. Aucun résultat de la machine ne peut être cité comme
  preuve. »
- `spec/language/features.md:268` déclare elle-même qu'aucun cas de `pass`
  n'existe tant que l'oracle n'existe pas.
- Conséquence : exiger un cas de conformance pour `F-META-006` obligerait à
  inventer un oracle, c'est-à-dire exactement le faux `PASS` que le gate T2 doit
  interdire. L'exemption n'est donc pas une tolérance : c'est la seule lecture
  cohérente de deux textes MUST.

## 3. Ce que l'exemption n'affirme pas

Elle n'affirme **aucune** des choses suivantes, et aucun document ne doit les
déduire :

- que la machine abstraite est implémentée ou testée ;
- que les règles `MACH-*` (40 MUST, mesurées par
  [`MUST-TRACEABILITY.md`](MUST-TRACEABILITY.md)) sont tracées vers un cas ;
- que « T2 `PASS` » signifie « toutes les MUST de `spec/language/` sont
  traçables ». Le chiffre `188/188` porte sur les **FeatureIds**, pas sur les
  **807 MUST** ; l'audit `tools/must_audit.py` mesure **344 MUST sans
  FeatureId**, hors de cette exemption comme hors du dénominateur 188.
- que l'exemption est approuvée par un humain : le champ `approved_by` actuel
  dit lui-même « claim interne, non ratifié par un humain ».

## 4. Responsable proposé (à désigner par le responsable du projet)

Le champ `approved_by` doit nommer une **personne** (ou un rôle explicitement
délégué) qui accepte l'exemption en connaissance de MACH-1.4. Proposition, non
contraignante :

| Rôle proposé | Qui | Ce qu'il accepte |
|---|---|---|
| Approbateur de l'exemption | **mainteneur du projet Ravum** (à nommer) — à défaut, le responsable du gate T2 | que `F-META-006` reste non testée jusqu'à l'existence d'un oracle indépendant, et que T2 soit lu comme `PASS` sur 187/187 règles testables |
| Rédacteur du suivi | l'agent de qualification (proposition seulement) | met à jour la date et la condition de réexamen à chaque passe ; ne peut pas approuver |

Aucune proposition ci-dessus n'est une approbation. Tant qu'un humain n'a pas
remplacé le libellé actuel, la ligne reste **`PENDING_HUMAN_APPROVAL`**.

## 5. Condition de réexamen (déjà écrite) et échéance proposée

Condition actuelle dans `tools/coverage_exemptions.json` : révoquer dès qu'une
machine abstraite exécutable existe (MACH-1.4 quitte `NOT_IMPLEMENTED`) ou si la
ligne `F-META-006` obtient une forme exécutable ; la couverture stricte doit
alors repasser à `188/188` **sans exemption**.

Échéance proposée (à confirmer par le responsable) :

- **Échéance de décision** : avant la promotion vers `main` (D3) ou avant tout
  tag de release, au premier des deux. Une exemption non revue ne doit pas
  traverser une release.
- **Date de revue proposée** : 2026-12-15 (revue documentaire : l'oracle
  existe-t-il ? la ligne a-t-elle changé ? le texte de MACH-1.4 a-t-il bougé ?).
- **Déclencheur automatique** : toute modification de `MACH-1.4`, de la ligne
  `F-META-006`, ou l'ajout d'un interpréteur/oracle dans le dépôt doit rouvrir
  la revue — le champ `review_condition` de l'exemption le dit déjà.

## 6. Option d'oracle indépendant (ce qu'il faudrait, sans chiffrer un coût non mesuré)

Trois options, non exclusives. Aucun coût en jours n'est avancé : il n'a pas été
mesuré, et l'inventer serait une fausse précision.

### Option 1 — Interpréteur du noyau pur, écrit indépendamment

C'est l'oracle que `core-machine.md` décrit (`MACH-8.1` : « écrit
indépendamment (pas de réutilisation du checker ni du codegen) »).

- **Périmètre minimal** : `MACH-1.1` (profil `pure`, valeurs `Copy`), scalaires
  et `&'static str` seulement (`MACH-2`), `let`/`if`/`match`/boucles/appels
  (`MACH-4`), traps `⟨panic⟩`, budget d'étapes `N` (`MACH-5`, `MACH-7`).
- **Sortie comparable** : `valeur` typée, `panic: msg`, `diverge: N`, `fault`
  (`MACH-8.2`), comparaison structurelle et prédicats IEEE pour les flottants
  (`MACH-8.3`).
- **Interdits** : ne pas réutiliser le parser : le programme d'entrée doit être
  le source Ravum, mais l'oracle ne peut pas s'appuyer sur le checker pour
  décider du sens (`MACH-1.3`, `MACH-8.1`) ; il peut en revanche lire l'AST si
  l'indépendance porte sur la **sémantique**, pas sur la syntaxe — cette
  frontière doit être tranchée par le responsable, car les deux lectures
  existent dans le texte.
- **Preuve attendue** : une campagne `F-META-006` de cas `run-pass` sur le
  noyau pur, plus des cas de divergence/trap, avec modèle et limites déclarés
  (`MACH-8.5` : `TESTED`, jamais `PROVED`).
- **Statut actuel** : `NOT_IMPLEMENTED` (MACH-1.4).

### Option 2 — Oracle différentiel sur un second backend

Comparer le backend Rust généré à un second chemin d'exécution (voie C,
Cranelift) sur le même sous-ensemble. Ce n'est **pas** un oracle indépendant au
sens de MACH-8.1 (même frontend, mêmes faits) ; c'est un test de parité. Utile,
mais insuffisant pour révoquer l'exemption : il ne teste pas la sémantique, il
teste deux implémentations de la même sémantique.

### Option 3 — Maintenir l'exemption, réexaminée à chaque passe

Coût nul en code ; l'exemption reste visible, machine-vérifiée, et le gate T2
reste `PASS` sur 187/187 testables. Risque : une exemption permanente finit par
être lue comme un `PASS` implicite ; la mitigation est ce dossier plus la ligne
T2 de la matrice, qui la nomme.

**Recommandation (non contraignante)** : Option 3 jusqu'à ce qu'un besoin réel
d'oracle apparaisse (par exemple une divergence de sémantique constatée), puis
Option 1 avec un périmètre gelé. Commencer par l'Option 1 sans besoin identifié
reviendrait à construire un second compilateur pour satisfaire un chiffre.

## 7. Implications de gate

| Scénario | Effet sur T2 | Effet sur les autres gates |
|---|---|---|
| Exemption approuvée telle quelle (statu quo vérifié) | `PASS` sur **187/187 testables + 1 exemption** ; `features_coverage.py --strict` exit 0 ; CI bloquante | Aucun ; T2 reste le seul gate concerné |
| Exemption révoquée **sans** oracle | 187/188 ; `--strict` exit 1 ; la CI `gate-contracts` échoue → T2 doit passer à `FAIL` (ou `PARTIAL` si la définition du gate change — changement de définition interdit par la matrice) | Aucun autre gate ne dépend de T2 pour ses mesures ; la cohérence documentaire (`report_consistency`) citerait un chiffre faux si la matrice n'était pas mise à jour |
| Oracle Option 1 livré | 188/188 **sans** exemption ; l'entrée d'exemption devient **périmée** et le vérificateur échoue tant qu'elle n'est pas retirée (`stale exemption`) | Ouvre `MACH-8.5`/`F-META-006` à de vrais cas `TESTED` ; ne change pas T4 (audit externe) |
| Ligne `F-META-006` modifiée pour devenir testable sans oracle | L'exemption devient invalide (la cellule `Conformance` ne dit plus `NOT_COVERED`) → échec volontaire | — |

Point à ne pas manquer : **la matrice des gates doit rester le registre
canonique**. Ce dossier ne modifie pas la ligne T2 ; il documente ce qu'elle
signifie et ce qui la ferait changer.

## 8. Actions requises du responsable (checklist)

1. Nommer l'approbateur humain et remplacer `approved_by` dans
   `tools/coverage_exemptions.json` (fichier hors périmètre de cette passe, à
   modifier par le responsable ou le Lead).
2. Confirmer ou corriger l'échéance de revue (§5) et l'inscrire dans la ligne T2
   de `docs/roadmap/CORE-1.0-GATE-MATRIX.md`.
3. Trancher la frontière d'indépendance de l'oracle (§6, Option 1) : un oracle
   qui lit l'AST du compilateur est-il « indépendant » au sens de MACH-8.1 ?
4. Décider si T2 `PASS` doit, à terme, être redéfini sur la métrique MUST
   (807 statements) plutôt que sur les FeatureIds (188) — voir
   [`MUST-TRACEABILITY.md`](MUST-TRACEABILITY.md).

## 9. Ce que ce dossier ne fait pas

- Il ne signe pas l'exemption, ne modifie pas `coverage_exemptions.json` ni la
  matrice des gates (hors périmètre ; corrections signalées au Lead).
- Il ne présente pas l'exemption comme une preuve de quoi que ce soit.
- Il ne chiffre pas le coût de l'oracle (non mesuré) et ne propose pas de
  planning d'implémentation.

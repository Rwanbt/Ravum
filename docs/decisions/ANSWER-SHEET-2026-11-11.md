# Décisions du 2026-11-11 — réponses, effets, et questions restantes

> **Ce fichier remplace la feuille de questions** : les décisions ont été rendues,
> voici ce qu'elles changent, ce qui a été appliqué immédiatement, et les deux
> points qui attendent encore quelque chose de toi (les trois revues LLM pour D4,
> et un arbitrage sur D13).
>
> **État au commit `0c2ceb8`** : conformance **456 cas : 455 PASS / 0 FAIL /
> 1 SKIP**, périmètre build **195/195** ; workspace **113 suites, 1537 passés** ;
> **0 forme `check`-pass / `build`-fail connue** ; gates **4 PASS / 7 PARTIAL /
> 2 BLOCKED**.

## 1. Décisions rendues et leur suite

| ID | Ta décision | Ce qu'elle change | Suite donnée |
|---|---|---|---|
| **D2** | Cible **production ready**, pas MVP | La barre d'acceptation monte (conformité + audit + reproductibilité + couverture/mutation + interfaces + distribution) | [`docs/roadmap/PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) : 14 critères de sortie, état mesuré de chacun, plan en 3 vagues. La **ratification** (backend + socle normatif) reste à signer : dossier prêt |
| **D3** | **Fusion dans `main` autorisée**, aucune release | `main` devient la référence documentaire du travail ; `dev` d'intégration reste alignée | Fusion exécutée (voir §3 pour la provenance et les impacts exacts) ; **aucun tag, aucune release, aucune signature** |
| **D4** | Trois revues LLM déjà faites ; ne doit pas bloquer le code | Rien dans D4 n'empêche de coder, tester, fusionner, durcir | **Rien n'est bloqué côté code.** Ce qui manque : les **trois textes** pour les trier (voir §4). Le label « production ready » attend un audit **humain** indépendant |
| **D5** | Tests sur Windows et Linux, pas de macOS | Tier 1 = Windows x86-64 + Linux x86-64 (`ubuntu-latest`) ; le reste `NOT_RUN` | D5 **close**, écrit dans le registre et `PLATFORMS-MATRIX.md` |
| **D6** | OK (geler le périmètre) | Le sous-ensemble documenté devient le périmètre Core 1.0 ; extensions = 1.x | Périmètre gelé (§6) |
| **D7** | « Il faut faire en sorte de la rendre safe » | La FFI n'est plus « expérimentale » : elle devient un **programme de durcissement** avec contrats écrits et refus outillés | Programme détaillé : [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §5 |
| **D9** | Pas d'intérêt tant que le langage n'est pas prêt pour la prod | Signature de release **différée** ; aucun mécanisme bloqué | Statut `DEFERRED`, **aucune clé générée**, T11 reste `PARTIAL` sur ce seul point |
| **D10** | « Je ne vois pas ce qui est bloquant » — et la couverture est faible | D10 n'est **pas** un blocage de code : c'est une dette de *preuve*. La couverture devient un chantier | Deux métriques adoptées ; couverture **étendue au codegen** (première mesure : 87,04 % lignes / 86,05 % régions) ; plan §7 |
| **D11** | OK | Comptes corrigés + collisions d'identifiants documentées | **Appliqué** : `effects.md` 9→10, `features.md` FEAT-12.5 6→8, `Q-GRAM-11` (voir §8) |
| **D13** | « Pas compris, sois plus explicite » | Rien tant que tu n'as pas choisi | Explication refaite avec deux programmes concrets : §5 |

## 2. Ce qui a été appliqué tout de suite (D11)

Trois incohérences mesurées dans `spec/` :

1. `effects.md` disait « Neuf groupes » et « les 9 groupes » alors que son propre
   tableau en liste **dix** (`io, fs, alloc, panic, nondet, rt, net, unsafe, ffi,
   async`), comme `docs/architecture/RAVUM-CORE-DESIGN.md` §3.6 et l'enum
   `Effect` de `facts.rs`. → **corrigé en 10** aux deux endroits.
2. `features.md` FEAT-12.5 énumérait **six** lignes `experimental` alors que
   **huit** lignes du tableau portent ce statut. → les deux manquantes
   (`F-STD-031`, `F-META-006`) sont ajoutées à la liste.
3. `grammar.md` : **deux** règles distinctes portent `G-5.11` (le type `!` et la
   règle `const_expr`) et **deux** portent `G-6.40` (le bloc `unsafe` et le
   raccourci de champ en pattern). → **documenté**, pas renuméroté :
   `Q-GRAM-11` explique les deux collisions, et `tools/must_audit.py` matérialise
   exactement cet écart (807 énoncés pour 805 identifiants uniques).

Vérifié après coup : `must_audit.py` → 807 énoncés / 805 identifiants / 463
atteignables (inchangé) ; `features_coverage.py --strict` → 188/188.

## 3. D3 — contexte, provenance et impacts de la fusion

**Contexte.** Trois branches comptent :
`main` = la référence documentaire du dépôt (`ca675497`, décision de licence) ;
`dev` = branche d'intégration ; `feat/lang-core-1.0-rc` = la branche du langage,
100 commits, tête `0c2ceb8`, PR [#101](https://github.com/Rwanbt/Ravum/pull/101)
en draft vers `dev`.

**Provenance — ce qui atterrit exactement.** Fusion de la PR #101 dans `dev`
(commit de fusion, **aucun squash**, aucune réécriture : l'historique des neuf
passes reste lisible), puis fusion de `dev` dans `main`. Les preuves sont liées
aux SHA : code mesuré `21cf7d4`, documentation `de6f0de`/`7665244`/`0c2ceb8`,
CI **14/14 verte** sur `0c2ceb8` (7 `push` + 7 `pull_request`). Rien n'est
poussé sur `main` sans passer par une PR : la protection de branche exige le
check `mvp`.

**Impacts.**
- `main` contient désormais un compilateur **DRAFT ratifiable**, pas une release :
  aucun tag, aucun binaire publié, aucune signature.
- Les workflows qui tournent sur `push` s'exécutent aussi sur `main` : la CI
  devient la vitrine de la branche de référence.
- `dev` et `main` sont alignées après l'opération ; la branche de travail reste
  ouverte pour la suite (les prochains commits iront sur une nouvelle branche ou
  sur `feat/lang-core-1.0-rc` puis PR).
- **Rollback** : un seul commit de fusion à annuler (`git revert -m 1 <sha>`),
  et la branche d'origine est toujours là. Aucune perte possible.
- **Rien d'irréversible** n'est fait : pas de protection de branche modifiée, pas
  de force-push, pas de suppression de branche.

## 4. D4 — ce qui bloque, et ce que je te demande

**Ce qui est bloqué, exactement :** le **label**. `T4` (audit externe) et `T12`
(Go/No-Go) restent `BLOCKED`, et aucune phrase du type « production ready » ne
peut être écrite avant une revue humaine indépendante. **Rien d'autre** : le code,
les tests, la CI, la fusion, le durcissement, la couverture, la FFI avancent
normalement.

**Pourquoi un LLM ne suffit pas** : trois modèles qui lisent le dépôt produisent
des constats utiles, mais aucun n'endosse la responsabilité d'une revue de
sûreté ; ils peuvent affirmer sans preuve, et ils n'ont pas exécuté le
compilateur sur des cas adversariaux. C'est la différence entre « une relecture »
et « un audit ».

**Ce que je fais de tes trois revues** (même procédure que les cinq revues de la
passe 6, qui avaient trouvé 11 faux verts) :

1. verser les textes bruts dans `docs/audit/2026-11-reviews/` (modèle, date,
   prompt) — **c'est ce que j'attends de toi** ;
2. extraire chaque constat en une ligne vérifiable ;
3. le vérifier indépendamment (programme minimal, commande, SHA) ;
4. classer : **CONFIRMÉ** → correctif + test ; **FAUX** → noté avec la preuve ;
   **NON VÉRIFIABLE** → documenté ;
5. publier le tableau de triage et les correctifs.

**Sans les textes**, je ne peux pas les trier — et je n'inventerai pas leurs
constats.

## 5. D13 — la question, expliquée avec des programmes

**Le sujet en une phrase** : aujourd'hui, un `&str` rangé dans un tableau (ou un
champ) ne peut pas en ressortir dans une variable ou un `return`, alors que la
spécification dit qu'un `&str` est une valeur `Copy` que l'on peut stocker et
retourner.

**Ce qui marche et ce qui ne marche pas aujourd'hui** :

```rv
fn compare(a: [&str; 2]) -> bool {
    return a[1] == "one";          // ACCEPTÉ : comparaison, dans l'instruction
}

fn copy_out(a: [&str; 2]) -> &str {
    return a[1];                   // REFUSÉ : E-OWN-REF-ESCAPE
}

fn main() {
    let a = ["zero", "one"];
    let s = a[1];                  // REFUSÉ aussi
    let t = "one";                 // ACCEPTÉ : un littéral
    return_ok(t);                  // ACCEPTÉ : copie d'un littéral local
}
```

**Pourquoi c'est refusé** : dans Core 1.0, un `&str` qui sort d'une variable est
supposé pointer vers une donnée **statique** (un littéral). Le vérificateur
d'emprunts n'a aucune règle qui prouve qu'un `&str` extrait d'un tableau ou d'un
champ reste statique ; il applique donc une règle conservatrice : « seule une
chaîne littérale (ou une copie de littéral local) peut sortir ». La conséquence
est une **limitation d'ergonomie**, pas un trou de sûreté : aucun programme
existant n'est refusé (9/9 examples, 0 FAIL de conformance).

**Les deux options, en clair :**

- **Option A — accepter.** On décide que tout `&str` contenu dans un agrégat est
  statique par construction, et on autorise `return a[1];`. Effet immédiat :
  `copy_out` et `let s = a[1];` compilent. Ce que ça coûte : la sûreté du langage
  repose sur une **hypothèse** (« aucun `&str` non statique n'entre dans un
  agrégat »). Elle est vraie aujourd'hui (pas de lifetimes, FFI de chaînes
  interdite), mais une extension future (FFI renvoyant une chaîne C, lifetimes)
  la casse silencieusement. Il faudrait donc financer un **traçage de la
  staticité** à travers index, champs, patterns et payloads, plus ses tests —
  plusieurs jours de travail sémantique.
- **Option B — garder le refus.** On ne change pas le comportement, et on
  **réécrit la phrase de la spec** pour qu'elle dise ce qui est vrai : la
  staticité ne se propage qu'aux littéraux et aux copies de littéraux, pas à
  travers les agrégats (limitation documentée, question ouverte `Q-OWN`). Coût :
  un texte + deux ou trois cas négatifs qui verrouillent la formulation. Effet
  pour l'utilisateur : `a[1]` dans une variable reste refusé, avec un message qui
  renvoie à la limitation documentée.

**Ma recommandation : B**, parce que c'est sûr, que rien n'est bloqué aujourd'hui,
et que A engage la sûreté du modèle pour un gain d'ergonomie. Si tu préfères A,
ce n'est pas un correctif mais un **choix de modèle** : il faut le financer comme
tel (traçage de staticité + tests d'invariant).

**Ce que je te demande** : `D13: A` ou `D13: B`.

## 6. D6 — périmètre gelé de Core 1.0

**Inclus et mesuré** : le sous-ensemble documenté par `spec/language/` — types
primitifs, tuples, tableaux, `String`/`Vec`, structs/enums, `match`, boucles,
`test fn`, effets/profils `core`/`pure`/`rt`, stdlib `core`/`io`/`fs`/`string`/
`vec`/`math`, modules, FFI minimale (`extern "C"` + `unsafe`, signatures
primitives), cible native et `wasm32-wasip1`, CLI (`check`/`build`/`run`/`test`/
`fmt`/`lint`/`doc`/`lsp`/`doctor`/`cache`/`watch`).

**Explicitement hors périmètre** (`UNSUPPORTED`, refusé avec un code stable) :
threads, réseau, async, GPU/WebGPU, mobile, hard-RT/WCET certifié, hot reload
complet (seul le *preview* fail-closed existe), macros, traits, génériques
avancés, paquets externes.

**Toute extension est une version 1.x**, pas une modification de 1.0.

## 7. D10 — traçabilité et couverture, deux choses différentes

**Traçabilité (D10)** : « chaque règle normative est-elle reliée à un cas qui la
satisfait ? » Deux mesures : **188/188 FeatureIds** (des fonctionnalités) et
**807 énoncés MUST** dont **463** seulement ont un cas (344 sans). Ce n'est pas
un blocage de code : c'est une dette de preuve. Décision : deux métriques
officielles + plan écrit pour les 344. Le plan consiste à décider, pour chaque
énoncé sans cas, entre « il reçoit un cas » et « il est hors périmètre, avec
justification écrite ».

**Couverture (ta remarque)** : « le code est-il exercé par les tests ? » La
mesure ne portait **que sur `sema`** — c'était le vrai trou : le **codegen**, qui
est la partie qui génère du Rust, n'était pas mesuré du tout. Première mesure
faite : **87,04 % lignes / 86,05 % régions** sur `ravum-lang-codegen`
(`emit.rs` 87,02 % / 86,02 %, `prelude.rs` 100 %, `lib.rs` 76,92 % / 66,67 %).
Le plan : mesurer tous les crates à logique, poser des seuils ratchet par crate,
remonter `check.rs` (80,26 % pour un seuil à 80), et compléter par la
**mutation** — qui est la mesure honnête pour un compilateur, parce qu'un `match`
géant peut être « couvert » sans être testé.

**Rien de tout cela ne bloque l'avancement** : ce sont des mesures à publier et
des seuils à tenir, pas des portes fermées.

## 8. Il ne reste que deux choses de ton côté

| Quoi | Pourquoi | Format |
|---|---|---|
| Les **trois revues** (Claude, ChatGPT, DeepSeek) | Pour les trier, vérifier chaque constat et corriger ce qui est réel | fichiers, texte collé, ou liens |
| **D13 : A ou B** | Pour clore la contradiction TYP-6.3 / OWN-6.2 | `D13: A` ou `D13: B` |

Tout le reste est appliqué ou planifié dans
[`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md).

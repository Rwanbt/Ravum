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
| **D3** | **Fusion dans `main` autorisée**, aucune release | `main` devient la référence documentaire du travail ; `dev` d'intégration reste alignée | **Exécutée le 2026-11-12** : `dev` = `a2b8db5` (PR #101), `main` = `94f3ac4` (PR #104), commits de fusion sans squash ; **aucun tag, aucune release, aucune signature** (§3) |
| **D4** | Quatre revues LLM fournies ; ne doit pas bloquer le code | Rien dans D4 n'empêche de coder, tester, fusionner, durcir | **Triées** : docs/audit/2026-11-reviews/TRIAGE.md — 16 constats, verdicts CONFIRMÉ / FAUX (preuve) / HISTORIQUE. Aucun blocage de code ; le label attend un audit **humain** |
| **D5** | Windows + Linux ; puis « mets les runners `macos-latest` » | Tier 1 = Windows + Linux ; macOS devient **Tier 2** avec son job | `.github/workflows/macos-qual.yml` ajouté (`macos-latest`, arm64 : tests workspace + conformance build-vérifiée) ; **non requis**, promotion après 30 jours verts |
| **D6** | OK (geler le périmètre) | Le sous-ensemble documenté devient le périmètre Core 1.0 ; extensions = 1.x | Périmètre gelé (§6) |
| **D7** | « Rust prouve bien la mémoire, la solution existe » | La FFI devient un **programme de durcissement en 4 étages** | Confinement `trusted` → effets déclarés sur l'`extern` → wrappers sûrs → isolation (processus/WASM) ; [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §5 |
| **D9** | Pas d'intérêt tant que le langage n'est pas prêt pour la prod | Signature de release **différée** ; aucun mécanisme bloqué | Statut `DEFERRED`, **aucune clé générée**, T11 reste `PARTIAL` sur ce seul point |
| **D10** | « Je vise une couverture de 100 % » | Objectif **100 % lignes et régions** inscrit dans le ratchet | `coverage_report.py --gap` liste les lignes non couvertes : **11 fichiers, ~2 100 lignes** au départ ; les seuils montent à 100 fichier par fichier |
| **D11** | OK | Comptes corrigés + collisions d'identifiants documentées | **Appliqué** : `effects.md` 9→10, `features.md` FEAT-12.5 6→8, `Q-GRAM-11` (voir §8) |
| **D13** | « Une preuve déterministe ? A et B me semblent bancales » | **Réponse : oui, ni A ni B** | `docs/rfc/RFC-D13-STATICITY.md` : règle d'origine + règle de stockage ⇒ staticité **prouvée**, pas supposée. **Décision 2026-11-12 : « 2- ok » → implémentée** (voir §5, issue) |

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
(commit de fusion `a2b8db5`, **aucun squash**, aucune réécriture : l'historique
des neuf passes reste lisible), puis fusion de `dev` dans `main` (PR #104,
commit de fusion `94f3ac4`). Les preuves sont liées aux SHA : code mesuré
`21cf7d4`, documentation `de6f0de`/`7665244`/`0c2ceb8`, spécification et
couverture `12c4a38`/`d046f68`, cible production `599df1b` ; CI **14/14 verte**
sur `599df1b` et **tous les checks verts** sur la PR #104 (`mvp` compris). Rien
n'est poussé sur `main` sans passer par une PR : la protection de branche exige
le check `mvp`.

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

> **Issue (2026-11-12)** : réponse du responsable « 2 - ok » → implémentation
> faite. La solution retenue est **la preuve** (règle d'origine + règle de
> stockage), pas l'hypothèse A ni le refus B. `return a[1]` et `let s = a[1];`
> sont désormais **acceptés**, `String::as_str()` reste refusé. Mesures et
> fichiers : [`RFC-D13-STATICITY.md`](../rfc/RFC-D13-STATICITY.md) §6. Les
> paragraphes ci-dessous sont l'explication d'origine, conservée comme dossier.

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

**Ma recommandation était : B**, parce que c'est sûr, que rien n'est bloqué
aujourd'hui, et que A engageait la sûreté du modèle pour un gain d'ergonomie. La
décision du 2026-11-12 est allée plus loin : **ni A ni B, mais la preuve** — la
règle de stockage déjà outillée transforme l'hypothèse de A en invariant
vérifié, donc le refus de B n'est plus nécessaire. Ce n'était pas un correctif
mais bien un travail sémantique, financé comme tel (voir RFC §6).

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

## 8. Ce qui reste à faire, et ce qui attend encore ta main

**Fait dans cette passe** : D11 corrigé ; couverture étendue au backend ;
objectif 100 % inscrit ; job macOS ajouté ; revue multi-IA triée ; programme FFI
en 4 étages ; conception D13 déterministe.

**Réponses du 2026-11-12** (les deux points de la version précédente) :

| Quoi | Réponse | Effet |
|---|---|---|
| **Exemption `F-META-006`** | « 1 - ok » | `approved_by` = responsable, date `2026-11-12`, réexamen au plus tard le `2027-02-12` ; `features_coverage.py --strict` reste 188/188 |
| **D13 : implémentation** | « 2 - ok » | Implémentée (règle d'origine dans `own.rs`/`typeck.rs`, positions de type dans le backend) ; 6 cas de conformance, 20 tests, corpus 462/461/0/1, `build_verified: true` |

Il ne reste donc **aucun point en attente de ta main** sur ce document. Les
suites (couverture 100 %, D7, mutation) sont suivies dans
[`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md).

**À surveiller** : le premier run de `macos-qual.yml` (Tier 2) et le premier run
de `mutation.yml` depuis `main`.

Tout le reste est appliqué ou planifié dans
[`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) et
[`docs/audit/2026-11-reviews/TRIAGE.md`](../audit/2026-11-reviews/TRIAGE.md).

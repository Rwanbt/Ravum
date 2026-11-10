# RFC — Backend natif (voie C) et ratification du bootstrap

> **Statut : `PROPOSED`** · **Date : 2026-11-10** · **Portée : Ravum Core 1.0**
> **Décision liée : D2** (`docs/roadmap/CORE-1.0-GATE-MATRIX.md` §4)
> **Ce document ne ratifie rien.** Il prépare une décision humaine. Aucun ADR
> n'est modifié, aucune fonctionnalité n'est autorisée par sa seule existence.

## 1. Problème

Le gate **T1** est `PARTIAL` parce que la chaîne réellement utilisée par
`build`/`run`/`test` est la **voie B** (frontend Ravum → faits typés → Rust
généré auditable → `rustc`), que la **voie A** (Rust + outillage Ravum) reste une
référence opérationnelle, et que la **voie C** (frontend Ravum → IR propre →
Cranelift) **n'existe pas**. `NOT_IMPLEMENTED` n'est pas `PASS` : M5 ne peut pas
être vert tant que D2 n'est pas tranchée.

La question n'est pas « faut-il un backend natif ? » mais « **quelle preuve
manque, et à quel coût la produit-on** ».

## 2. Ce qui est mesuré aujourd'hui (état réel)

Déclaré avec l'environnement, conformément à [`../architecture/PROJECT_INVARIANTS.md`](../architecture/PROJECT_INVARIANTS.md) :

| Élément | État mesuré | Où |
|---|---|---|
| Chaîne utilisée par `build`/`run`/`test` | voie B, réelle (émet du Rust, `rustc` l'exécute) | `docs/roadmap/CORE-1.0-GATE-MATRIX.md` T1 |
| Voie A | opérationnelle (référence) | idem |
| Parité A/B | `MATCH` sur 4 programmes du corpus gelé `fixtures/m5/` | `evidence/m5-*.json`, `tools/m5-bench` |
| Voie C | **absente** | idem |
| Reproductibilité du binaire | bit-à-bit identique entre deux racines et deux `target/` (`SHA-256 AAD66766…`, Windows 11 x86-64, rustc 1.95.0) | `docs/qualification/REPRODUCIBILITY.md`, `scripts/repro-check.ps1` |
| Conformance | 346 cas, 0 FAIL (voir passe finale) | `docs/qualification/T2-FINDINGS.md` |

**Aucune performance de la voie C n'est mesurée**, puisqu'elle n'existe pas. Toute
RFC qui citerait un gain de performance de la voie C inventerait une mesure.

## 3. Alternatives

### Option 1 — Ratifier la voie B comme backend de Core 1.0

- **Contenu** : documenter que Core 1.0 cible `rustc` comme backend, que la voie C
  est hors périmètre de 1.0, et que M5-change est réévalué pour 1.x.
- **Coût** : rédaction d'un ADR (une page) + mise à jour T1/M5 dans la matrice de
  gates. Aucun code.
- **Risque** : la dépendance à `rustc` reste une dépendance de chaîne (version,
  drapeaux, ABI interne non utilisée) ; l'objectif « langage universel autonome »
  est repoussé, mais rien n'est cassé.
- **Falsification** : si un consommateur exige un binaire sans `rustc` sur la
  machine cible, ou si la sémantique Ravum ne peut plus être abaissée vers Rust
  sans perte (cas déjà rencontré : motifs flottants, G-6.36 — résolu par un
  abaissement en garde, D-PAT-1), l'option devient insuffisante.

### Option 2 — Pilote voie C borné (sous-ensemble gelé), voie B par défaut

- **Contenu** : définir un IR minimal + un chemin Cranelift pour un sous-ensemble
  **gelé et déclaré** (par exemple : `i32`/`i64`/`f64`, `if`/`while`/`match`
  entier, fonctions, appels, `io::println`), et une **parité A/B/C** mesurée sur
  ce sous-ensemble uniquement. Tout le reste continue de passer par la voie B.
- **Coût** : le plus élevé des trois options en effort d'ingénierie ; dépendance
  à Cranelift ; un second chemin d'émission à qualifier (conformance, mutation,
  couverture, Miri/fuzz ne couvrent pas du Cranelift) ; risque de divergence
  sémantique entre deux backends.
- **Risque** : deux backends = deux vérités. Sans un **oracle de parité
  automatique** (même programme, même sortie, mêmes codes d'erreur), le sous-ensemble
  dérive silencieusement.
- **Prérequis non négociable si cette option est retenue** :
  1. le sous-ensemble est **déclaré dans la spec** (`features.md`) et sa
     non-couverture est un refus explicite, pas un plantage ;
  2. la parité est **un test du corpus** (mode `run-pass` comparant les deux
     binaires), pas un tableau de bord manuel ;
  3. aucune règle MUST n'est satisfaite par la seule voie C tant que la parité
     n'est pas mesurée.

### Option 3 — Voie C complète

- **Contenu** : backend natif pour tout Core 1.0.
- **Coût** : réécriture de la cible d'émission et de toute la preuve (conformance,
  couverture, mutation, Miri, fuzz, repro, SBOM) sur un second chemin.
- **Verdict de cette RFC** : **non recommandé à court terme**. Le risque n'est pas
  technique, il est **épistémique** : la totalité des preuves actuelles porte sur
  la voie B ; les dupliquer sur C sans oracle de parité produirait un second
  système non prouvé présenté comme équivalent.

### Option 4 — Statu quo explicite (`DEFERRED`)

- **Contenu** : garder T1 `PARTIAL`, M5 rouge, et écrire noir sur blanc que la
  voie C est hors périmètre 1.0 jusqu'à décision.
- **Coût** : nul en code. Le gate reste honnêtement incomplet.
- **Risque** : un `PARTIAL` permanent finit par être lu comme un `PASS` implicite
  si la matrice n'est pas relue. Mitigation : la ligne T1 nomme D2.

## 4. Recommandation (non contraignante)

**Option 1 + Option 4 combinées** : ratifier la voie B pour Core 1.0, consigner la
voie C comme hors périmètre 1.0, et **rouvrir D2 en 1.x** avec un critère
d'ouverture écrit à l'avance — par exemple : « un consommateur hors de la machine
de développement exige un binaire produit sans `rustc` », ou « une règle MUST de
la spec devient inexprimable en Rust généré ». La voie C devient alors une
réponse à un besoin daté, avec son propre gate, et non un symbole de complétude.

## 5. Ce qu'un humain doit décider

1. Option retenue (1, 2, 3 ou 4) et **date de réexamen**.
2. Si option 2 : le **sous-ensemble gelé** exact et le propriétaire de l'oracle de
   parité.
3. Si option 1/4 : la formulation exacte de la ligne T1 de la matrice de gates.

## 6. Ce que cette RFC ne fait pas

- Elle ne ratifie aucun ADR et ne crée aucun numéro ADR.
- Elle ne prétend pas que la voie B est « définitive ».
- Elle ne cite aucune performance de la voie C.

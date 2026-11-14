# Décisions humaines encore ouvertes — dossier unique (Core 1.0)

> **Statut : `PROPOSED` — dossier d'instruction. Aucune décision n'est prise ici.**
> Ce document rassemble, en un seul endroit, les décisions qui **n'appartiennent
> pas aux agents** : ratification, audit indépendant, clé de signature,
> dénominateurs de preuve, contrats de plateforme, périmètres et texte du mandat.
> Il ne remplace ni [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
> (registre des gates), ni
> [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) (définition de
> la cible), ni `DECISIONS_REQUIRED.md` §0 (décisions déjà prises) — il les
> **consolide** et cite leurs preuves.
>
> **État vérifié le 2026-11-14 à 18:50 (Europe/Paris)**, branche
> `goal/consolidation-2`, HEAD **`6b6edd7`**, arbre de travail **sale** : d'autres
> agents écrivaient et ont commité pendant la rédaction (passage de `214bf5e` à
> `6b6edd7`, puis apparition de fichiers non suivis — dont la politique de
> dépréciation et le contrat d'installateur Linux, voir §3.1 constat C9). Les
> chiffres et les états de ce dossier sont donc **datés** : ils doivent être
> re-vérifiés avant toute décision.
>
> **Aucun `PROVED`, aucune maturité revendiquée.** Les statuts employés sont
> `PENDING_HUMAN_APPROVAL`, `PARTIAL`, `NOT_RUN`, `À VÉRIFIER`, `PROPOSED`.

## 0. Comment lire ce dossier

1. **Une fiche = un acte de décision.** Chaque fiche donne : la question exacte,
   deux ou trois options réelles avec leur conséquence, la preuve ou le document
   de référence, le décideur, ce qui est bloqué tant que ce n'est pas tranché, et
   une recommandation argumentée **ou** « pas de recommandation » quand le
   dossier est insuffisant.
2. **Règle de preuve.** Une affirmation est soit vérifiée par une commande
   exécutée dans cette passe (« **mesuré ici** », commandes en §3.2), soit lue
   dans un fichier du dépôt, soit **citée** avec son document et sa section
   (« **cité :** »). Un chiffre cité n'est pas re-mesuré ici et le dit.
3. **Aucune estimation de durée, aucun chiffre inventé.** Quand un chiffre attendu
   est introuvable, il est marqué `À VÉRIFIER` et l'écart est expliqué en §3.1.
4. **Une recommandation n'est pas une décision.** Elle est proposée par un agent,
   argumentée sur les preuves disponibles ; le responsable peut la refuser sans
   motivation.

## 1. Tableau de synthèse

| ID | Décision (question en une ligne) | Décideur | Bloque une RC interne | Bloque la revendication production | Reco |
|---|---|---|---|---|---|
| `AGENTS.md` | Approuver la révision 2026-11-13 du mandat ? | responsable | non | non | approuver |
| `D2` | Ratifier le socle normatif (ADR + spec) et le contrat de backend ? | responsable | non | **oui** (R1) | ratifier par lots |
| `D2-VOIE-C` | Voie C : ratifier la voie B pour 1.0 et consigner la voie C hors périmètre ? | responsable | non | partiel (T1/M5) | option 1+4 |
| `D11` | Renuméroter les deux `rule_id` dupliqués ou entériner la collision ? | responsable | non | partiel (R1) | entériner |
| `D7` | Ratifier la frontière FFI, et que faire de `i128`/`u128` ? | responsable | non | **oui** (R5/T6) | restreindre la surface |
| `RFC-OWN-FACTS-1` | Approuver `OwnFacts`, et l'étape C (suppression du `MovePlanner`) ? | responsable | non | non | A oui, C différée |
| `RFC-RT-TRAPS-1` | Ratifier la règle des traps `rt` ? | responsable | non | **oui** (T6) | ratifier le §2 complet |
| `D-2026-11-14-3` | Ratifier l'amendement T6 (`--rt-entry` + indexation bornée) ? | responsable | non | **oui** (règle `rt`) | ratifier avec limites |
| `D-2026-11-14-2` | Accepter `NodeId` u64 / 65 536 fichiers et requalifier `modules/file-limit` ? | responsable | non | partiel (contrat) | accepter |
| `RFC-NODE-ID` | Ratifier la règle « aucun cache indexé par `FileId` » ? | responsable | non | non | ratifier après mise à jour |
| `RFC-RV-DSP-BLOCK-1` | Ratifier l'ABI de bloc généré et autoriser la phase 8 ? | responsable | non | partiel (T8/T9) | pas de reco sur l'option |
| `D-2026-11-14-1` | Quelle méthode pour le test de puissance T2, et quand la campagne nominale ? | responsable | **oui** (dossier T2) | **oui** (T2) | substitut + nominal d'abord |
| `D10` | Dénominateur MUST (463/807), objectif 100 %, statut des lignes de test ? | responsable | **oui** (dossier T2) | **oui** (R3/R8/T2) | deux métriques ; objectif à reformuler |
| `D-2026-11-14-4` | Accepter la comparaison du Rust généré après normalisation des ids ? | responsable | non | partiel (T4 du GOAL) | requalifier le critère |
| `D-2026-11-14-5` | Cible LSP `p95 < 50 ms` non tenue à 5 110 lignes : que décide-t-on ? | responsable | non | **oui** (R9/P0-A) | accepter et publier l'écart |
| `D4` | Mandater l'audit humain indépendant (ou écrire une exemption) ? | responsable | non | **oui** (R13/T4) | mandater après rafraîchissement |
| `T12` | Go/No-Go humain sur le dossier de RC ? | responsable | **oui** | **oui** | pas de reco sur l'acte |
| `D9` | Custody de la clé de signature et publication d'`allowed_signers` ? | responsable + détenteur | non | non | ne pas rouvrir maintenant |
| `D-2026-11-14-7` | Ratifier la politique de dépréciation et le contrat installateur Linux (désormais écrits) ? | responsable | non | **oui** (R10/R11) | politique : oui ; contrat Linux après run CI |
| `D-2026-11-14-6` | Comment clore T6/T9 (threads/réseau/async, verticales) ? | responsable | non | **oui** (T6/T9) | périmètre écrit |
| `RFC-DEV-TOOLS-1` | Autoriser Miri et `cargo-mutants` en local ? | responsable | non | non | autoriser |

**Lecture rapide.** Une seule décision bloque l'**acte** de RC : `T12` (le
Go/No-Go). Deux décisions bloquent la **cohérence du dossier** de RC parce
qu'elles changent le verdict T2 (critère d'acceptation non satisfait) : la
méthode du test de puissance (`D-2026-11-14-1`) et le dénominateur de preuve
(`D10`). Le reste bloque la **revendication production**, une **release publiée**,
ou la **stabilité d'un contrat**.

## 2. Fiches

### A. Mandat

#### `AGENTS.md` — Approuver la révision 2026-11-13 du mandat

- **Question exacte** : la révision du `2026-11-13` de `AGENTS.md`, qui se
  déclare `PENDING_HUMAN_APPROVAL` et corrige des faits périmés (« le langage
  n'est plus à venir », fusion du 2026-11-12, cible D2 « production ready »),
  est-elle approuvée ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Approuver telle quelle | Le mandat en vigueur décrit l'état réel ; aucune obligation de mission n'est ajoutée ni retirée (la révision le déclare elle-même) |
  | B | Approuver avec modificatifs | Le texte reste `PENDING_HUMAN_APPROVAL` jusqu'à la version corrigée ; les agents continuent de lire un mandat non en vigueur |
  | C | Refuser | Le mandat de référence garde la phrase fausse « pas encore un compilateur fonctionnel » et l'état périmé |

- **Preuve / référence** : en-tête de [`AGENTS.md`](../../AGENTS.md) lignes 3-6
  (statut et contenu de la révision) ; faits corrigés cités dans le corps (fusion
  `main` du 2026-11-12 PR #101 → `dev`, PR #104 → `main`) ; enregistrement dans
  `DECISIONS_REQUIRED.md` §0 (D3) ; commit `fac8a8c` cité par
  [`GOAL-PRODUCTION-READINESS-REPORT.md`](GOAL-PRODUCTION-READINESS-REPORT.md) §5.1.
- **Décideur** : le responsable (c'est son mandat).
- **Bloqué tant que ce n'est pas tranché** : rien techniquement ; l'ambiguïté est
  de gouvernance — un document de référence qui se déclare non en vigueur tout en
  étant le seul mandat écrit.
- **Recommandation** : **A**. La révision ne fait que corriger des faits datés
  (elle le dit explicitement) et n'ajoute aucune obligation ; la refuser
  conserverait une phrase reconnue fausse depuis le 2026-11-12.

### B. Contrats de plateforme et de langage

#### `D-2026-11-14-2` — `NodeId` u64 / plafond 65 536 + requalification de `modules/file-limit`

- **Question exacte** : (Q1) `NodeId = u64` (32 bits de `FileId`, 32 bits de
  compteur par fichier) et `MAX_SOURCE_FILES` 200 → **65 536** sont-ils acceptés
  comme nouveau **contrat de plateforme** ? (Q2) le cas de conformance
  `modules/file-limit` peut-il passer de `compile-fail` à `compile-pass` (201
  sources désormais acceptées, le refus restant couvert par un test unitaire à
  limite abaissée localement) ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Accepter Q1 et Q2 | Le layout 32/32 et la limite 65 536 deviennent contractuels ; le cas reste, requalifié |
  | B | Refuser Q1 (revenir à 8 bits / 200 fichiers) | Le test « paquet de 1 000 modules » et la table de session LSP perdent leur base ; le marqueur synthétique `0xFE` entrerait en collision dès 254 fichiers (constat du rapport §4) |
  | C | Accepter le layout, garder un plafond plus bas (à écrire) | Nouveau contrat à documenter, tests de limite à rejouer, gain du test 1 000 modules perdu au-delà du plafond |

- **Preuve / référence** : **mesuré ici** — `crates/ravum-lang-syntax/src/ast.rs:20`
  (`pub type NodeId = u64;`), `crates/ravum-lang-syntax/src/parser.rs:77`
  (`next: (u64::from(file) << 32) | 1`), `crates/ravum-lang-sema/src/loader.rs:72`
  (`pub const MAX_SOURCE_FILES: usize = 65_536;`) ; `conformance/cases/modules/file-limit/case.json`
  lu (`mode: compile-pass`, notes datées, 201 fichiers sources comptés).
  **Cité :** [`GOAL-CONSOLIDATION-2-REPORT.md`](GOAL-CONSOLIDATION-2-REPORT.md) §4
  (changement, tests ajoutés, 475 paquets examinés / 232 produisent du Rust) et
  §9.1-§9.2 (les deux points `PENDING_HUMAN_APPROVAL`).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : le caractère contractuel du layout
  d'identifiants et de la limite de fichiers ; la clôture de l'item T4 du GOAL
  (`PARTIAL`) ; la règle de cache de `RFC-NODE-ID` s'appuie sur ce layout.
- **Recommandation** : **A**. Le changement est implémenté et testé (paquet de
  1 000 modules, test unitaire de refus `E-LIMIT-FILES`), l'ancien layout était
  structurellement plafonné, et le cas de conformance n'est pas supprimé mais
  requalifié avec sa note. **À faire avant la décision** : mettre à jour les deux
  RFC qui décrivent encore un layout 8/24 bits (constat C4 en §3.1).

#### `RFC-NODE-ID` — Ratifier la règle « aucun cache indexé par `FileId` »

- **Question exacte** : le modèle d'identité du §3 et la règle de cache du §4
  (cache indexé par `(chemin, hash de contenu)`, jamais par `FileId` ; résultats
  obsolètes jamais publiés, INV-07 ; références inter-fichiers par chemin de
  définition ; faits rechargés revalidés) sont-ils ratifiés — et confirme-t-on
  qu'**aucune migration `NodeId`** n'est attendue en Core 1.0 ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier §3 et §4 après mise à jour du texte | Règle en vigueur pour les futurs caches ; un cache indexé par `FileId` devient une violation écrite |
  | B | Ratifier §4 seule, garder §3 informatif | La règle de cache est opposable ; la description du layout reste non contractuelle |
  | C | Ne rien ratifier | Un futur cache peut être indexé par `FileId` et attribuer des faits au mauvais fichier après un ajout (risque déjà identifié §2) |

- **Preuve / référence** : [`RFC-NODE-ID.md`](../rfc/RFC-NODE-ID.md) §1-§4, §7 ;
  test de stabilité cité 2/2 par
  [`GOAL-PRODUCTION-READINESS-REPORT.md`](GOAL-PRODUCTION-READINESS-REPORT.md)
  phase 6. **Constat vérifié ici** : le §1 et le §3 du RFC décrivent `NodeId`
  comme un **`u32` avec 8 bits de `FileId` et 24 bits de compteur**, alors que le
  code est `u64` 32/32 depuis le 2026-11-14 (voir fiche `D-2026-11-14-2`) ; le
  même fait périmé figure dans `RFC-OWN-FACTS.md` §2.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : les caches futurs (LSP incrémental)
  n'ont pas de règle ratifiée ; la phrase « `NodeId` ne sort pas du processus »
  n'est qu'une recommandation de RFC.
- **Recommandation** : **A**, mais **pas avant** la correction du layout dans le
  texte : ratifier un document qui décrit un contrat faux créerait une
  contradiction documentaire vérifiable. La règle du §4, elle, coûte zéro
  changement de code et ferme un risque réel.

#### `RFC-RT-TRAPS-1` — Ratifier la règle des traps du profil `rt`

- **Question exacte** : ratifier le §2 (indexation, division/reste, `+ - *`,
  négation, décalages produisent l'effet `panic` donc `E-EFF-PROFILE` en `rt`,
  sauf formes exemptées ; flottants et `as` exemptés ; `wrapping_*`,
  `saturating_*`, `checked_*` comme formes explicites) — et trancher les trois
  points du §5 : (i) inclure **décalages** et **négation unaire**, (ii) confirmer
  que `pure` **ne change pas**, (iii) le sort de `wrapping_div`/`wrapping_rem`
  (qui paniquent encore sur un diviseur nul et ne sont donc **pas** des formes
  sûres).
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier le §2 complet (décalages + négation inclus, `pure` inchangé) | `rt` devient défendable sur les traps ; asymétrie `rt`/`pure` assumée par écrit |
  | B | Ratifier seulement les opérations de l'énoncé initial (`+ - *`, `/`, `%`, `[]`) | Décalages et négation restent des traps implicites ; la règle contredit `overflow-checks=on` |
  | C | Statu quo (option (a) de la RFC) | `rt` ne garantit rien sur les traps ; INV-05 reste violé |

- **Preuve / référence** : [`RFC-RT-TRAPS.md`](../rfc/RFC-RT-TRAPS.md) §2, §3, §5,
  §6 (5.1-5.3 implémentés : 10 tests unitaires, 5 cas `effects/rt-trap-*`) ;
  [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) T6 `PARTIAL` ;
  `PRODUCTION-READINESS.md` §5 et §9 (phase 5).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : T6, toute revendication sur le profil
  `rt`, la cohérence entre la spec et les builtins ajoutés.
- **Recommandation** : **A**. Omettre `*`, décalages ou négation laisserait des
  traps que le profil prétend fermer, alors que les formes explicites sont déjà
  implémentées et testées ; la RFC documente elle-même la restriction assumée sur
  l'indexation dynamique de `Vec`.

#### `D-2026-11-14-3` — Ratifier l'amendement T6 de `RFC-RT-TRAPS-1` (`--rt-entry`)

- **Question exacte** : ratifier l'amendement du 2026-11-14 — (i) les règles `rt`
  (effets interdits **et** traps) s'appliquent aux seules fonctions **atteignables**
  depuis une entrée désignée par `--rt-entry <module::fonction>`, (ii) deux formes
  d'indexation bornée sont reconnues (`for i in 0..K` avec `K` littéral ≤ taille ;
  `for i in 0..a.len()` sur tableau/tranche non réaffectée et non masquée), (iii)
  une entrée inconnue est refusée (`E-TOOL`, exit 1) — ou demander une variante ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier l'amendement tel que spécifié, limites du §7 consignées | L'option et la règle deviennent contractuelles ; `examples/dsp-gain-iir` compile en `rt` |
  | B | Ratifier en restreignant (n'accepter qu'une des deux formes bornées, à préciser) | Moins de surface à défendre, mais l'exemple DSP peut cesser de compiler selon la forme retirée |
  | C | Refuser le point d'entrée (retour au paquet entier) | `examples/dsp-gain-iir` ne compile plus en `rt` : l'exemple a été réécrit pour cette règle ; l'obstacle « `io` dans `main` » revient |

- **Preuve / référence** : [`RFC-RT-TRAPS.md`](../rfc/RFC-RT-TRAPS.md) §2
  (« Indexation bornée (amendement T6) », « Profil `rt` par point d'entrée »), §7
  (preuves, limites) ;
  [`GOAL-CONSOLIDATION-2-REPORT.md`](GOAL-CONSOLIDATION-2-REPORT.md) §T6 (statut
  `PENDING_HUMAN_APPROVAL`, 17 tests `traps`, 5 cas `effects/rt-entry-*`, 483
  cas / 482 PASS). **Mesuré ici** : `scripts/cli-flag-audit.ps1` déclare
  `--rt-entry` (ligne 50) avec son compagnon `--profile rt` (lignes 58-59) ;
  `examples/dsp-gain-iir` existe et contient un `.rv`.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : la règle `rt` reste non ratifiée
  alors que l'option est dans la surface CLI auditée ; aucune revendication `rt`.
- **Recommandation** : **A**, avec les limites du §7 reprises comme obligations
  écrites (entrée **déclarée** et non prouvée appelée depuis un callback audio ;
  atteignabilité syntaxique sans analyse d'alias ; `Vec` refusé ; méthodes d'`impl`
  non adressables). Ces limites sont mesurées ; les passer sous silence serait la
  seule faute possible ici.

#### `RFC-OWN-FACTS-1` — Approuver `OwnFacts`, et l'étape C (suppression du `MovePlanner`)

- **Question exacte** : (Q1) approuver l'ajout d'`OwnFacts` à `Facts` (interface
  gelée du design §4 : `use_kind` + `place` par `NodeId`) ? (Q2) approuver
  l'étape C — suppression du `MovePlanner`, de `is_copy` local et de
  `builtin_method_moves_receiver` au profit de la lecture des faits — maintenant,
  ou seulement après l'étape B complète et un diff octet pour octet ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Q1 + Q2 conditionnelle (B complète, ≤ 20 désaccords, Rust identique, diff justifié) | La classe P0-A peut être fermée par construction ; suppose que les exclusions de B (identité de liaison, récepteurs, arguments de référence) soient levées |
  | B | Q1 seulement, C différée | Le garde debug reste ; le `MovePlanner` reste la source du lowering ; aucun risque de régression de forme |
  | C | Refuser Q1 (retirer `OwnFacts`) | La duplication checker/backend revient ; la classe P0-A reste ouverte à chaque extension |

- **Preuve / référence** : [`RFC-OWN-FACTS.md`](../rfc/RFC-OWN-FACTS.md) §3
  (critères de sortie des trois étapes), §8 (A `TESTED`, B `PARTIAL`, C
  `NOT_RUN`) ; [`GOAL-PRODUCTION-READINESS-REPORT.md`](GOAL-PRODUCTION-READINESS-REPORT.md)
  §2 phase 4 et §6 (C exige le diff byte-à-byte) ;
  [`GOAL-CONSOLIDATION-2-REPORT.md`](GOAL-CONSOLIDATION-2-REPORT.md) §T7 (garde
  étendu aux trois exclusions, 0 désaccord sur 120 programmes — chiffre du
  rapport, non rejoué ici) ; audit d'entrée
  [`2026-11-14-architecture-audit.md`](../audit/2026-11-14-architecture-audit.md)
  §5 « DE » (« tant que DA n'existe pas, supprimer `MovePlanner` est un pari sans
  filet ») et Q2-2.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : Q2-2 ; la fermeture structurelle de
  P0-A ; rien dans le binaire courant (le planner reste la source du lowering).
- **Recommandation** : **B**. Q1 est additif et déjà en place ; C ne peut pas être
  jugée sur ses propres critères tant que B reste `PARTIAL` (trois exclusions
  documentées) et que le diff octet pour octet n'est pas disponible. Approuver C
  « en principe » aujourd'hui serait une autorisation sans critère mesurable.

#### `D7` — Ratifier la frontière FFI, et trancher `i128`/`u128`

- **Question exacte** : (Q1) ratifier la position de
  [`docs/ffi/SAFETY.md`](../ffi/SAFETY.md) — FFI **scalaire expérimentale**,
  liste de types fermée, refus hors `unsafe`, `pure`/`rt` refusés, TCB de la
  bibliothèque C écrite — ou la garder `OPEN` ? (Q2) `i128`/`u128` restent-ils à
  la frontière : test de largeur **par cible**, ou retrait de la liste admise ?
- **Options Q2**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Garder `i128`/`u128` + écrire un test de largeur par cible | La largeur devient prouvée par cible ; MSVC n'a pas `__int128`, donc le test devra probablement refuser sur cette cible |
  | B | Retirer `i128`/`u128` de la surface Core 1.0 (`E-FFI-TYPE`) | Surface plus étroite, contrat cohérent avec « ne promets pas ce qui n'est pas mesuré » ; cas de conformance à mettre à jour |
  | C | Statu quo (types admis, aucun test de largeur) | Un type traverse sans contrat de largeur : promesse non mesurée |

- **Preuve / référence** : [`SAFETY.md`](../ffi/SAFETY.md) §2 (liste admise,
  copies `ffi_safe`/`ast_ffi_safe`), **C-FFI-2** (« décision humaine, pas
  correctif d'agent »), §5 (« non mesuré : `i128`/`u128`/`isize`/`usize` à
  l'exécution »), §6 (quatre points qui bloquent la ratification, dont les
  contradictions `abi.md` ABI-2.5/ABI-3.3 vs G-3.10) ;
  [`RFC-D7-FFI-EFFECTS.md`](../rfc/RFC-D7-FFI-EFFECTS.md) ; matrice T6 `PARTIAL`.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : R5, D7, T6 ; le mot « FFI » dans
  toute documentation de garantie ; la phrase « frontière sûre ».
- **Recommandation** : Q2 → **B** (retirer `i128`/`u128`), sauf besoin réel
  documenté, auquel cas **A** avec un test par cible. Un type admis sans test de
  largeur est exactement le genre de promesse que le mandat interdit ; le besoin
  d'un entier 128 bits à la frontière C de Core 1.0 n'est documenté nulle part.
  Q1 → **pas de ratification avant** que les contradictions de spec
  (ABI-2.5/3.3 vs G-3.10) soient tranchées : ratifier un contrat dont la spec se
  contredit créerait un gate `PASS` de forme.

#### `RFC-RV-DSP-BLOCK-1` — Ratifier l'ABI de bloc généré et autoriser la phase 8

- **Question exacte** : ratifier l'ABI de bloc (wrapper `unsafe` généré et local,
  corps Ravum en Rust sûr, profil `rt` obligatoire, `#[repr(C)]` `DspState` z1/z2,
  manifeste ABI v2) et le découpage minimal du §7.3 — et décider séparément si le
  **digest de schéma d'état** (FNV-1a 64 + texte exporté) est une extension de
  protocole à retenir ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier l'ABI + autoriser l'implémentation (wrapper, `--emit dsp-block`, test de swap) | Relie le langage au hot swap DSP ; ajoute une surface `unsafe` générée et un couplage langage/hôte (le test de swap doit décider où il vit) |
  | B | Ratifier l'ABI, différer l'implémentation | Le contrat est écrit et opposable ; rien n'est codé ; T8/T9 restent `PARTIAL` |
  | C | Repousser en 1.x | Aucune verticale DSP `.rv` ; le hot swap reste réservé au Rust écrit à la main |

- **Preuve / référence** : [`RFC-RV-DSP-BLOCK.md`](../rfc/RFC-RV-DSP-BLOCK.md)
  §7.1 (ABI v2 réellement lue : `ravum_abi_version = 2`, `ravum_manifest_v1`,
  `ravum_process_block`, pas de digest de schéma), §7.2 (**tout `NOT_RUN`** :
  wrapper généré, `--emit dsp-block`, test de swap), §7.3 (découpage et point 4 :
  le digest ne doit pas bloquer l'ABI v2) ;
  [`GOAL-PRODUCTION-READINESS-REPORT.md`](GOAL-PRODUCTION-READINESS-REPORT.md)
  §2 phase 8 (`NOT_RUN`, aucun wrapper) ; matrice T8/T9 `PARTIAL` ; audit Q2-3
  (`Non pour l'ABI`).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : phase 8 (8.3/8.4) ; la revendication
  « le langage alimente le hot swap DSP » ; T8/T9.
- **Recommandation** : **pas de recommandation sur l'option** : le dossier est
  suffisant sur l'ABI existante (relue) mais la valeur produit d'une verticale
  `.rv` et l'effort du wrapper ne sont pas mesurés. **Recommandation de
  séquençage** : décider le digest **séparément** de l'ABI v2, comme le propose la
  RFC §7.3.4, sinon le désaccord sur l'empreinte bloque une ABI déjà consommée par
  l'hôte.

### C. Socle normatif et backend

#### `D2` — Ratifier le socle normatif et le contrat de backend

- **Question exacte** : (Q1) quel **périmètre exact** le responsable ratifie-t-il
  (`ADR-000…041` + `spec/language/` aujourd'hui `DRAFT`) ? (Q2) ratifie-t-il le
  contrat de backend de référence
  [`RFC-D2`](../rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md) P2.1–P2.4 (voie B,
  validation sémantique côté Ravum, pas d'IR, contrat testé) ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier en bloc (tous les ADR + toute la spec) | R1 fermable ; mais des ADR décrivent des surfaces non implémentées (ADR-018 backends natifs / voie C absente ; ADR-036 GPU et ADR-006 hard-RT déclarés `UNSUPPORTED` par D6) : un bloc ratifié devient un engagement |
  | B | Ratifier **par lots** : contrat de backend + règles MUST prouvées par la conformance ; garder le reste `PROPOSED` | R1 reste `PARTIAL` mais le socle utile devient contractuel ; exige une liste écrite et un critère d'ajout |
  | C | Ne rien ratifier | Toute sémantique reste « candidate » ; version `0.x` (P3.6.7) ; R1 `PARTIAL` |

- **Preuve / référence** : **mesuré ici** — `python tools/validate_repo_docs.py`
  → `PASS: 42 proposed ADRs`, et `docs/adr/` contient **42** fichiers
  (`ADR-000…ADR-041` : **le chiffre « 43 ADR » écrit dans `DECISIONS_REQUIRED.md`
  D2 et dans `PRODUCTION-READINESS.md` R1 n'est pas celui du dépôt** — constat
  C1). **Cité :** `spec/language/` `DRAFT` ;
  [`RFC-D2-REFERENCE-BACKEND-CONTRACT.md`](../rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md)
  §1 (P2.1-P2.4), §6 (C1-C6), §8 (recommandations) ;
  [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) T1 `PARTIAL`,
  T2 `PARTIAL` ; `PRODUCTION-READINESS.md` §2 R1 et §3 R1.
  **Constat de fraîcheur** : le §5.1 du RFC affirme que `--verify-build` n'existe
  pas — il existe et il est **bloquant** en CI (`linux-qual.yml:123`,
  `qualification-evidence.yml:91`, `scripts/evidence.ps1:79`, `--verify-build`
  dans `tools/conformance/main.rs:104`). Les six formes `check`-pass /
  `build`-fail du §5.2 sont fermées : cinq par la coercition (D-COERCE-1) et la
  sixième (`struct P { s: &str }` + `P { s: "x" }`) par le backend de D13 —
  **mesuré ici** par une sonde `check` + `build` + `run` en exit 0 (sortie `x`)
  avec le binaire déjà présent `target/debug/ravum-lang.exe`. La ratification doit
  donc porter sur les **principes** P2.1-P2.4 et sur C1-C6, pas sur l'état du RFC.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : R1 ; le mot « stable » pour toute
  règle ; la version `1.0.0` (P3.6.7) ; la liste des PARTIAL du dossier de RC.
- **Recommandation** : **B**. La RFC elle-même recommande de ratifier P2.1-P2.4
  et de financer C1-C4/C5 avant promotion ; depuis, C5 est outillé et bloquant
  (périmètre build 207/207, 124 vérifiés — instantané
  `evidence/current.json`). Ratifier les 42 ADR en bloc engagerait des ADR dont la
  surface est explicitement `UNSUPPORTED`, ce qui transformerait une cible en
  promesse.

#### `D2-VOIE-C` — Voie C et gate T1

- **Question exacte** : quelle option retenue pour la voie C (backend natif
  direct) : (1) ratifier la voie B comme backend de Core 1.0 et consigner la voie
  C hors périmètre 1.0 avec date de réexamen ; (2) pilote voie C borné avec oracle
  A/B/C ; (3) voie C complète ; (4) statu quo explicite `DEFERRED` ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | 1+4 | Voie B ratifiée, voie C hors périmètre 1.0, réouverture écrite pour 1.x | T1 reste `PARTIAL` tant que la ligne n'est pas reformulée ; M5 reste rouge honnêtement ; aucun coût de code |
  | 2 | Pilote borné + parité A/B/C testée | Deux backends = deux vérités ; exige un sous-ensemble déclaré dans `features.md` et un oracle de parité automatique |
  | 3 | Voie C complète | Dupliquer toute la preuve (conformance, couverture, mutation, Miri, fuzz, repro) sur un second chemin non prouvé |

- **Preuve / référence** : [`RFC-BACKEND-NATIF-VOIE-C.md`](../rfc/RFC-BACKEND-NATIF-VOIE-C.md)
  §2 (voie C absente, aucune performance mesurée), §3 (options et coûts), §4
  (recommandation 1+4), §5 (ce qu'un humain doit décider) ; matrice T1
  (`PARTIAL`, « décision humaine D2 »).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : T1 ne peut pas passer `PASS` ;
  M5 reste rouge ; toute formulation « binaire sans `rustc` ».
- **Recommandation** : **1+4**. Aucune performance de la voie C n'est mesurée
  (elle n'existe pas) et la totalité des preuves porte sur la voie B ; la RFC
  propose un critère de réouverture écrit (« un consommateur hors machine de
  développement exige un binaire sans `rustc` », ou « un MUST devient
  inexprimable en Rust généré ») qui vaut mieux qu'un symbole de complétude.

#### `D11` — Renuméroter les deux `rule_id` dupliqués ou entériner la collision

- **Question exacte** : les comptes de `effects.md` et `features.md` sont
  corrigés (10 groupes, 8 lignes `experimental`), mais deux `rule_id` restent
  dupliqués (`G-5.11`, `G-6.40`) et **documentés** (`Q-GRAM-11`) sans
  renumérotation. Renumérote-t-on (identifiant contractuel, Q-DIAG-4) ou
  entérine-t-on la collision par écrit ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Renuméroter | Identifiants contractuels modifiés (Q-DIAG-4) ; impact sur `features.md`, les cas et les tests qui citent le `rule_id` |
  | B | Entériner la collision par écrit | Aucun changement de contrat ; R1 doit alors se lire « aucune contradiction de sémantique connue, deux identifiants dupliqués documentés » |
  | C | Ne rien décider | La collision reste documentée mais non arbitrée ; R1 reste ambigu |

- **Preuve / référence** : `DECISIONS_REQUIRED.md` D11 ; matrice §4 ligne D11
  (« documentées mais pas renumérotées : la renumérotation reste une décision
  humaine ») ; `PRODUCTION-READINESS.md` §2 R1 (« `spec/` sans contradiction
  interne (D11) »).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : la formulation de R1 et donc une
  partie de D2.
- **Recommandation** : **B**. Renuméroter un identifiant exposé est un changement
  de contrat pour tout outil consommateur, pour un gain de forme ; la collision
  est déjà localisée, documentée et sans effet sémantique. En contrepartie, R1 doit
  être reformulé pour dire exactement ce qui est vrai.

### D. Preuves et verdicts

#### `D-2026-11-14-1` — Méthode du test de puissance T2 et campagne nominale

- **Question exacte** : (Q1) pour le test de puissance exigé par T2 (« annuler un
  par un les correctifs des 13 formes historiques et vérifier que le générateur
  retrouve la divergence sur 3 × 2 000 programmes »), accepte-t-on un **substitut**
  (désactivation d'un mécanisme, par exemple rendre `plan_slot_groups` inerte) ou
  exige-t-on des **commits de revert recréés** sur l'arbre courant ? (Q2) la
  **campagne nominale** `typed_oracle_long` (3 graines × 2 000, `#[ignore]`) est-elle
  rejouée d'abord, après, ou pas du tout ?
- **Options Q1**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Reverts recréés sur l'arbre courant (4 lots de correctifs) | Mesure la propriété visée ; mais le revert du correctif d'ancrage **entre en conflit** (`emit.rs`, `backend_audit.rs`), et un patch recréé ne correspond plus forcément au défaut d'origine : on mesurerait un autre arbre |
  | B | Substitut « mécanisme désactivé », protocole écrit, mêmes 13 formes, mêmes 3 × 2 000 | Mesure un mécanisme, pas « le commit annulé » — à déclarer comme tel ; le score devient comparable d'une passe à l'autre si le protocole est figé |
  | C | Statu quo (score `NOT_RUN`) | Le critère d'acceptation « score de puissance publié » reste non satisfait ; T2 reste `PARTIAL` pour cette raison |

- **Preuve / référence** : [`GOAL-CONSOLIDATION-2-REPORT.md`](GOAL-CONSOLIDATION-2-REPORT.md)
  §2 (tentative `git revert -n 21cf7d4` → deux conflits, worktree supprimé ; score
  exact **0/13**, `NOT_RUN`) et §9.3, §11.2, §11.4 ;
  [`T2-FINDINGS.md`](../qualification/T2-FINDINGS.md) §27.5 (mêmes faits, 5 des 13
  formes hors de la grammaire du générateur) ; **mesuré ici** :
  `crates/ravum-lang-codegen/tests/differential_oracle.rs:2570-2579`
  (`typed_oracle_long`, `#[ignore]`, `RAVUM_TYPED_PROGRAMS` défaut **2 000**,
  `RAVUM_TYPED_RELEASE_EVERY` défaut 10).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : T2 et R2 ; la clôture du dossier de
  RC sur ce point ; toute phrase « le générateur retrouve les formes historiques ».
- **Recommandation** : Q1 → **B**. Le revert est **mesuré** comme impossible sur
  l'arbre courant ; le reconstituer produirait un test dont on ne sait plus quelle
  version du code il mesure. Un substitut, s'il est scripté, comparable et déclaré
  comme « mécanisme désactivé » (et non « commit annulé »), donne un score
  reproductible. Q2 → **A** (nominal d'abord) : la campagne nominale établit
  l'état de référence avant qu'une expérience ne modifie l'arbre, et c'est l'ordre
  le moins ambigu ; le rapport laisse d'ailleurs la question ouverte (§11.4).

#### `D10` — Dénominateur MUST, objectif 100 %, statut des lignes de test

- **Question exacte** : (Q1) quel **dénominateur** pour la traçabilité : (a) deux
  métriques officielles (FeatureIds + MUST) avec obligation de traiter les 344
  non mappés ; (b) périmètre écrit excluant les 344 ; (c) tracer les 344 ?
  (Q2) l'objectif **100 % lignes ET régions** par fichier est-il maintenu, alors
  que les régions d'erreur `fmt::Error` des `impl Display` sont inatteignables par
  des tests ? (Q3) les lignes situées dans les modules `#[cfg(test)]` comptent-elles
  dans le dénominateur de l'objectif ?
- **Options Q1**

  | # | Option | Conséquence |
  |---|---|---|
  | a | Deux métriques officielles + plan écrit pour les 344 | T2 peut être jugé sur un critère explicite ; les 344 restent une dette nommée |
  | b | Déclarer les 344 hors périmètre, avec justification écrite | T2 fermable plus vite, mais le terme « traçabilité de la spécification » change de sens |
  | c | Tracer les 344 | Le plus coûteux ; aucun chiffre de faisabilité n'existe |

- **Options Q2**

  | # | Option | Conséquence |
  |---|---|---|
  | a | Exclusion de mesure ciblée et fermée des régions `fmt::Error` (`coverage_thresholds.json`) | L'objectif reste 100 % ailleurs ; une exclusion permanente est créée |
  | b | Reformuler l'objectif : 100 % lignes + seuils de régions par fichier, régions `fmt::Error` déclarées non mesurables | Objectif atteignable et honnête ; change un objectif déjà ratifié le 2026-11-11 |
  | c | Garder l'objectif tel quel | L'écart reste publié indéfiniment ; R8/T10 restent `PARTIAL` sans critère de sortie |

- **Options Q3** : (a) hors dénominateur (mesure produit seule, cf.
  `coverage_product_split.py`) ; (b) dans le dénominateur (gate sur l'export par
  instanciation) ; (c) publier les deux et ne pas trancher.

- **Preuve / référence** : **mesuré ici** — `python tools/must_audit.py` →
  **807** énoncés MUST, **463** atteignables (`463/807`), **344** non mappés ;
  `python tools/features_coverage.py --strict` → **188/188** (+1 exemption
  `F-META-006`) ; `python tools/coverage_report.py --input target/cov-all.json
  --check` → **exit 0**, `89,79 %` lignes / `89,23 %` régions, **10 fichiers sous
  l'objectif** ; `tools/coverage_thresholds.json` porte
  `"objective": {"lines": 100, "regions": 100, "decided_on": "2026-11-11"}`.
  **Cité :** [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §3 R8
  et §7 bis (2 098 lignes listées par `--gap`, **1 715** de code produit à
  **86,41 %**, **201** lignes dans `#[cfg(test)]`) ;
  [`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md) §2-§4
  (mesure `llvm-cov` des régions `write!` à `count=0`, trois options) et §5
  (re-mesure 89,79/89,23, `traps.rs` 97,61/97,13, `facts.rs` 100/100) ;
  `DECISIONS_REQUIRED.md` D10 ; `MUST-TRACEABILITY.md`.
  **Constat C3** : le chiffre « 201 lignes de test » dépend de l'export —
  `coverage_product_split.py --input target/cov-all.json` donne aujourd'hui
  **1 209** lignes produit non couvertes et **781** lignes dans `#[cfg(test)]`
  (`loader.rs` : 0 en produit, 570 en test), et **89,61 %** de code produit. Les
  deux vues ne sont pas superposables et ne doivent pas être additionnées.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : T2 (`PARTIAL`), R3, R8, T10 ; toute
  phrase « traçabilité complète » ou « couverture à 100 % ».
- **Recommandation** : Q1 → **a** (deux métriques + plan) : c'est déjà l'état de
  fait, et cette option évite de déclarer 344 énoncés hors périmètre sans base
  écrite. Q2 → **b**, avec la variante **a** si le responsable préfère rester au
  plus près de l'objectif ratifié : la mesure `llvm-cov` documentée montre que
  100 % de régions est inatteignable par des tests sur les `Display`, et un
  objectif inatteignable cesse d'être un critère. Q3 → **pas de recommandation** :
  le dossier est insuffisant (C3) — la règle de comptage (export par
  instanciation vs fusion, produit vs test) doit être tranchée avant de choisir.

#### `D-2026-11-14-4` — Déviation littérale T4 : comparaison du Rust généré

- **Question exacte** : le critère du GOAL T4 (« Rust généré identique **octet
  pour octet** sur `examples/` et tout le corpus ; toute différence = arrêt »)
  est-il requalifié en « identique après normalisation des **numéros
  d'identifiants dans les noms de symboles** » (`__rv_root_<id>`, `__rv_sel_<id>_<n>`,
  `rvm_ffi_<id>`), 15 fichiers sur 232 différant uniquement par ces numéros — ou
  exige-t-on l'identité littérale ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Requalifier le critère (normalisation écrite, liste des motifs) | Le critère devient mesurable et reproductible ; il ne compare plus des noms synthétiques |
  | B | Exiger l'identité littérale | Le changement `NodeId` u64 ne peut pas être déclaré conforme ; il faudrait un mapping stable d'identifiants dans les symboles (travail non planifié) |
  | C | Refuser le changement `NodeId` | Revient à la fiche `D-2026-11-14-2` option B, avec les mêmes conséquences |

- **Preuve / référence** : [`GOAL-CONSOLIDATION-2-REPORT.md`](GOAL-CONSOLIDATION-2-REPORT.md)
  §0.3 et §4 (475 paquets examinés, **232** produisent du Rust, **15** fichiers
  différents, **232/232** identiques après normalisation, 0 différence
  inexpliquée ; seul changement de statut : `modules/file-limit`, refusé →
  accepté). **Mesuré ici** : les noms de symboles embarquent bien un `NodeId` —
  `crates/ravum-lang-codegen/src/emit.rs:1916` (`id: NodeId`), `:1927`
  (`let __rv_root_{id} = …`), `:1961` (`__rv_sel_{id}_{level}`), `:1464`
  (`fn rvm_ffi_{}…`, `f.id`).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : le statut de l'item T4 du GOAL
  (`PARTIAL`) et l'acceptation de ce point dans le dossier de RC.
- **Recommandation** : **A**. La différence est une conséquence **directe** du
  layout 32/32 (le compteur par fichier apparaît dans le nom) ; la normalisation
  ne retire aucune vérification de compilation ou d'exécution, elle compare ce qui
  est censé être stable. Exiger l'identité littérale rendrait tout changement de
  layout intestable sans un mapping dont aucun besoin n'est démontré.

#### `D-2026-11-14-5` — Cible LSP à l'échelle (`p95 < 50 ms`)

- **Question exacte** : la cible « diagnostic `p95 < 50 ms` » n'est pas tenue à
  l'échelle (p95 **337,6 ms** à 5 110 lignes, mesure en **debug**) : accepte-t-on
  l'écart publié et garde-t-on la cible pour les petits paquets, reformule-t-on la
  cible (par taille de paquet), ou finance-t-on une ré-analyse par modules
  dépendants ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Accepter et publier l'écart ; garder la cible pour les petits paquets | Aucun code ; la cible reste un SLO non tenu à l'échelle, écrit comme tel |
  | B | Reformuler la cible par taille (p95 par tranche de lignes) | Change une cible (ADR-032, `PROPOSED`, « SLO exploratoire ») ; rend les futures mesures comparables |
  | C | Financer la ré-analyse incrémentale (modules dépendants), critère de l'audit Q2-1 (`p95 < 50 ms` à 20 000 lignes, **ou** écart publié) | Coût d'ingénierie réel ; aucun chiffre de faisabilité ni de gain n'est mesuré aujourd'hui |

- **Preuve / référence** : [`LANG-TIMINGS.md`](LANG-TIMINGS.md) §8 (`check` p50
  **364,9 ms** à 5 110 lignes, `own` 71,7 %) et §9 (LSP de bout en bout
  `p50 322,4 ms / p95 337,6 ms` à 5 110 lignes, cible « non tenue ») ;
  [`LSP.md`](../qualification/LSP.md) §7 ; `GOAL-CONSOLIDATION-2-REPORT.md` §3 ;
  `ADR-032` ligne 15 (SLO exploratoire `p95 < 50 ms`, ADR `PROPOSED`) ;
  audit d'entrée [`2026-11-14-architecture-audit.md`](../audit/2026-11-14-architecture-audit.md)
  Q2-1 (critère « `p95 < 50 ms` à 20 000 lignes, ou écart publié »).
  **Constat C2** : un chiffre de « **163,5 ms en release** » à 5 110 lignes a été
  avancé pour ce dossier ; **il n'existe pas dans le dépôt** (recherche `163,5` et
  `163.5` sur tous les `.md/.json/.rs/.ps1/.py` hors `target/` et
  `node_modules/` : **0 occurrence**). La seule mesure à l'échelle est celle de
  `LANG-TIMINGS.md` §9, en debug ; **aucune mesure à 20 000 lignes n'existe**.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : R9 ; toute phrase « P0-A tenue » ;
  les notes de version qui citeraient une latence d'échelle.
- **Recommandation** : **A** (accepter et publier l'écart), **pas de
  recommandation** sur C tant que la mesure à 20 000 lignes et une mesure en
  release n'existent pas. Argument : la cause est mesurée (analyse sémantique
  globale ; `own` porte 97,5 % du temps à 50 k lignes) et la cible actuelle est un
  SLO exploratoire d'un ADR `PROPOSED` ; reformuler (B) ou incrémenter (C) sont des
  choix de modèle qui doivent se fonder sur des courbes, pas sur une extrapolation.

#### `D4` — Mandater l'audit humain indépendant (ou écrire une exemption)

- **Question exacte** : mandate-t-on **maintenant** un audit humain indépendant
  (borrow checker, codegen/prélude, frontière FFI, loader/résolution, contrats de
  sortie, liste des trous), avec le périmètre et le critère de clôture proposés —
  ou écrit-on une **exemption** datée, ou diffère-t-on ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Mandater l'audit, périmètre P4.2, clôture P4.5 | T4/T10/T12 peuvent sortir de `BLOCKED` ; coût humain ; le brief doit être rafraîchi avant envoi (constat C6) |
  | B | Exemption écrite, datée, signée, avec la liste de ce qui n'est pas audité | P3.4.2 l'autorise explicitement pour une promotion ; le verdict reste plafonné et la revendication production n'est pas acquise |
  | C | Différer | T4, T10, T12 restent `BLOCKED` ; R4 et R13 restent `BLOCKED` ; aucun label production |

- **Preuve / référence** : [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
  T4/T12 `BLOCKED` ; [`D3-D4-D9-POLICIES.md`](../release/D3-D4-D9-POLICIES.md) §2
  (P4.1 indépendance, P4.2 périmètre, P4.3 méthode, P4.4 sortie, P4.5 clôture) ;
  [`AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md) et
  [`AUDIT-PROMPT.md`](../audit/AUDIT-PROMPT.md) ; triage des quatre revues LLM
  dans `docs/audit/2026-11-reviews/TRIAGE.md` (fichier existant), traité par
  `DECISIONS_REQUIRED.md` §0 (D4).
- **Décideur** : le responsable, qui nomme l'auditeur ; **seul un humain signe le
  verdict** (P4.1).
- **Bloqué tant que ce n'est pas tranché** : T4, T10, T12 ; R4, R13 ; la
  promotion vers `main` (P3.4.2) ; les mots « audité », « production ready ».
- **Recommandation** : **A**, après rafraîchissement du dossier : le brief contient
  des chiffres datés (323 cas, 1 020 tests, 386 cas, 187/187, 89 suites / 1 405
  passés) alors que HEAD mesure 483 cas et 188/188 (constat C6). Envoyer un brief
  faux à un auditeur indépendant affaiblirait la seule preuve que le mandat
  attend de lui.

#### `T12` — Go/No-Go humain sur le dossier de RC

- **Question exacte** : prononce-t-on un Go sur le dossier de release candidate
  interne (verdict plafonné `CORE_RC_CANDIDATE`, non publié, lié à un SHA, sans
  tag ni release), avec la **liste explicite** des gates `PARTIAL`/`BLOCKED`
  acceptés et de leur conséquence pour l'utilisateur — ou un No-Go conditionnel,
  ou un report ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Go `CORE_RC_CANDIDATE` avec liste des PARTIAL/BLOCKED acceptés | Un dossier de RC interne existe, non publié, non signé ; T4/T12 ne deviennent pas verts pour autant |
  | B | No-Go conditionnel (prérequis listés : `D-2026-11-14-1`, `D10`, `D-2026-11-14-2`) | Aucun label RC ; les prérequis deviennent la feuille de route de la preuve |
  | C | Report indéfini | Rien ne change ; le dossier reste `BLOCKED` |

- **Preuve / référence** : [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
  T12 (« dossier de RC interne complet ; absents : dogfooding indépendant et
  Go/No-Go humain ; verdict plafonné à `CORE_RC_CANDIDATE` ») ;
  [`D3-D4-D9-POLICIES.md`](../release/D3-D4-D9-POLICIES.md) P3.7 (RC interne
  autorisée avant tout le reste, non publiée) et P3.4.5 (lister les PARTIAL
  acceptés) ; `GOAL-PRODUCTION-READINESS-REPORT.md` §7.2.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : le label RC, tout tag, toute release ;
  la phrase « release candidate ».
- **Recommandation** : **pas de recommandation sur l'acte** (Go/No-Go est un acte
  de responsabilité, pas une évaluation d'agent). **Recommandation de
  séquençage** : ne pas prononcer de Go avant que `D-2026-11-14-1` et `D10` soient
  tranchées — sinon la liste des PARTIAL acceptés serait ambiguë sur le point
  précis qui fait foi (T2), et le dossier serait incohérent avec lui-même.

### E. Distribution et interfaces

#### `D9` — Custody de la clé de signature et publication d'`allowed_signers`

- **Question exacte** : où vit la clé privée (A : poste du mainteneur hors ligne ;
  B : machine dédiée air-gap ; C : token FIDO ; D : KMS/HSM), qui peut signer, et
  **où la liste `allowed_signers` fait-elle foi** (dépôt seul, ou copie hors
  bande) ?
- **Options** (reprises de `KEY-CUSTODY.md` §2)

  | # | Option | Conséquence |
  |---|---|---|
  | A | Clé hors ligne sur le poste du mainteneur | Simple ; sécurité = celle du poste ; pas de séparation de rôle |
  | B | Machine dédiée air-gap (+ clé de secours A) | Isole la clé du poste de développement ; procédure manuelle ; exige un second poste |
  | C | Token matériel | Clé non exportable ; support `ssh-keygen -Y sign` avec token **à qualifier** avant de retenir |
  | D | KMS/HSM cloud | Rotation et audit natifs ; dépendance externe, contraire à l'esprit « pas de réseau » des preuves actuelles |

- **Preuve / référence** : `DECISIONS_REQUIRED.md` D9 (différée le 2026-11-11 :
  « sans intérêt avant la mise en production ») ; matrice §4 ligne D9 (« REPORTÉE »)
  et T11 `PARTIAL` ; [`SIGNATURES.md`](../qualification/SIGNATURES.md) (mécanisme
  qualifié : artefact intact accepté, artefact altéré **rejeté** avec exit 1 ;
  **aucune clé de projet**) ; [`KEY-CUSTODY.md`](../release/KEY-CUSTODY.md) §2-§5 ;
  [`D3-D4-D9-POLICIES.md`](../release/D3-D4-D9-POLICIES.md) §3 (P9.1-P9.7).
  **Mesuré ici** : aucun fichier `allowed_signers*` n'existe dans le dépôt ;
  `scripts/attest-release.ps1` existe.
- **Décideur** : le responsable ; un **détenteur de clé nommé** (P9.6) — un agent
  ne détient pas la clé et ne signe pas.
- **Bloqué tant que ce n'est pas tranché** : R12 ; T11 (`PARTIAL` → `PASS`) ; toute
  release **publiée** présentée comme signée, et toute revendication de provenance
  vérifiable.
- **Recommandation** : **pas de recommandation de réouverture maintenant** — le
  responsable a explicitement différé cette décision le 2026-11-11, et une clé
  générée pour l'occasion produirait une signature décorative. Le jour où une
  release publiée est décidée, le dossier propose **B + clé de secours A**, avec
  `allowed_signers` publié **et** copié hors bande (P9.4).

#### `D-2026-11-14-7` — Ratifier la politique de dépréciation et le contrat installateur Linux

- **Question exacte** : les deux documents **produits pendant la rédaction de ce
  dossier** (non suivis à HEAD `6b6edd7`, voir C9) sont-ils ratifiés ?
  (Q1) [`DEPRECATION-POLICY.md`](../release/DEPRECATION-POLICY.md) —
  `PROPOSED` · `PENDING_HUMAN_APPROVAL`, 7 interfaces publiques, règles
  `DEP-2.1`…`DEP-3.4`, **registre de dépréciations vide**, **aucun mécanisme
  d'avertissement dans le code**. (Q2) [`INSTALL-LINUX.md`](../release/INSTALL-LINUX.md)
  — `PROPOSED` « candidate, non ratifiée », périmètre **Linux x86-64 glibc sur
  `ubuntu-latest`** (musl, ARM64, macOS `NOT_RUN`), contrat **`NOT_RUN` en local
  et `PENDING` en CI** (l'étape qui l'exécute est ajoutée au job `install` de
  `linux-qual.yml`, aucun run n'existe encore).
- **Options Q1**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier la politique telle quelle | Les règles `DEP-*` deviennent opposables (gel du registre append-only, cycle annonce/avertissement/délai, qui décide) ; **R10 ne se ferme pas pour autant** — la politique le dit elle-même et les quatre autres exigences de R10 (CLI = contrat, JSON versionné, registre figé, LSP testé) restent |
  | B | Ratifier avec amendements (préciser lesquels, par ex. le canal d'avertissement §3.3, proposé mais non implémenté) | Politique opposable et adossée à un mécanisme réel ; demande une seconde revue |
  | C | Renvoyer / différer | R10 reste `PARTIAL` ; toute release publiée promet des interfaces sans règle de retrait |

- **Options Q2**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Ratifier **après un run CI vert** du contrat élargi | Le contrat devient opposable **et** exercé ; c'est la seule forme qui ne transforme pas un `PENDING` en `PASS` |
  | B | Ratifier maintenant, en acceptant `PENDING` explicitement | Contrat écrit et opposable ; l'exécution reste non prouvée, ce qui doit être dit dans la documentation de release |
  | C | Renvoyer / différer | R11 reste `PARTIAL` ; Linux installé mais non contractuel |

- **Preuve / référence** : **mesuré ici** — les deux fichiers existent sur disque
  (horodatages `18:47:28` et `18:46:11`) mais **ne sont pas dans `HEAD`** :
  `git cat-file -e HEAD:docs/release/DEPRECATION-POLICY.md` et
  `HEAD:docs/release/INSTALL-LINUX.md` échouent, comme pour
  `scripts/install-contract.sh` ; `.github/workflows/linux-qual.yml` est
  **modifié non commité** et ajoute l'étape « Public installer contract ».
  Lecture des deux en-têtes : statuts `PROPOSED` · `PENDING_HUMAN_APPROVAL`,
  `NOT_RUN` local / `PENDING` CI pour le contrat Linux. **Cité :**
  `PRODUCTION-READINESS.md` R10 et R11 ; `D3-D4-D9-POLICIES.md` P3.6.4 (une
  plateforme sans test d'installation n'est pas annoncée). **Constat C8** :
  `PRODUCTION-READINESS.md` §3 R11 dit « pas de contrat d'installateur public »
  pour Linux, `GOAL-PRODUCTION-READINESS-REPORT.md` §4.2 T11 dit « contrat
  installateur Linux vert au round 19 » ; `INSTALL-LINUX.md` tranche la lecture :
  le run vert `43562818570` prouve l'installateur, **pas** le contrat élargi.
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : R10, R11 ; T10/T11 si la release
  annonce Linux ; les notes de version (interface « stable » sans politique de
  retrait ratifiée).
- **Recommandation** : Q1 → **A** (ratifier telle quelle) : le document ne change
  aucun comportement, son registre est vide, et il nomme lui-même ce qu'il ne
  ferme pas — le laisser `PROPOSED` perpétuerait l'absence de règle de retrait
  que R10 reproche. Q2 → **A** (ratifier **après** un run CI vert) : ratifier un
  contrat `PENDING` reproduirait exactement le défaut que le dépôt combat
  ailleurs (un `PASS` de forme sur une preuve non exécutée) ; le coût est un run,
  pas une décision supplémentaire.

### F. Périmètre et outils

#### `D-2026-11-14-6` — Comment clore T6 et T9

- **Question exacte** : (a) déclare-t-on explicitement le périmètre — threads,
  réseau, async et temps réel dur `UNSUPPORTED` par la spec, T9 = les exemples
  `.rv` existants sans HTTP/SQLite/DSP hôte — en gardant `PARTIAL` jusqu'à un
  `PASS` motivé ; (b) ouvre-t-on une extension de périmètre ; (c) tente-t-on de
  fermer T6/T9 en 1.0 ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | a | Périmètre écrit, gates maintenus `PARTIAL` | Honnête, aucun code ; les gates nomment ce qui manque |
  | b | Extension de périmètre (réseau/stockage/DSP) | Contredit le gel D6 (extension = 1.x) ; nouveau chantier de spec et de refus |
  | c | Fermer T6/T9 en 1.0 | Contredit D6 et la spec (`UNSUPPORTED`) ; conduirait à un `PASS` de forme |

- **Preuve / référence** : [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
  §2 (T6 `PARTIAL` « threads, réseau, async explicitement `UNSUPPORTED` » ; T9
  `PARTIAL` « HTTP + SQLite, DSP hôte, E2E navigateur absents ») et §3 T6/T9 ;
  `DECISIONS_REQUIRED.md` §0 (D6 : périmètre gelé, extension = 1.x) ;
  [`RFC-P2-VERTICALS-BLOCKERS.md`](../rfc/RFC-P2-VERTICALS-BLOCKERS.md)
  (`PROPOSED`, blocages mesurés) ; `RFC-PERIMETRE-UNIVERSEL-D6.md`.
  **Mesuré ici** : `examples/` contient **10** répertoires avec au moins un `.rv`
  (`hello`, `fib`, `calculator`, `file-tool`, `stats`, `dsp-gain-iir`,
  `package-demo`, `tests-demo`, `ffi-abs`, `file-store`). **Constat C7** : la
  matrice §2 dit « 10 verticales », sa §3 en liste 9 (elle omet `file-store`,
  pourtant cité §2 comme le magasin clé/valeur).
- **Décideur** : le responsable.
- **Bloqué tant que ce n'est pas tranché** : T6, T9 ; les mots « applications
  réelles » et « runtime » ; toute phrase sur le réseau ou le stockage.
- **Recommandation** : **a**, et corriger le comptage de la matrice (§3 T9) pour
  qu'il dise 10 avec `file-store` nommé. Argument : D6 est déjà tranchée et la
  spec déclare ces surfaces `UNSUPPORTED` — un `PARTIAL` documenté est l'état
  exact ; ouvrir (b) est un acte 1.x qui n'appartient pas à ce dossier.

#### `RFC-DEV-TOOLS-1` — Autoriser Miri et `cargo-mutants` en local

- **Question exacte** : autorise-t-on l'installation **locale**, pour la
  vérification seulement, du composant `miri` (toolchain nightly déjà présente)
  et de `cargo-mutants` **version 27.1.0** (celle de la CI), afin de fermer les
  deux `NOT_RUN` locaux (Miri, mutation) sans dépendre d'un push ?
- **Options**

  | # | Option | Conséquence |
  |---|---|---|
  | A | Autoriser les deux, versions épinglées | Deux qualifications locales rejouables sur la machine de référence ; aucun impact produit |
  | B | Refuser | Miri et mutation restent `NOT_RUN` en local ; le rejeu exige un push, que l'agent n'a pas le droit de faire |
  | C | Autoriser Miri seulement | Ferme un `NOT_RUN` ; la mutation reste tributaire de la CI |

- **Preuve / référence** : [`RFC-DEV-TOOLS.md`](../rfc/RFC-DEV-TOOLS.md) §1-§5 ;
  **mesuré ici** : `cargo mutants --version` → `error: no such command: mutants` ;
  `rustup component list --toolchain nightly-x86_64-pc-windows-msvc --installed`
  → ni `miri` ni `rust-src` (la toolchain nightly existe pourtant) ; les preuves
  CI existent (Miri : `878e212`, 65 + 134 + fuzz court ; mutation : `b5298db`,
  183/150/24/9) — chiffres cités, non rejoués ici.
- **Décideur** : le responsable (R5 du GOAL : tout outil ajouté exige une RFC et
  une approbation).
- **Bloqué tant que ce n'est pas tranché** : deux `NOT_RUN` locaux ; le rejeu de la
  campagne mutation sans push.
- **Recommandation** : **A**. Les deux outils sont déjà utilisés par la CI aux
  mêmes versions, n'entrent ni dans `Cargo.toml` ni dans l'artefact, et la
  réversibilité est documentée (`rustup component remove`, `cargo uninstall`) ;
  sans eux, toute re-qualification locale exige un push, ce qui contredit la
  contrainte imposée aux agents.

## 3. Constats de vérification

### 3.1 Écarts trouvés pendant la rédaction (à corriger ou à trancher)

| # | Constat | Preuve |
|---|---|---|
| **C1** | Les documents disent « **43** ADR `PROPOSED` » (`DECISIONS_REQUIRED.md` D2, `PRODUCTION-READINESS.md` R1) ; le dépôt en contient **42** (`ADR-000…ADR-041`) | **mesuré ici** : `python tools/validate_repo_docs.py` → `PASS: 42 proposed ADRs` ; 42 fichiers hors `README.md` |
| **C2** | Le chiffre « **163,5 ms en release** » à 5 110 lignes n'existe pas dans le dépôt ; la seule mesure à l'échelle est **337,6 ms p95 en debug** | **mesuré ici** : recherche `163,5`/`163.5` sur `.md/.json/.rs/.ps1/.py` hors `target/` et `node_modules/` → **0 occurrence** ; `LANG-TIMINGS.md` §9 et `LSP.md` §7 ne portent que la mesure debug |
| **C3** | Le nombre de « lignes de test » dépend de l'export : **201** (rapport, export `cov-batch1`) contre **781** (`coverage_product_split.py` sur `target/cov-all.json`) ; produit **1 715 / 86,41 %** contre **1 209 / 89,61 %** | **mesuré ici** : `coverage_product_split.py --input target/cov-all.json` ; **cité :** `PRODUCTION-READINESS.md` §7 bis |
| **C4** | `RFC-NODE-ID` §1/§3 et `RFC-OWN-FACTS` §2 décrivent un `NodeId` **`u32` 8/24 bits** ; le code est **`u64` 32/32** depuis le 2026-11-14 | **mesuré ici** : `ast.rs:20`, `parser.rs:77`, `loader.rs:72` contre les textes des deux RFC |
| **C5** | `RFC-D2` §5.1 affirme que `--verify-build` n'existe pas ; il existe et il est **bloquant** ; les six formes du §5.2 sont fermées | **mesuré ici** : `tools/conformance/main.rs:104`, `.github/workflows/linux-qual.yml:123`, `qualification-evidence.yml:91`, `scripts/evidence.ps1:79` ; fermetures citées : D-COERCE-1, D13 |
| **C6** | `AUDIT-BRIEF.md` porte des chiffres datés (323 cas, 1 020 tests, 386 cas, 187/187, 89 suites / 1 405 passés) alors que HEAD mesure 483 cas et 188/188 | **mesuré ici** : `evidence/current.json` (483 / 482 / 1 ; 188/188) et `must_audit`/`features_coverage` ; **cité :** `AUDIT-BRIEF.md` §3 |
| **C7** | Matrice T9 : §2 dit « 10 verticales », §3 en liste 9 (`file-store` omis) | **mesuré ici** : 10 répertoires de `examples/` contiennent un `.rv` ; **cité :** matrice §2/§3 |
| **C8** | T11 Linux : « pas de contrat d'installateur public » (`PRODUCTION-READINESS.md` §3) contre « contrat installateur Linux vert au round 19 » (`GOAL-PRODUCTION-READINESS-REPORT.md` §4.2) ; la CI n'exerçait que `--prefix`/`--version`/manifeste ; `docs/release/INSTALL-LINUX.md` (apparu à 18:46, **non suivi**) tranche : le run `43562818570` prouve l'installateur, pas le contrat élargi, qui est `PENDING` | **mesuré ici** : `.github/workflows/linux-qual.yml:254-264` + `git diff` du fichier ; **cité :** les deux documents maîtres et `INSTALL-LINUX.md` §1 |
| **C9** | Travaux concurrents : HEAD est passé de `214bf5e` à **`6b6edd7`** pendant la rédaction ; des fichiers non suivis apparaissent (`docs/release/DEPRECATION-POLICY.md`, `docs/release/INSTALL-LINUX.md`, `scripts/install-contract.sh`, `docs/guide/`, `docs/release/RELEASE-NOTES-0.1.0-DRAFT.md`, ce dossier) ; `README.md` et `.github/workflows/linux-qual.yml` sont modifiés non commités. **L'instantané date de 18:50** : d'autres modifications sont apparues pendant la rédaction (quatre `lib.rs` de crates `ravum-lang-*`, un cas `conformance/cases/types/ffi-str-rejected/`) — elles ne sont pas analysées ici | **mesuré ici** : `git log -1`, `git status --short` à 18:50 ; `git cat-file -e HEAD:docs/release/INSTALL-LINUX.md` → absent |
| **C10** | La politique de dépréciation **n'existait pas à 18:45** ; elle existe désormais (`docs/release/DEPRECATION-POLICY.md`, `PROPOSED`, non suivie) — la fiche `D-2026-11-14-7` a été réécrite en conséquence. `docs/release/INSTALL.md` reste « candidate, non ratifiée » | **mesuré ici** : recherche de fichiers à 18:45 (absente) puis à 18:50 (présente, 626 lignes) ; `INSTALL.md` §3 |
| **C11** | Les prémisses de `RFC-DEV-TOOLS-1` sont toujours vraies localement | **mesuré ici** : `cargo mutants --version` → commande absente ; composants nightly sans `miri`/`rust-src` |

Conséquence de **C9** : trois fiches doivent être relues **après** que les
travaux en cours sont commités et documentés — `D-2026-11-14-5` (R9/LSP), `D9`/`T11`
(artefacts) et `D-2026-11-14-7`. Un workflow de reproductibilité inter-runners et
une alarme de performance ont été commités dans `6b6edd7`, mais **aucun document
maître n'est encore mis à jour** et **aucun run n'est constaté dans ce dossier** ;
la politique de dépréciation et le contrat d'installateur Linux sont apparus
**non suivis** pendant la rédaction (C10). Le jour où ces preuves sont publiées,
les formulations R7/R9/R10/R11 de `PRODUCTION-READINESS.md` changeront.

### 3.2 Commandes exécutées dans cette passe

| Commande | Résultat réel |
|---|---|
| `git status --short` / `git log -1` / `git rev-parse --short HEAD` | branche `goal/consolidation-2`, HEAD `6b6edd7`, arbre sale (C9) ; second contrôle à 18:50 : `linux-qual.yml` et `README.md` modifiés, **7 entrées non suivies à cet instant** (d'autres sont apparues depuis) |
| `git cat-file -e HEAD:scripts/install-contract.sh` / `HEAD:docs/release/INSTALL-LINUX.md` | absent de `HEAD` (non suivi) |
| `python tools/must_audit.py` | 807 MUST, 463 mappés, **344** non mappés (5 avertissements de table : `core-machine.md:88`, `expressions.md:254`, `grammar.md:271/411/460`) |
| `python tools/features_coverage.py --strict` | **188/188**, 1 exemption `F-META-006` (réexamen 2027-02-12) |
| `python tools/validate_repo_docs.py` | `PASS: 42 proposed ADRs` |
| `python tools/coverage_report.py --input target/cov-all.json --check` | `exit 0`, **89,79 %** lignes / **89,23 %** régions, seuils tenus |
| `python tools/coverage_report.py --input target/cov-all.json --gap` | **10** fichiers sous l'objectif 100 %, totaux 19 087/21 257 |
| `python tools/coverage_product_split.py --input target/cov-all.json` | produit **1 209** non couvertes / 11 637 (**89,61 %**), test **781** |
| `cargo mutants --version` | `error: no such command: mutants` |
| `rustup component list --toolchain nightly-x86_64-pc-windows-msvc --installed` | nightly présent, **sans** `miri`, **sans** `rust-src` |
| lectures de code | `ast.rs:20`, `parser.rs:77`, `loader.rs:72`, `emit.rs:1464/1916/1927/1961`, `conformance/cases/modules/file-limit/case.json`, `differential_oracle.rs:2570-2579` |
| sonde `struct P { s: &str }` | `target/debug/ravum-lang.exe check` puis `build` puis exécution : **exit 0 / 0 / 0**, sortie `x` (forme RFC-D2 §5.2) |
| lectures de documents | `AGENTS.md`, `DECISIONS_REQUIRED.md` §0/D2-D13, `PRODUCTION-READINESS.md`, `CORE-1.0-GATE-MATRIX.md`, `GOAL-CONSOLIDATION-2-REPORT.md`, `GOAL-PRODUCTION-READINESS-REPORT.md`, `docs/rfc/*` (7 RFC), `docs/ffi/SAFETY.md`, `docs/qualification/{COVERAGE-100-OBJECTIVE,REPRODUCIBILITY,MUTATION-EQUIVALENCE,T2-FINDINGS,LANG-TIMINGS,LSP,SIGNATURES}.md`, `docs/release/{D3-D4-D9-POLICIES,KEY-CUSTODY,INSTALL}.md`, `docs/audit/{AUDIT-BRIEF,2026-11-14-architecture-audit}.md`, plus à 18:50 `docs/release/{DEPRECATION-POLICY,INSTALL-LINUX}.md` (en-têtes et sections) |
| recherches d'existence | aucun `allowed_signers*` ; `163,5`/`163.5` → 0 occurrence ; `docs/release/DEPRECATION-POLICY.md` absent à 18:45 puis présent à 18:50 (C10) ; `scripts/install-contract.sh` non suivi (C9) |

Aucune mesure de gate n'a été rejouée dans cette passe (pas de `cargo test`, pas
de conformance, pas de Miri, pas de mutation, pas de build cargo) : les chiffres
de gates sont ceux des documents et de `evidence/current.json`, datés par leur
SHA.

## 4. Ordre de décision recommandé

### 4.1 Ce qui bloque quoi

| Décision | Bloque une RC interne | Bloque la revendication « production » | Bloque une release publiée |
|---|---|---|---|
| `T12` | **oui** (c'est l'acte) | **oui** | **oui** |
| `D4` | non (P3.7) | **oui** (R13/T4, promotion `main`) | **oui** |
| `D-2026-11-14-1` | **oui** (verdict T2 du dossier) | **oui** (T2/R2) | **oui** |
| `D10` | **oui** (verdict T2, R8) | **oui** (R3/R8/T2) | **oui** |
| `D-2026-11-14-2` | non | partiel (contrat de plateforme) | **oui** (contrat publié) |
| `D2` | non | **oui** (R1) | **oui** (version 1.0.0) |
| `D9` | non | non | **oui** (artefact signé) |
| `D-2026-11-14-5` | non | **oui** (R9/P0-A à l'échelle) | **oui** (notes de version) |
| `D-2026-11-14-7` | non | **oui** (R10/R11) | **oui** |
| `D7` | non | **oui** (R5/T6) | **oui** |
| `D-2026-11-14-6` | non | **oui** (T6/T9) | **oui** |
| `RFC-RT-TRAPS-1`, `D-2026-11-14-3` | non | **oui** (règle `rt`) | **oui** |
| `D2-VOIE-C` | non | partiel (T1/M5) | **oui** (via `D2`, version 1.0.0) |
| `D-2026-11-14-4` | non | partiel (item T4 du GOAL) | non |
| `D11` | non | partiel (formulation R1) | partiel (spec 1.0) |
| `RFC-OWN-FACTS-1`, `RFC-NODE-ID`, `RFC-RV-DSP-BLOCK-1` | non | non / partiel (T8/T9) | non |
| `RFC-DEV-TOOLS-1`, `AGENTS.md` | non | non | non |

### 4.2 Ordre conseillé

1. **`AGENTS.md`** — aucun coût, conditionne le texte sous lequel tous les agents
   travaillent. Peut être tranché immédiatement.
2. **`D-2026-11-14-2`** (NodeId + `modules/file-limit`), puis **`RFC-NODE-ID`**
   après mise à jour du layout dans le texte, puis **`D-2026-11-14-4`**
   (comparaison normalisée). Ces trois décisions forment un bloc : elles portent
   sur le même changement et se contrediraient si elles étaient tranchées
   séparément.
3. **`D10` question Q1** (dénominateur MUST) **puis `D-2026-11-14-1`** (méthode du
   test de puissance, campagne nominale d'abord). L'ordre interne compte : Q1
   définit le critère, la méthode produit la preuve. Tant que ces deux décisions
   ne sont pas prises, le dossier de RC portera un T2 `PARTIAL` dont les deux
   raisons sont **de méthode**, pas de code.
4. **`D4`** (mandater l'audit, après rafraîchissement du brief) **puis `T12`**
   (Go/No-Go). C'est le passage à la RC : `D4` conditionne la revendication
   production et la promotion vers `main` ; `T12` est l'acte qui donne (ou refuse)
   le label `CORE_RC_CANDIDATE`.
5. **`D2`** et **`D2-VOIE-C`** — nécessaires à une version `1.0.0` et à un T1
   motivé, pas à une RC. `D11` doit être tranchée en même temps que `D2` (même
   phrase de R1).
6. **Décisions de périmètre et de contrat** : `RFC-RT-TRAPS-1` +
   `D-2026-11-14-3`, `RFC-OWN-FACTS-1`, `D7`, `D-2026-11-14-6`,
   `D-2026-11-14-5`, `D-2026-11-14-7` (politique de dépréciation : ratifiable
   tout de suite ; contrat Linux : après un run CI vert),
   `RFC-RV-DSP-BLOCK-1`. Elles conditionnent la **formulation** des notes de
   version et des guides, pas la construction du binaire. Elles peuvent être
   prises en parallèle.
7. **`D9`** — seulement quand une release **publiée** est décidée ; ne pas rouvrir
   avant (décision explicite du 2026-11-11).
8. **`RFC-DEV-TOOLS-1`** — indépendante et parallélisable ; elle ne débloque que
   deux rejeux locaux, mais elle supprime la dépendance à un push pour
   re-qualifier Miri et la mutation.

**En une phrase** : une RC interne n'attend que `T12` ; `D4` conditionne ce qu'on
peut en **dire** (jamais « production ready »), et le dossier ne sera **cohérent**
que si `D10-Q1` et `D-2026-11-14-1` sont tranchées d'abord ; tout le reste bloque
la **revendication production** ou une **release publiée**, pas la construction.

## 5. Ce que ce dossier ne décide pas

- **Aucune décision n'est prise ici.** Les recommandations sont des propositions
  d'agent ; elles n'engagent ni le responsable, ni un gate, ni un document maître.
- **Il ne rouvre pas les décisions déjà prises** : D1 (licence), D3 (fusion
  `main` sans release), D5 (cibles Tier 1), D6 (périmètre gelé), D8, D12, D13,
  D-PAT-1, D-CMP-1, D-UFCS-1, D-MOD-3.2, D-CONST-1, D-COERCE-1 restent closes.
- **Il ne décide pas** : le choix de l'auditeur nommé, le détenteur de la clé,
  la date d'une release, la fusion d'une PR, un tag, un numéro de version, la
  méthode de fusion, ni le contenu normatif d'une règle (les RFC sont proposées,
  pas amendées).
- **Il ne modifie aucun fichier maître** : `PRODUCTION-READINESS.md`,
  `CORE-1.0-GATE-MATRIX.md`, `LANGUAGE-CORE-STATUS.md`, `GOAL-*.md`,
  `DECISIONS_REQUIRED.md` et `AGENTS.md` restent inchangés. Les écarts de §3.1
  sont **signalés**, pas corrigés.
- **Il ne remplace pas** la matrice des gates (qui fait foi sur les états), ni
  `PRODUCTION-READINESS.md` (qui définit la cible), ni `DECISIONS_REQUIRED.md` §0
  (qui enregistre les décisions prises).
- **Il ne couvre pas** la promotion de macOS en Tier 1 (décision datée de D5,
  après 30 jours de CI verte — échéance non vérifiée ici), ni les décisions
  d'intégration/revue de PR, ni la priorisation produit (DSP, LSP, verticales),
  ni les arbitrages de coût.
- **Il ne donne aucune estimation de durée, de coût ou de probabilité** : aucune
  n'a été mesurée. Les chiffres cités sont liés à un SHA, une machine et
  `rustc 1.95.0` ; un chiffre sans son SHA ne vaut rien.
- **Il ne prétend pas que la liste est close** : les travaux concurrents du
  2026-11-14 (performance, reproductibilité inter-runners, guide utilisateur,
  notes de version) peuvent créer de nouvelles décisions ; la politique de
  dépréciation et le contrat d'installateur Linux, apparus **pendant** la
  rédaction, sont traités en `D-2026-11-14-7` mais restaient **non suivis** à
  HEAD `6b6edd7`. Ce dossier doit être revu quand ils seront commités.

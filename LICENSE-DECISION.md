# Décision de licence — `MIT OR Apache-2.0` (DÉCIDÉE)

> **Statut : `DECIDED` le 2026-11-09** — la licence **`MIT OR Apache-2.0`** est
> appliquée au workspace Ravum. Les fichiers [`LICENSE-MIT`](LICENSE-MIT) et
> [`LICENSE-APACHE`](LICENSE-APACHE) sont à la racine, et `license =
> "MIT OR Apache-2.0"` est déclaré dans `[workspace.package]` puis hérité par
> **les 22 membres** via `license.workspace = true`.
>
> **Origine de la décision** : décision du responsable du projet, appliquée sur
> `main` (`ca675497`) et `dev` (`fbaa506`, PR #103), puis répercutée sur la
> branche de développement du langage. Ce document **enregistre** la décision ;
> il ne la prend pas. Ce texte n'est **pas un avis juridique**.
>
> Les sections 2 et 3 ci-dessous conservent l'analyse qui a servi à instruire le
> choix : elles sont gardées pour la traçabilité, pas comme une question ouverte.

## 1. État constaté et vérifié (2026-11-09, branche `feat/lang-core-1.0-rc`)

| Constat | Vérification |
|---|---|
| Fichiers `LICENSE-MIT` et `LICENSE-APACHE` présents à la racine | `git ls-tree origin/dev` / racine du dépôt |
| Champ `license` dans `Cargo.toml` (workspace) | `license = "MIT OR Apache-2.0"` |
| Héritage par les **22** membres du workspace | `cargo metadata --locked` → `license = MIT OR Apache-2.0` pour chacun |
| Dépendances : 41 packages résolus | `cargo metadata --locked` / SBOM |
| Licences des dépendances : **toutes permissives** | `scripts/licence-audit.ps1 -Json` : `dependency_count: 41`, `denied: []`, `unlicensed: []` |

**Conséquence** : le code peut être utilisé, copié, modifié et redistribué selon
l'option retenue par le preneur (MIT **ou** Apache-2.0), sous réserve de
conserver les mentions de copyright et, pour Apache-2.0, le fichier `NOTICE`
s'il existe.

## 2. Contraintes découlant de l'option retenue

1. **Deux fichiers de licence** fournis : `LICENSE-MIT` et `LICENSE-APACHE`.
2. **Champs `license`** cohérents dans `Cargo.toml` — fait, vérifié par
   `cargo metadata`.
3. **Notices tierces** : les dépendances doivent être créditées (MIT et/ou
   Apache-2.0) ; `THIRD-PARTY-NOTICES` peut être dérivé du SBOM.
4. **Contributions** : à encadrer (DCO ou CLA) avant d'accepter du code externe.
5. **Marque** : la licence logicielle n'accorde aucun droit sur le nom
   « Ravum » ; une politique de marque reste distincte.
6. **Cohérence SBOM/provenance** : régénérer le SBOM après décision pour que
   `licenseDeclared` cesse d'être `NOASSERTION` pour les 22 membres.
7. **Pas de rétroactivité automatique** : la licence s'applique aux versions
   distribuées après la décision.

### A. Apache-2.0

| Aspect | Impact |
|---|---|
| Nature | permissive, avec concession de brevet explicite |
| Obligations | conserver copyright/licence ; fournir `NOTICE` si présent ; signaler les modifications |
| Brevets | concession expresse des contributeurs ; clause de représailles (litige brevet ⇒ fin de licence) |
| Copyleft | aucun sur les œuvres dérivées |
| Compatibilité dépendances | oui (MIT/Apache-2.0) ; compatible GPLv3/AGPLv3 |
| Distribution propriétaire | autorisée (avec notices) |
| Écosystème / contributions | très courant (Rust) ; rassurant pour les entreprises |
| Points d'attention | formalisme `NOTICE` ; certains juristes le jugent plus lourd que MIT |

### B. MIT

| Aspect | Impact |
|---|---|
| Nature | permissive minimale |
| Obligations | conserver copyright + texte de licence dans les copies/parties substantielles |
| Brevets | **aucune** concession explicite (silence) |
| Copyleft | aucun |
| Compatibilité dépendances | oui (MIT/Apache-2.0) |
| Distribution propriétaire | autorisée (avec notice) |
| Écosystème / contributions | adoption maximale, très simple |
| Points d'attention | pas de protection brevet ; peu de garanties explicites |

### C. MIT OR Apache-2.0 (double licence, au choix du preneur)

| Aspect | Impact |
|---|---|
| Nature | combinaison courante de l'écosystème Rust : le preneur choisit MIT **ou** Apache-2.0 |
| Obligations | les deux textes doivent être fournis ; le preneur respecte l'option qu'il retient |
| Brevets | protection si le preneur choisit Apache-2.0 |
| Copyleft | aucun |
| Compatibilité dépendances | oui |
| Distribution propriétaire | autorisée |
| Points d'attention | deux fichiers, communication plus complexe ; les contributions doivent être couvertes par les deux (accord contributeur) |

### D. MPL-2.0

| Aspect | Impact |
|---|---|
| Nature | copyleft **au niveau fichier** |
| Obligations | les fichiers MPL modifiés et distribués doivent rester MPL et leur source être disponible ; les fichiers ajoutés peuvent être sous autre licence |
| Brevets | concession explicite |
| Copyleft | limité aux fichiers MPL ; pas de contamination de l'œuvre plus large |
| Compatibilité dépendances | oui (dépendances permissives) ; compatible GPL/LGPL/AGPL |
| Distribution propriétaire | possible pour l'œuvre combinée, avec obligations sur les fichiers MPL |
| Écosystème / contributions | moins courant en Rust ; demande de la rigueur sur la frontière des fichiers |
| Points d'attention | gestion fichier par fichier ; `NOTICE`/source des fichiers modifiés |

### E. AGPL-3.0

| Aspect | Impact |
|---|---|
| Nature | copyleft fort, **incluant l'usage réseau** (§13) |
| Obligations | toute version modifiée utilisée via un service réseau doit offrir la source correspondante aux utilisateurs de ce service ; distributions ⇒ source complète |
| Brevets | concession explicite |
| Copyleft | fort sur l'ensemble dérivé |
| Compatibilité dépendances | oui (permissives compatibles AGPLv3) |
| Distribution propriétaire | **incompatible** avec une offre propriétaire du code Ravum lui-même |
| Écosystème / contributions | peut décourager l'adoption en entreprise et l'intégration dans des produits fermés |
| Points d'attention | définition du « réseau » et des fronteières du service ; obligations pour un futur hébergement SaaS |

### F. Propriétaire (tous droits réservés / licence commerciale)

| Aspect | Impact |
|---|---|
| Nature | aucune licence publique ; distribution sous contrat |
| Obligations | dépendances permissives : conserver copyright/notices ; ne pas modifier les fichiers MPL (aucune dépendance MPL constatée) |
| Brevets | selon le contrat commercial |
| Copyleft | aucun |
| Compatibilité dépendances | **oui** avec l'inventaire actuel (toutes permissives) |
| Distribution propriétaire | objet même de l'option |
| Écosystème / contributions | contributions externes à encadrer (CLA) ; pas de réutilisation communautaire ; incompatible avec un futur « registre public » de packages |
| Points d'attention | gouvernance, CLA, gestion des contributions, image du projet ; l'option « open source » serait fermée |

## 3. Contraintes à respecter quelle que soit l'option

1. **Décision humaine explicite**, enregistrée (ADR ou document de gouvernance) ; aucun agent
   ne peut la prendre ni la modifier en silence.
2. **Fichier `LICENSE`** à la racine (absent aujourd'hui) — sa création est un acte de
   décision, pas une tâche d'outillage.
3. **Champs `license`** dans `Cargo.toml` (workspace + crates) pour que `cargo metadata`, le
   SBOM (`licenseDeclared`) et les consommateurs soient cohérents. Aujourd'hui : `null`.
4. **Notices tierces** : les 41 dépendances doivent être créditées selon leurs licences
   respectives (généralement copyright + texte MIT et/ou Apache-2.0) ; un fichier
   `THIRD-PARTY-NOTICES` peut être dérivé du SBOM.
5. **Contributions** : définir comment les futures contributions sont couvertes (DCO ou CLA)
   avant d'accepter du code externe sous une licence choisie.
6. **Marque** : la licence logicielle n'accorde aucun droit sur le nom « Ravum » ; une
   politique de marque est distincte.
7. **Cohérence SBOM/provenance** : après décision, régénérer le SBOM (les 22 `NOASSERTION`
   deviendront la licence choisie) et mettre à jour `docs/release/`.
8. **Pas de rétroactivité automatique** : une licence s'applique aux versions publiées après
   la décision ; les copies déjà distribuées restent sous les termes applicables au moment de
   leur distribution.

## 4. Décision enregistrée

- [x] option retenue : **MIT OR Apache-2.0** (double licence, au choix du preneur) ;
- [x] décision enregistrée dans ce document et appliquée par PR #103 sur `dev` ;
- [x] `LICENSE-MIT` et `LICENSE-APACHE` à la racine ;
- [x] `license` renseigné dans `Cargo.toml` (workspace + 22 membres) ;
- [ ] `THIRD-PARTY-NOTICES` généré depuis le SBOM (`NOT_DONE`) ;
- [ ] `docs/release/SBOM.md`, `PROVENANCE.md`, `RELEASE-CHECKLIST.md` mis à jour
      pour la licence (`PARTIAL` : la section 7 de la checklist est réconciliée,
      le SBOM doit être régénéré) ;
- [ ] SBOM régénéré et `licenseDeclared` vérifié pour les 22 membres (`NOT_DONE`) ;
- [ ] contributions (DCO/CLA) et marque statuées (`PENDING_HUMAN`).

## 5. Statut

**`DECIDED`** — la licence `MIT OR Apache-2.0` s'applique. Ce qui reste ouvert est
**l'outillage de conformité** (notices tierces, régénération du SBOM) et la
**gouvernance des contributions**, pas le choix de licence.

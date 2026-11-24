# Décisions du responsable — 2026-11-24 (Ravum UI)

Décisions données en conversation le 2026-11-24, enregistrées par l'agent
sans interprétation ajoutée.

- **UI-D1 — Objectif** : réaliser **Ravum UI abouti**, « pas de MVP et zéro
  dette technique », validé par des tests réels sur de petites applications.
  **Unifia est reporté** : il ne sera envisagé qu'une fois que Ravum UI aura
  fait ses preuves.
- **UI-D2 — Événements** : d'abord un modèle **à messages** (enum
  d'événements traité par une fonction `update`, type Elm/Iced). Les
  closures viendront ensuite, dans une RFC séparée.
- **UI-D3 — Voie Web** (vérifier et optimiser des sites, contributions aux
  navigateurs) : **reportée**, hors du chantier Ravum UI natif.
- **UI-D4 — Dépendances de rendu** : **banc d'essai mesuré puis décision du
  responsable**. Aucune dépendance n'est adoptée sans cette décision.
- **UI-D5 — Ordre** : langage (RFC et extensions), puis moteur natif avec
  hot reload, testé sur de petites applications, puis Verify, responsive et
  motion construits sur de vraies interfaces.

Source de l'analyse discutée : un échange externe (plan « GOAL Ravum UI »),
dont seules les parties compatibles avec ces décisions sont reprises dans
[`docs/ui/PLAN.md`](../ui/PLAN.md).

## Compléments — revue externe transmise par le responsable (2026-11-24)

Le responsable a transmis une revue externe de la PR #210 avec un mandat
révisé. Ce mandat est enregistré ici ; tout point qui contredirait une
décision antérieure est signalé, et non appliqué en silence (aucun ne la
contredit).

- **UI-D6 — Ambition préservée** : on réduit le périmètre de chaque
  livraison, jamais l'ambition finale. Ces capacités restent toutes dans le
  plan, avec leur jalon :
  - styles riches au niveau CSS, Design Contract, scores déterministes ;
  - responsive sur les tailles intermédiaires, Motion Contracts ;
  - surfaces spécialisées (DAW, vidéo, 3D), mobile, closures et traits.
- **UI-D7 — Verify conjoint** (précise UI-D5) : le moteur expose dès le
  premier rendu les données dont Verify a besoin, et un Verify minimal
  (débordement, sortie de conteneur, chevauchement) accompagne le hot reload.
- **UI-D8 — Banc d'essai graphique autorisé** dès maintenant, en
  parallèle de la RFC. Il porte sur egui avec wgpu, Taffy, Vello, wgpu,
  winit, Slint, Blitz, Parley, AccessKit et Skia si c'est justifié. Aucune
  adoption définitive avant le rapport et votre décision (UI-D4).
- **UI-D9 — Mesures matérielles** sur les machines du responsable :
  - portable i7 13e génération, RTX 4070 8 Go, 16 Go, Windows et Ubuntu ;
  - fixe RTX 3060 12 Go, 32 Go ;
  - Xiaomi 14 Ultra.

  L'agent fournit les scripts et le protocole, et n'invente aucun résultat.
- **UI-D10 — Feuille de route** U0 à U10, avec le Web (W0 à W2) et Unifia
  différés : [`docs/ui/PLAN.md`](../ui/PLAN.md), révision 2. Les scores de
  qualité sont **maintenus** sous forme de sous-scores déterministes, et une
  erreur critique bloque quel que soit le score.
- **RFC-UI-LANG-1** : pas encore ratifiée. La révision 2 corrige la
  contradiction entre `Option<&V>` et OWN-6.1 en introduisant OWN-6.8
  (emprunts bornés), qui est à ratifier.
- **PR #210** : pas de fusion ni de ratification sans votre accord explicite
  et une CI verte sur le SHA exact.

## Décisions de cadrage finales (2026-11-24, transmises par le responsable)

- **UI-D11 — RFC-UI-LANG-1** : le **principe** d'OWN-6.8 (emprunts bornés)
  et de P1 à P5 est validé, ainsi que :
  - l'ordre déterministe des clés de `Map` ;
  - les opérations de texte ;
  - le clone structurel explicite et l'exclusion des ressources, par
    classification de type plutôt que par liste de noms.

  Conditions avant l'implémentation définitive :
  - les règles normatives sont rédigées (typage et propagation des vues,
    réemprunts, gardes, accès disjoints, sorties anticipées, emprunts
    transitifs et points sûrs de remplacement à chaud, interdiction à
    travers `.await`, représentation dans l'IR et le JIT, OWN-6.1 maintenu) ;
  - elles sont qualifiées, avec accord des voies B et C sur le périmètre
    revendiqué.

  La révision 3 de la RFC les rédige ; elles seront intégrées à la spec avec
  l'implémentation.
- **UI-D12 — Slint** : écarté comme dépendance principale. La politique est
  celle de dépendances permissives compatibles `MIT OR Apache-2.0`. La
  justification est corrigée : Slint offre la GPLv3, une licence gratuite
  avec conditions et une licence commerciale. Ce n'est donc pas une
  impossibilité juridique, mais un choix d'indépendance. Slint reste une
  référence comparative.
- **UI-D13 — Réactivité** : deux modèles, la reconstruction suivie d'un diff
  (A) et la réactivité granulaire (B), sont mesurés au banc avant de choisir.
- **UI-D14 — Portabilité** : L0 et L1 ne dépendent pas du JIT. L2 utilise le
  JIT en développement là où l'OS le permet, et la compilation anticipée en
  distribution. Les plugins sont isolés selon la plateforme.
- **UI-D15 — Git** : le chantier UI est séparé de la V2. Une PR dédiée part de
  la branche `claude/ravum-ui` (créée depuis `dev`) ; les commits sont
  conservés, rien n'est réécrit.
- **UI-D16 — Banc** : les tours 2, 3 et 4 sont autorisés. La pile modulaire est
  la candidate privilégiée, sans adoption ratifiée. La méthode est améliorée
  selon la revue (piles complètes, vrais changements de propriétés, scènes
  virtualisées et non virtualisées, mesures brutes publiées).

## Revue finale indépendante (2026-11-24, transmise par le responsable)

- **UI-D17** : l'architecture, le plan U0 à U10 et le principe de P1 à P5
  sont validés.

  Corrections demandées avant ratification et fusion :
  - **V2 (PR #212)** : valider les opérandes de `Term::TrapBounds`.
    **Fait** dans `f02d5ad` : indices et type `u64`, `match` exhaustif sur
    les terminateurs, validation par l'interpréteur avant exécution ; tests
    négatifs pour le validateur, `from_json`, l'interpréteur, le JIT et le
    moteur à chaud.
  - **RFC-UI-LANG-1 (révision 4)** :
    - provenance transitive ;
    - accès par l'emprunt mutable ;
    - propagation des vues limitée aux valeurs contenant une référence ;
    - points sûrs par thread et globaux ;
    - sémantique normative de `&mut` et conditions d'équivalence de la copie
      en entrée avec écriture en retour ;
    - effet `alloc` du clone ;
    - écart volontaire de `split("")` avec Rust.
- **Autorisation de fusion**, conditionnelle :
  - #212 vers `dev` une fois les corrections faites et la CI verte sur le
    SHA exact ;
  - puis #213 actualisée par rapport à `dev`, CI verte et conditions de revue
    satisfaites.

  Rien sur `main`, aucune release.

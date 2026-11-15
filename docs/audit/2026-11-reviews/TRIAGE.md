# Triage des quatre revues externes (Claude Opus 5.5, DeepSeek V4.1, ChatGPT Sol 6.1, ChatGPT Chatweb)

> **Provenance** : revues fournies par le responsable le 2026-11-11, portant sur
> `feat/lang-core-1.0-rc` à `5d88ecb` (base de la passe 6). Les textes bruts sont
> conservés dans le vault Obsidian ; ce document ne garde que les **constats
> vérifiés**, la preuve et la suite donnée — une revue par IA est une **entrée de
> triage**, pas une certification (D4).
>
> **Méthode** : chaque constat est reformulé en une ligne vérifiable, puis
> confronté au code et aux mesures du HEAD. Trois verdicts : `CONFIRMÉ` (correctif
> ou action), `FAUX` (preuve contraire citée), `HISTORIQUE` (vrai au SHA cité,
> corrigé depuis — ce qui débloque le code, mais ne doit pas être rejoué).
>
> **Mesures de référence au HEAD (`a2b8db5` / code `21cf7d4`)** : conformance
> **456 cas, 455 PASS / 0 FAIL / 1 SKIP**, build vérifié **195/195** ; sonde
> round-3 **47 programmes, 47 check-pass, 47 build-pass, 0 divergence** ;
> workspace **113 suites / 1537 passés** ; **0 forme `check`-pass / `build`-fail
> connue**.

## 1. Constats vérifiés

| # | Constat (auteur) | Verdict | Preuve au HEAD |
|---|---|---|---|
| 1 | « 37 programmes acceptés par `check` refusés par `rustc` » (Claude) | **HISTORIQUE** | Vrai à `5d88ecb` : c'était la mesure de la passe 6. Les huit familles ont été fermées (passes 6→9) ; la sonde round-3 donne **47/47/0** et le corpus **0 FAIL**. Aucune forme connue ne reste ouverte (`CONFORMANCE-BUILD-CONTRACT.md` §9) |
| 2 | « Un crash natif renvoie le code de sortie 0 sous Windows » (Claude, [mesuré]) | **FAUX au HEAD** | `child_exit_code(None)` dans `crates/ravum-lang-cli/src/commands/run.rs` mappe une terminaison sans code (signal) vers **exit 1** avec une note ; couvert par les contrats `exit_codes` (paires Windows/POSIX) |
| 3 | « Le répertoire `.ravum/gen` partagé fait produire le mauvais binaire avec exit 0 en build concurrent » (Claude) | **HISTORIQUE** | Test `two_concurrent_builds_of_the_same_package_succeed` (`crates/ravum-lang-cli/tests/build_isolation.rs`) : deux builds simultanés du même paquet réussissent, aucun n'écrase l'autre, arbre publié propre. Vert dans la suite |
| 4 | « `watch` absorbe les éditions faites pendant un build et tue la version saine » (Claude) | **HISTORIQUE** | `watch` est *fail-closed* depuis la passe 6 : compilation avant arrêt, rejet des résultats périmés, copie privée par processus, repli sur la dernière version valide. Preuves : suite `watch_continuity` (verte) + `scripts/watch-fail-closed-check.ps1` |
| 5 | « `D-COERCE-1` : le test censé valider la coercition porte sur un programme qui ne compile pas ; retirer la coercition, corriger `examples/stats` » (Claude) | **DÉJÀ APPLIQUÉ** | Passe 6 : l'arm `String` possédé est retiré de `coerce`, un test qui documentait l'acceptation est remplacé par un test de refus, `examples/stats` et `run/stats` passent par `.trimmed.as_str()`. Vérifié : aucun test `string_by_value` ne subsiste |
| 6 | « 553 des 805 énoncés MUST (68,7 %) ne sont cités par aucun test » (Claude) | **CONFIRMÉ, autre dénominateur** | Notre métrique (`tools/must_audit.py`) : **807 énoncés, 463 atteignables (57,4 %), 344 non atteints**. L'écart vient de la définition (« cité par un cas » vs « atteignable par un FeatureId ») ; les deux disent la même chose : le dénominateur 188/188 ne mesure pas les MUST (D10) |
| 7 | « `F-META-006` est auto-approuvée par l'agent » (Claude, ChatGPT Sol) | **CONFIRMÉ** | `tools/coverage_exemptions.json` : `approved_by` = « claim interne, non ratifié par un humain ». C'est exactement ce que le mandat interdit de présenter comme une revue. **Action** : l'exemption est désormais marquée `PENDING_HUMAN_APPROVAL` ; elle attend ta validation et une date d'expiration (proposition : réexamen à la première oracle exécutable ou 12 mois) |
| 8 | « `mutation.yml` n'a jamais tourné, son cron ne se déclenche pas depuis `main` » (Claude) | **CONFIRMÉ** | `gh run list --workflow mutation.yml` = **0 run**. Il est en `schedule`/`dispatch` et était absent de la branche par défaut ; depuis la fusion dans `main` il y est, le cron peut donc démarrer — à surveiller, rien d'autre à faire |
| 9 | « La protection de branche n'exige que `mvp`, aucune revue, `enforce_admins=false` » (Claude) | **CONFIRMÉ** | Lecture API : required check `mvp` seul, `required_approving_review_count: 0`, `enforce_admins: false`, pas de signature requise. **Proposition D3** (à ton approbation, car cela change la protection) : ajouter `gate-contracts` + `linux-qual` aux checks requis, passer `enforce_admins=true`, épingler les actions par SHA |
| 10 | « Les chiffres de la matrice sont hétérogènes : 386 §2 / 346 T2 / 343 T3 ; 1405 tests §1 vs 1019/269 T10 » (DeepSeek, ChatGPT Chatweb a lu 385 cas / 1405 tests / 89,2 %) | **CONFIRMÉ, sans contradiction** | Ce sont des **sections datées** (passe 2 à 5) qui précèdent l'en-tête « chiffres courants ». Deux relecteurs les ont prises pour l'état courant : c'est un défaut de lisibilité. **Action** : chaque section historique porte désormais un bandeau « chiffres de la passe N, non courants » |
| 11 | « Le gate T2 est `PASS` » (ChatGPT Chatweb : 5 gates verts au lieu de 4) | **FAUX au HEAD, vrai avant** | Le titre de détail disait encore « T2 — `PASS` » alors que le tableau disait `PARTIAL` (D10). Corrigé dans cette passe : titre aligné sur le tableau. C'est ce titre périmé qui explique le 5/13 du relecteur |
| 12 | « Aucune RFC FFI dédiée » (DeepSeek) | **FAUX** | `docs/rfc/RFC-D7-FFI-EFFECTS.md` existe depuis la passe 6 : inventaire réel (`extern "C"` + `unsafe`, types primitifs), preuve que `rustc` ne reçoit **aucun** `-L`/`-l`/`--extern`, contrats manquants listés, tests négatifs proposés |
| 13 | « `trim()` renvoie `String` ; il faudrait `&str` » (DeepSeek, ChatGPT Sol) | **CONFIRMÉ, choix contraint** | `builtins.rs` déclare `trim -> String` (valeur possédée). Ce n'est pas un oubli : sans lifetimes, un `&str` emprunté à un `String` local ne peut pas sortir de l'instruction — c'est précisément le problème D13. Le corriger proprement **demande** la solution D13 (§3), pas un changement de signature isolé |
| 14 | « Un binaire produit via Rust n'exige pas `rustc` chez l'utilisateur » (ChatGPT Sol) | **CONFIRMÉ, déjà écrit** | La machine de compilation et la machine d'exécution sont distinctes (`docs/release/`, `RFC-D2`) ; la dépendance `rustc` ne concerne que la première |
| 15 | « Un checksum seul n'établit pas l'attribution ; `allowed_signers` dans le dépôt n'est pas une ancre hors bande » (ChatGPT Sol, Claude) | **CONFIRMÉ** | Vrai, et déjà noté : la procédure `KEY-CUSTODY.md` est `PROPOSED`, la signature est **différée** (D9), et l'empreinte devra être publiée par un canal indépendant (site, notes de release) avant toute release |
| 16 | « Les revues IA ne remplacent pas l'audit humain » (les quatre) | **CONFIRMÉ** | C'est la définition de T4/T12 et la raison pour laquelle D4 reste `BLOCKED` pour le **label**, pas pour le code |

## 2. Recommandations convergentes retenues (et ce qu'elles changent)

Les quatre revues convergent sur quatre points ; trois sont déjà l'état du dépôt,
le quatrième est une décision :

1. **Voie B pour Core 1.0, pas de backend natif maintenant** (unanime). C'est la
   décision D2 déjà appliquée : la cible production-ready garde la voie B et la
   voie C hors 1.0 (`PRODUCTION-READINESS.md`, critère R1). Aucune IR publique
   avant un second consommateur réel.
2. **Pas de runtime universel monolithique** : verticales, contrats d'abord
   (unanime). Déjà l'état : threads/réseau/async `UNSUPPORTED` — le gel D6 le dit
   maintenant par écrit.
3. **FFI : frontière explicite, wrappers sûrs, `unsafe` confiné** (unanime).
   Adopté et détaillé : `PRODUCTION-READINESS.md` §5, avec les effets déclarés
   sur l'`extern`, l'inventaire des modules `trusted` et l'option d'isolation.
4. **D-COERCE-1 : emprunt explicite** (unanime). Déjà appliqué en passe 6 ; un
   relecteur propose un « auto-emprunt contrôlé » à la place : c'est un choix de
   langage, pas un correctif, et il est **couvert par la solution D13** (la
   staticité) plutôt que par une coercition supplémentaire.

## 3. Ce que le triage change dans le dépôt

| Action | Fichier | État |
|---|---|---|
| Exemption `F-META-006` marquée `PENDING_HUMAN_APPROVAL` | `tools/coverage_exemptions.json` | fait dans cette passe |
| Bandeaux « chiffres historiques » sur les sections datées de la matrice | `docs/roadmap/CORE-1.0-GATE-MATRIX.md` | fait dans cette passe |
| Titre T2 aligné sur le tableau (`PARTIAL`) | `docs/roadmap/CORE-1.0-GATE-MATRIX.md` | fait (passe 9) |
| Programme FFI : wrappers + isolation + effets sur l'`extern` | `docs/roadmap/PRODUCTION-READINESS.md` §5 | fait dans cette passe |
| Durcissement de la protection de branche | GitHub (hors dépôt) | **proposé**, attend ton accord (change la protection) |
| `mutation.yml` : premier run à observer depuis `main` | CI | à surveiller |
| `trim()` et D13 | voir §3 de la feuille de décisions | **décision D13** |

## 4. Ce que ce triage ne prétend pas

- Il ne « valide » pas les revues : il vérifie des constats, et il en rejette
  certains avec preuve.
- Il ne remplace pas l'audit humain (T4) : les quatre relecteurs sont des modèles,
  aucun n'endosse la responsabilité d'une revue de sûreté.
- Il ne prouve pas l'absence de défauts non signalés : 16 constats triés ne sont
  pas une couverture exhaustive.

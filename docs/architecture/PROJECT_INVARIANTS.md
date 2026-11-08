# PROJECT_INVARIANTS — Projet Ravum

> **Projet de charte normative — non ratifié**. Les principes expriment l'intention constante du responsable ; les précisions techniques et nouveaux identifiants restent à approuver. Ne pas modifier ces exigences au nom d'un agent IA.

## Objectifs P0

- **P0-A** Feedback continu et fluide : source→diagnostic→compilation ciblée→activation/preview ; jamais de rebuild inutile.
- **P0-B** Performance runtime native et binaire léger, sans GC imposé.
- **P0-C** Sûreté mémoire par défaut et frontières non sûres explicites.
- **P0-D** Contrats, preuves et tests avec honnêteté du statut et de la TCB.
- **P0-E** Feedback IA structuré, réparation sans dégradation de contrat.
- **P0-F** Profils temps réel : interdictions d'effets, budgets, WCET lié aux plateformes qualifiées.
- **P0-G** Domaines universels : native, web/frontend, backend, SQL, desktop/mobile, embarqué, DSP, streaming, GPU.
- **P0-H** Sobriété CPU/RAM, baseline matérielle 16 Go.
- **P0-I** Réduction du **temps total jusqu'à un livrable vérifié** (humain et agent inclus).

## Invariants à tester dès le MVP (proposés)

- **INV-01 — Honnêteté** : `TESTED/ASSUMED/UNKNOWN/PENDING/STALE/TIMEOUT/UNSUPPORTED` ne deviennent jamais `PROVED` par commodité. Les preuves déclarent leur modèle et leur TCB.
- **INV-02 — Autorité des contrats** : l'IA peut proposer un changement de spécification, pas l'accepter en silence.
- **INV-03 — Activation sûre** : un AST incomplet, une compilation échouée ou un module incompatible ne peut remplacer la version active.
- **INV-04 — Contexte exact** : toute évidence porte ses identités de snapshot, build, preuve, artefact et activation.
- **INV-05 — Thread temps réel** : pas de compilation, allocations, locks bloquants, JIT, migrations ou destruction coûteuse sur le callback audio ; ne pas extrapoler un test au hard-RT.
- **INV-06 — Universalité modulaire** : runtime minimal par domaine, pas d'imposition d'un framework UI ou serveur.
- **INV-07 — Résultats obsolètes** : l'annulation et la révision de snapshot empêchent toute activation d'un résultat antérieur.
- **INV-08 — Traçabilité** : source/contrats/outils/flags/plateforme/matériel/seed pour toute mesure ou preuve.
- **INV-09 — Productivité globale** : mesurer la boucle agent jusqu'à correctif vérifié, pas seulement le codegen.
- **INV-10 — Budget de complexité** : aucune abstraction sans consommateur réel ; composants remplaçables avec tests de contrat.
- **INV-11 — Indépendance** : protocoles, preuves et sémantique Ravum ne dépendent d'aucune API privée `rustc_*`.
- **INV-12 — Oracle indépendant** : Rust ne suffit pas pour démontrer une sémantique Ravum plus permissive que Rust.
- **INV-13 — Qualité des revues** : ne jamais voter sur un ADR sans avoir lu et cité sa décision réelle ; sinon `NOT_REVIEWED`.

### Règles de preuve et confiance

Les modèles de preuves, plateformes, FFI, solveurs, contre-exemples et certificats sont étudiés par ADR-004/013/014/035. Une absence de certificat ne doit pas être masquée : mentionner `solver-trusted` ou `certificate-checked` lorsque justifié.

**Statut présent :** aucune propriété du nouveau compilateur n'a été démontrée.

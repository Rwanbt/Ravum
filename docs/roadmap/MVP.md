# MVP Ravum — plan exécutable sans dette architecturale volontaire

> **Statut : PROPOSED (feuille de route), partiellement exécutée.** Les jalons M0 à M4 ont produit des prototypes réels (MVP-0a) puis un cycle de durcissement (MVP-0b) ; l'état vérifié est tenu par [docs/architecture/STATUS.md](../architecture/STATUS.md) et les rapports [docs/mvp/](../mvp/). Les chiffres de ce document restent des **cibles exploratoires**, pas des performances observées ; certaines ne sont pas atteintes et sont publiées comme telles. Ce document n'est pas une décision de développer un compilateur autonome.

## Produit que nous voulons valider

Une modification du code (y compris dans un buffer non sauvegardé) provoque l'analyse ciblée, un diagnostic structuré, la compilation pertinente, l'activation sûre du patch si compatible et une preview qui reste utilisable. La dernière version valide subsiste si l'édition échoue. L'agent IA peut corriger sans changer les contrats normatifs.

## Architecture F puis D *conditionnelle*

**F** : MVP avec Rust existant pour démontrer les mécanismes et livrer éventuellement un outil autonome utile. **D** : futur langage Ravum avec sémantique et IR propres, seulement après une comparaison reproductible (M5). Ni l'outil Rust ni le codegen via Rust ne doivent imposer la sémantique de Ravum.

## M0 — Baseline, spécifications, corpus

- Charte P0 et invariants (proposés) ; traçabilité exigences→ADR→tests.
- Référence de mesure documentée : OS, CPU, RAM (machine 16 Go primaire), disque, toolchain, charge.
- Fixtures **calculator** (fonction pure), **DSP gain/IIR**, **mini UI**, **API+SQLite** ; jeux négatifs de patch et contrats.
- Distinguer `SourceSnapshotId`, `BuildContextId`, `VerificationKey`, `ArtifactId`, `ActiveVersionId` ; documenter canonicalisation et limites.
- Pré-enregistrer S1-R/S1-T et S8-A/S8-B, métriques p50/p95/p99, mêmes modèles IA, contraintes de temps/tokens.
- Baseline Rust `cargo`, frontend, codegen, **link**, activation, rendu ; performance et RAM.
- Définir modèle d'autorité solo pour les spécifications, licence future et périmètre des contributions.

**Go :** corpus et protocole reproductibles ; aucun chiffre « Ravum » inventé. **No-Go :** exigences manquantes, dépendances non verrouillées, tests masqués.

## M1 — MVP-0a boucle continue

Composants internes candidats, chacun uniquement à l'arrivée de son consommateur : `ravum-protocol`, `ravum-snapshot`, `ravum-coordinator`, `ravum-rust-adapter`, `ravum-patch-host`, `ravum-cli`.

1. Snapshot immuable en mémoire avec overlays, sans modifier les fichiers utilisateur.
2. Analyse Rust via protocole public (LSP, cargo/rustc CLI), diagnostics sourcés et versionnés.
3. Scheduler prioritaire interactif, annulation/rejet de résultats périmés, budgets.
4. PatchHost **prototype** sous frontière C ABI FFI-sûre et manifeste ; refuser un layout incompatible, conserver l'ancienne version tant qu'elle est appelée. Aucun ABI public figé.
5. CLI minimale : `check`, `watch`, `status`, `patch` / sortie JSON ; aucune commande prétendue existante avant implémentation.
6. Tests négatifs de version, structure, fonctions en vol, échec de build, crash worker, annulation.

**Go exploratoire :** feedback local p95 < 50 ms et petit patch p95 < 500 ms sur machine définie, sécurité fail-closed prouvée par tests adaptés. Si seuil raté, rapport et pivot, **jamais dissimulation des défauts**.

## M2 — Preuves et cache de confiance

- Adaptateur de vérification Kani ou autre outil comparé par S6, **processus isolé** ; aucune preuve implicite.
- Clé complète du contexte preuve : propriété, hypothèses, outil/version, modèles, contrats, flags, cibles, dépendances.
- Cache mis en quarantaine jusqu'aux adversarial tests S2 (mutation de contrats, axiomes, flags, FFI, solvers, toolchain).
- Statuts `PROVED` seulement dans le périmètre déclaré, `PENDING/STALE` visibles, résultats `UNKNOWN` non interprétés comme succès.

## M3 — Profil DSP temps réel

- Simple traitement gain puis IIR à état ; suivi de continuité et discontinuités séparément des XRUN.
- Patch préparé hors RT, activation à frontière de bloc, reclamation et `Drop` sur thread non-RT.
- Mesures et cas négatifs d'alloc/lock/FFI/destructeurs transitifs ; Linux instrumentation + Windows profil honnête `UNKNOWN/ASSUMED` selon qualification.
- Une vraie validation audio device/host est un jalon distinct du simulateur.

## M4 — Full-stack minimal

- App web interactive + serveur HTTP + SQLite ; types partagés, validations serveur, requêtes SQL, migration N/N-1.
- E2E véritables ; vérifier contrat et authentification/autorisation serveur.
- WASI/WIT étudiés par couple toolchain/runtime fixé ; ne pas supposer leur disponibilité universelle.

## M5 — Gate « nouveau langage ? »

Comparer sur corpus **identique**, même machine, mêmes benchmarks et mêmes agents IA :
- **A** Rust + outillage Ravum.
- **B** mini frontend Ravum → Rust.
- **C** mini frontend Ravum → IR contrôlée → Cranelift.

S1-R (Rust baseline) ne valide pas S1-T (frontend jouet à requêtes par définition) ; expériences distinctes. Mesurer analyse, codegen, **link**, patch, preview, mémoire, **temps jusqu'au livrable vérifié** S8 et handicap LLM hors corpus. Rust n'est qu'un oracle du sous-ensemble sémantique commun. **Ne pas accepter un langage autonome par préférence personnelle ou score théorique.**

## M6+ — Élargissement si et seulement si justifié

Formaliser sémantique, ownership, effets, contrats, IR multiples nécessaires mesurées, natif/WASM, backends spécialisés, optimisation validée, API IA, RT avancé, plateformes. Les sujets GPU/mobile/hard-RT restent des programmes séparés.

## Règles anti-dette / DoD

- Aucune API projet sans consommateur ; adaptateur externes isolés ; jamais de dépendance à `rustc_private` dans `protocol` et `evidence`.
- Pas de backend direct ou de linker maison au MVP-0a.
- Code utilisateur et prototypes expérimentaux séparés ; `main` n'est pas une release.
- Toute issue précise hypothèse, preuves, cas négatif, limites, impacts P0/ADR, état.
- CI par impact **conservative** : jouer tous les tests affectés et les E2E requis ; suite complète périodique, jamais de suppression d'échec pour « passer au vert ».
- Chaque résultat contient code SHA, contextes, configuration, matériel, et données brutes ; inconnu reste inconnu.
- Aucune valeur de latence/coverage/PROVED/RT publiée avant mesure réelle.

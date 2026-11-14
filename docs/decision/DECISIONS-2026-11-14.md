# Enregistrement des décisions du responsable — 2026-11-14 (vague 3)

> **Statut : décisions du responsable, transmises le 2026-11-14.** Ce document
> recopie **mot pour mot** la section « Décisions du responsable » du prompt de la
> vague 3 ; il ne les réinterprète pas. Les décisions y prennent effet par la
> transmission du prompt ; aucun agent ne les a prises.
>
> **Portée.** Ce document enregistre aussi les décisions qui ne sont **pas**
> tranchées (`D2`, `D4`, `D9`, `T12`, `D-2026-11-14-6`, `D-2026-11-14-7`) : elles
> restent des décisions humaines ouvertes et ne sont ni ratifiées ni contournées
> ici.
>
> **Application.** Chaque décision est reportée à sa place dans
> [`DECISIONS_REQUIRED.md`](../../DECISIONS_REQUIRED.md) §0, dans
> [`HUMAN-DECISIONS-REQUIRED.md`](../reports/HUMAN-DECISIONS-REQUIRED.md), dans
> les statuts des RFC concernées et dans
> [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) /
> [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) pour les
> mentions de périmètre et les cibles. Aucun verdict de gate n'est changé par cet
> enregistrement : un `PARTIAL` reste `PARTIAL` tant qu'une commande de la vague
> ne prouve pas autre chose.

## Décisions du responsable (2026-11-14) — elles font autorité

S1  AGENTS.md : révision du 2026-11-13 APPROUVÉE.
S2  NodeId = u64 (32 bits FileId + 32 bits index), plafond 65 536 fichiers :
    APPROUVÉ. modules/file-limit requalifié compile-pass.
S3  RFC-RT-TRAPS-1 et son amendement --rt-entry (portée par point d'entrée,
    deux formes d'indexation bornée) : APPROUVÉS.
S4  docs/ffi/SAFETY.md, clauses C-FFI-1 à C-FFI-10 : APPROUVÉ comme contrat
    de la frontière scalaire. i128 et u128 sont RETIRÉS des types FFI admis.
    C-FFI-4 : une fonction C qui déroule, longjmp, lève un signal ou termine
    le processus est HORS CONTRAT (obligation de l'appelant, non vérifiée).
S5  Objectif de couverture : 100 % lignes et régions du CODE PRODUIT, hors
    modules #[cfg(test)] et hors chemins inatteignables documentés un par un.
    Seconde métrique : mutation ciblée. Les 344 MUST non mappés sont une dette
    tracée, pas un blocage.
S6  Test de puissance T2 : recréer sur l'arbre courant l'inverse de chaque
    correctif historique ; EXIGER qu'un test existant échoue avec l'inverse
    (sinon : lacune de test à documenter, pas un succès) ; puis 3 graines ×
    2 000 programmes du générateur dirigé par les types. Score publié sur 13.
S7  Comparaison du Rust généré : identité après normalisation, limitée aux
    deux motifs __rv_root_<id> et rvm_ffi_<id>. Toute autre différence = échec.
S8  REFUSÉ tel que proposé. La cible LSP n'est pas assouplie pour absorber un
    défaut algorithmique. Ordre : corriger la phase own (agent A), re-mesurer,
    puis appliquer : p95 < 50 ms jusqu'à 1 000 lignes ; p95 ≤ 150 ms à 5 110
    lignes en profil release ; contrôle de linéarité en CI (voir agent A).
    L'incrémentalité sémantique reste un chantier 1.x.
A1  RFC-OWN-FACTS-1 : APPROUVÉE, étape C comprise, aux conditions de
    l'agent E.
A2  Bloc DSP (phase 8, RFC-RV-DSP-BLOCK-1) : REPORTÉ à la 1.1. Hors périmètre
    1.0. Ne pas l'implémenter. T8 et T9 restent PARTIAL avec cette mention.
A3  Voie C : hors 1.0. Le gate T1 est évalué sur « voies A et B comparées,
    B retenue pour 1.0 ».
A4  RFC-DEV-TOOLS-1 : APPROUVÉE. cargo-mutants et Miri peuvent être installés
    localement. Ce sont des outils de développement, pas des dépendances.
A5  D11 : pour chaque rule_id dupliqué de grammar.md, la première règle garde
    son numéro, la seconde reçoit un numéro neuf inutilisé, consigné dans
    Q-GRAM-11. Corriger aussi les deux lignes avec un | non échappé.

NON TRANCHÉ — tu n'y touches pas, tu les listes dans ton rapport :
D2 (ratification du socle et de RFC-D2), D4 (audit externe), D9 (clé de
signature), T12 (Go/No-Go), D-2026-11-14-6, D-2026-11-14-7.

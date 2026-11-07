# Spécification exploratoire — sémantique, typage, preuve et confiance

## A. Sémantique à définir formellement avant codegen

1. **Valeurs** : entiers signés/non signés fixes, bool, flotteurs stricts, `Unit`, strings/bytes UTF-8, tableaux, slices, tuples, structures, enums, fonctions, fermetures ; identité et égalité définies par type.
2. **Emprunts** : exclusivité de `&mut`, partage de `&`, durées de vie statiques, transferts de propriété, gestion des mouvements partiels, `Drop` déterministe dans domaine contrôlé, portée async/concurrence.
3. **Erreur** : chemins de panic explicitement vérifiables ; `Result/Option` ; impossible de convertir erreur en réussite implicitement ; exceptions FFI isolées.
4. **Entiers** : overflow checked par défaut ou opération marquée wrapping/saturating ; conversion de types explicite. Décision définitive via ADR-007.
5. **Flottants** : IEEE, NaN, Inf, -0, ordre, fusion FMA et arrondis ; modes strict/approx autorisés seulement avec contrats d'erreur.
6. **Terminaison** : pas de terminaison générale garantie ; preuve de variant exigible pour profil `total`/temps borné.
7. **Concurrence** : ordre mémoire pour atomiques, happens-before, data-race freedom du code sûr, fairness hypothétique du scheduler ; absence de deadlock seulement prouvée si contrat spécifique.
8. **Système** : interactions OS/FI/unsafe comme frontières de modèle avec hypothèses explicites et interdiction d'élévation de capacité.

## B. Modalités de preuve

Forme d'une obligation : `P = (property, scope, domain, assumptions, model, target, toolchain, solver, dependencies, budget)`. Une obligation ne peut être identifiée uniquement par numéro de ligne. Un changement de sémantique ou de dépendance change sa validité.

| Statut | Sens | Activation strict |
|---|---|---|
| `PROVED` | Propriété démontrée dans le modèle annoncé ; TCB explicite | Oui pour l'obligation couverte |
| `DISPROVED` | Contre-exemple pertinent trouvé | Non |
| `UNKNOWN` | Analyse non concluante | Non pour obligation requise |
| `TIMEOUT` | Budget expiré | Non pour obligation requise |
| `UNSUPPORTED` | Fonctionnalité ou cible non prise en charge | Non pour obligation requise |
| `ASSUMED` | Hypothèse externe, non démontrée | Non par défaut ; dérogation signée propre au profil |
| `TESTED` | Tests effectués ; aucune preuve universelle | Non si preuve requise |

Le terme `PROVED` doit préciser **qui est dans la TCB**. Un `UNSAT` SMT sans certificat vérifié est une preuve conditionnelle à la correction du solver, de l'encodage et de l'IR ; le manifeste doit le signaler. Une précondition contradictoire peut engendrer des théorèmes vacus : inclure satisfiabilité de préconditions et reachability checks.

**Code fantôme** : spécifications et preuves effaçables en runtime uniquement si l'obligation est effectivement démontrée ; contrôles runtime conservés sinon selon profil. Ne jamais désactiver un contrôle pour obtenir un meilleur benchmark sans justificatif.

## C. Niveaux de contrat

- `default_safe` : ownership, typage, effets de base, instrumentation de panic si nécessaire.
- `verified` : obligations normatives explicitement marquées, inconnus bloquants à la release.
- `rt_safe` : aucune opération interdite dans call graph, contrôles obligatoires.
- `rt_bounded` : `rt_safe` + bornes allocations (zéro ou fixées), pile/itérations/buffers ; modèle coût.
- `hard_rt` : attestation cible spécialisée indépendante.

Exemple **illustratif** (pas de parseur ni outil exécutable) :

```rust
capability DbRead;

fn safe_withdraw(balance: u64, amount: u64) -> Result<u64, BankError>
    requires amount <= balance
    ensures result.is_ok() && result.unwrap() == balance - amount
    effects(none)
{
    Ok(balance - amount)
}
```

Ce contrat devra être adapté à la représentation effective de `Result` et aux règles d'assertion (`unwrap` n'est ici qu'illustratif). Une fonction incorrecte n'est pas rendue juste par un test unitaire vert.

## D. Contrat d'audit IA et preuve

- Les contrats *normatifs* résident dans fichiers/métadonnées gouvernés ; une IA peut les lire, commenter et proposer leur changement, **pas les modifier en silence**.
- Un patch préserve au minimum : contrats, types publics, capacités, appels FFI/unsafe déclarés, et contraintes de release du profil.
- Toute optimisation signale : équivalence stricte, raffinement, ou seulement tests/différentiel. On n'affirme pas « équivalent » sur seule absence de divergence testée.
- Les contre-exemples comportent valeurs minimales, scheduler seed, environnement, version et relecture par le même outil si possible.

## E. Validation négative incontournable

Injecter exemples volontairement fautifs : use-after-free via pont unsafe, références pendantes statiques, division par zéro, overflow, buffer out-of-bounds, data race, lock dans callback, loop infinie en bounded, axiome erroné, contrat avec `requires false`, timeout du solveur, invalidation de cache, FFI ABI mismatch. Les résultats attendus sont des refus ou statuts explicites, pas des faux succès.

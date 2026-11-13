# RFC-NODE-ID-1 — Identité des nœuds : ce qui est déjà vrai, et la règle de cache qui manque

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-NODE-ID-1` |
| Statut | **PROPOSED** — `PENDING_HUMAN_APPROVAL` pour la règle de cache (§4) |
| Date | 2026-11-13 |
| Auteur | agent de développement (session GOAL) |
| Portée | `ravum-lang-syntax` (`NodeId`), `ravum-lang-sema` (`Facts`), LSP, futurs caches |
| Décision attendue | ratifier la règle « aucun cache indexé par `FileId` » et le modèle d'identité ci-dessous |

## 1. Constat mesuré (phase 6.1)

L'audit C5 soupçonnait des « `NodeId` globaux attribués dans l'ordre de lecture »,
hostiles à l'incrémentalité. **Ce n'est pas ce que fait le code** :

- `crates/ravum-lang-syntax/src/parser.rs` (lignes 67-86) alloue
  `NodeId = (FileId & 0xff) << 24 | compteur_local`, avec un compteur **par
  fichier** ;
- le loader parse chaque fichier indépendamment (`parse_file(file_id, text)`),
  puis assemble les modules.

Preuve exécutée :

```
cargo test -p ravum-lang-sema --test node_id_stability
2 passed; 0 failed
```

Le test `editing_one_file_does_not_move_the_other_files_node_ids` charge un
paquet à deux fichiers, ajoute une instruction dans `main.rv` (via les overrides
en mémoire du loader) et compare l'**instantané AST complet** (`Debug`, ids et
spans inclus) du module `util.rv` : il est **identique**. Le second test vérifie
que l'octet haut porte bien un `FileId` distinct de celui de la racine et que le
compteur est dans les 24 bits bas.

**Conséquence : la migration `(FileId, index local)` de la phase 6.3 est sans
objet.** Aucun commit de migration n'est nécessaire ; l'écrire serait un
refactoring sans consommateur (R11).

## 2. Ce qui peut encore bouger

`FileId` est attribué dans l'**ordre de chargement** (`SourceMap::add`), pas par
chemin. Donc :

- éditer un fichier existant ne change **aucun** `NodeId` des autres fichiers
  (mesuré ci-dessus) ;
- **ajouter ou retirer un fichier** peut décaler les `FileId` des fichiers
  chargés après lui, donc tous leurs `NodeId` ;
- l'ordre de chargement peut changer si un `mod` est déplacé dans un fichier.

C'est acceptable pour le compilateur (une compilation repart de zéro) mais
**interdit de persister un `FileId` dans un cache** : un cache qui stockerait
`NodeId -> fait` verrait des faits attribués au mauvais fichier après un ajout.

## 3. Modèle proposé (description de l'existant, pas un changement)

```text
NodeId (u32) = [ FileId (8 bits) | compteur local au fichier (24 bits) ]
```

- unicité par fichier, pas d'allocateur global ;
- limite documentée : ~16,7 M de nœuds par fichier (`MAX_NODES` vérifie la
  borne) ;
- les références **entre fichiers** ne doivent pas être des `NodeId` persistés :
  elles passent par un **chemin de définition** (chemin de module + nom), que le
  résolveur reconstruit à chaque chargement ;
- aucune API publique n'expose le découpage en octets comme un contrat stable :
  seul `NodeId` (u32) est gelé par le design §4.

## 4. Règle de cache proposée (la seule décision nouvelle)

1. Un cache de résultat de frontend est indexé par **(chemin, hash de contenu)**
   — jamais par `FileId`, jamais par `NodeId` seul.
2. Un résultat calculé pour une version antérieure d'un buffer n'est jamais
   publié (INV-07) : la clé inclut le hash du contenu effectivement analysé.
3. Les références inter-fichiers sont résolues par chemin de définition, pas par
   identifiant numérique.
4. Les faits qui survivent à un rechargement (`OwnFacts` à terme) sont
   **revalidés** contre le nouveau `FileId` avant usage.

Cette règle n'exige aucun changement de type : elle contraint les caches futurs
(LSP, incrémental du frontend) et documente pourquoi `NodeId` ne doit pas sortir
du processus.

## 5. Alternatives

| Option | Coût | Verdict |
|---|---|---|
| (a) Statu quo implicite | nul | laisserait un futur cache indexé par `FileId` casser silencieusement |
| (b) Plages de `u32` réservées par fichier | faible | fragile à l'ajout de fichiers, ne règle pas la persistance |
| (c) `(FileId, index)` + règle de cache (§3-§4) | nul (déjà implémenté) + une règle écrite | **retenu** |

## 6. Validation

- Test `node_id_stability` (2 tests) dans `cargo test --workspace`.
- Conformance et oracle différentiel inchangés (aucun code du compilateur
  modifié).
- Toute future implémentation de cache devra fournir un test qui édite un
  fichier et prouve que le résultat publié correspond au **nouveau** hash
  (INV-07), pas un test qui se contente de mesurer une latence.

## 7. Décision demandée

1. Ratifier le modèle du §3 (description de l'existant) comme référence pour les
   caches.
2. Ratifier la règle du §4, en particulier « aucun cache indexé par `FileId` ».
3. Confirmer qu'aucune migration de `NodeId` n'est attendue en Core 1.0.

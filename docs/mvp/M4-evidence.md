# M4 — Verticale full-stack : état réel

> Mesures et résultats du 2026-11-08, machine de référence. Application témoin :
> `apps/ravum-todo`. Aucun navigateur n'est piloté par les tests et aucun TLS n'est
> implémenté — c'est écrit ici et répété dans les limites.

## 1. Ce qui est implémenté

| Couche | Contenu | Fichier |
|---|---|---|
| Types partagés | `Task`, `NewTask`, `TaskPatch`, règles de validation, contrat machine | `crates/ravum-todo-core/src/lib.rs` |
| Stockage | SQLite embarqué (rusqlite), migrations versionnées, dépôt typé | `crates/ravum-store/src/lib.rs` |
| API | serveur HTTP/1.1 minimal, CRUD, jeton Bearer, erreurs structurées | `apps/ravum-todo/src/routes.rs` |
| UI | page statique + `app.js`, aucune étape de build | `apps/ravum-todo/static/` |
| E2E | 10 tests sur socket réel + base SQLite réelle | `apps/ravum-todo/tests/e2e.rs` |

### Frontière de dépendance

Une seule dépendance externe ajoutée pour M4 : `rusqlite` avec SQLite **embarqué**
(compilé depuis les sources), isolée derrière `ravum-store`. Aucun autre crate ne
voit SQL. Motif du choix : le build a été vérifié localement avant de l'adopter, et
l'embarqué rend la CI reproductible sur n'importe quel OS, contrairement à un lien
vers `winsqlite3.dll`.

## 2. Contrat partagé

`GET /api/schema` expose, depuis la **même** définition Rust que la validation :

```json
{
  "contract_version": 1,
  "resources": {"task": {"fields": [
    {"name": "id", "type": "integer", "writable": false},
    {"name": "title", "type": "string", "max_length": 120, "required": true},
    {"name": "done", "type": "boolean", "default": false},
    {"name": "due_date", "type": "string|null", "format": "YYYY-MM-DD"},
    {"name": "created_at", "type": "string", "writable": false}
  ]}},
  "compatibility": "exact contract_version match; no forward compatibility is claimed"
}
```

Le client compare `contract_version` au démarrage ; en cas d'écart il **désactive
toutes les mutations** et affiche la raison. La règle est dans le code partagé
(`client_supports`) et testée ; le comportement JavaScript lui-même n'est pas exécuté
par la suite de tests.

## 3. Migrations de schéma

| Version | Contenu | Comportement |
|---|---|---|
| 1 | table `tasks` (id, title, done, created_at) | — |
| 2 | ajout de `due_date TEXT` | additive, les lignes existantes sont conservées |

Règles tenues par le code :

1. chaque migration s'exécute dans **sa** transaction, puis `PRAGMA user_version`;
2. une base **plus récente** que le binaire est **refusée au démarrage** avec un
   message explicite (fail-closed) ;
3. une version négative est refusée ;
4. aucun `DROP`, aucun `ALTER` destructif : la migration v1→v2 est purement additive.

Preuves : test unitaire `migrating_from_v1_preserves_rows_and_adds_the_column`
(`ravum-store`), test E2E `e2e_migration_from_a_legacy_database` (base v1 réelle,
serveur réel), et `an_unknown_schema_version_refuses_to_start` (refus explicite).

## 4. Validation serveur

Toutes les règles sont appliquées **côté serveur**, pas seulement dans l'UI :

| Cas | Réponse |
|---|---|
| titre vide après `trim` | `400` `validation_failed`, violation `title_empty` |
| titre > 120 caractères (comptés en caractères, pas en octets) | `400`, `title_too_long` |
| échéance mal formée (`2026-1-1`, `2026-13-40`, `20261109`) | `400`, `due_date_invalid` |
| JSON illisible | `400`, `invalid_json` |
| patch vide | `400`, `nothing_to_change` |
| écriture sans jeton ou avec un jeton faux | `401`, `unauthorized` |
| tâche inexistante | `404`, `not_found` |

Le serveur renvoie **toutes** les violations d'une requête, pas seulement la première.

## 5. Autorisation

Jeton Bearer partagé, exigé sur `POST`, `PATCH`, `DELETE`. Les lectures sont publiques
— choix explicite, pas un oubli : il n'y a ni session, ni utilisateur, ni hachage de
mot de passe. Ce n'est **pas** un modèle d'authentification ; c'est un contrôle
d'accès minimal pour démontrer la frontière.

## 6. Résultats d'exécution

Script : `powershell -ExecutionPolicy Bypass -File scripts/demo-m4.ps1`
Sortie brute : `evidence/local/demo-m4.txt` (non versionnée, régénérée à chaque run).

| Étape | Résultat observé |
|---|---|
| `/health` | `schema_version=2`, `contract_version=1` |
| `/api/schema` | `contract_version=1`, compatibilité « exact » |
| création | `201`, titre nettoyé (`"  écrire la démonstration  "` → `"écrire la démonstration"`), `due_date=2026-11-09` |
| mise à jour | `200`, `done=true` |
| suppression | `204` |
| liste après suppression | 0 tâche |
| écriture sans jeton | `401` |
| titre vide | `400`, corps contenant `title_empty` |
| migration v1 → v2 | ligne `tache v1` conservée, `due_date` ajoutée et utilisable |

## 7. Tests

| Suite | Nombre | Portée |
|---|---|---|
| `ravum-todo-core` (unitaires) | 7 | validation, compatibilité, contrat |
| `ravum-store` (unitaires) | 6 | format de date, CRUD, migration, refus de version |
| `ravum-todo` (unitaires HTTP) | 4 | parsing de requête, cas malformés |
| `ravum-todo` E2E (socket + SQLite) | 10 | CRUD, validation, autorisation, contrat, redémarrage, migration, schéma inconnu, UI statique |

Total du workspace : **139 tests**, 139 verts, 0 échec.

## 8. Ce qui n'est PAS démontré

1. **Aucun navigateur** n'exécute `app.js` : le contrat et la validation sont testés
   au niveau du serveur et de la règle partagée, pas du DOM.
2. **Aucun TLS**, aucune protection contre le rejeu, aucune expiration de jeton.
3. **Une seule connexion SQLite** derrière un mutex : pas de pool, pas de test de
   charge concurrente.
4. **Pas de « recompilation ciblée »** au sens du compilateur incrémental : l'UI est
   statique (aucune étape de build) et le serveur est un binaire Rust ordinaire.
   Toucher `static/index.html` déclenche bien une recompilation de `ravum-todo`
   (fichier inclus par `include_str!`), mais rien n'est mesuré à ce sujet.
5. **Pas de migration destructive** : seules les migrations additives sont testées.
   Un changement de type de colonne n'est ni outillé ni autorisé.
6. **Pas de E2E multi-navigateurs**, pas de test d'accessibilité, pas de test de
   charge.

## 9. Reproduire

```powershell
cargo build --workspace
cargo test --workspace -- --test-threads=1
powershell -ExecutionPolicy Bypass -File scripts/demo-m4.ps1
# ou manuellement :
cargo run -p ravum-todo -- --db target/ravum/todo.db --port 8080 --token dev-token
```

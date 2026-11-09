# MVP-0b / L5.3 — Analyse de licence et inventaire tiers (décision humaine requise)

> **Aucune licence n'est publiée par ce document ni par ce cycle.** La décision revient
> au responsable (#34 / RAV-011). Ce document prépare la décision : inventaire réel des
> dépendances, familles de licences, options et conséquences, et outil de contrôle
> automatique pour qu'aucune décision implicite ne s'installe.

## 1. Constat

| Élément | État observé (2026-11-08) |
|---|---|
| Fichier `LICENSE` / `COPYING` à la racine | **absent** — vérifié par script |
| Déclaration dans `Cargo.toml` (workspace) | aucune (`publish = false`, pas de champ `license`) |
| Mention README | « aucune licence n'est actuellement déclarée » |
| Issue | [#34](https://github.com/Rwanbt/Ravum/issues/34) ouverte, décision non prise |
| Dépendances tierces | **41 paquets**, tous permissifs (§2) |

Conséquence légale actuelle : sans licence, le code reste « tous droits réservés » par
défaut. Ce n'est pas un état open source, et ce n'est pas un oubli à corriger par un
agent : c'est une décision explicite du responsable.

## 2. Inventaire tiers réel

Source : `cargo metadata --format-version 1`, filtré sur les paquets hors workspace.
L'inventaire complet (nom, version, licence, dépôt) est produit par
`powershell -File scripts/licence-audit.ps1 -Json`.

| Famille de licence | Paquets | Exemples |
|---|---:|---|
| `MIT OR Apache-2.0` | 27 | serde, serde_json, sha2, windows-sys (via deps) |
| `MIT` | 6 | libsqlite3-sys, rusqlite, rsqlite-vfs, sqlite-wasm-rs, generic-array, zmij |
| `MIT/Apache-2.0` (forme ancienne) | 4 | fallible-iterator, vcpkg, version_check |
| `ISC` | 1 | libloading |
| `Unlicense OR MIT` | 1 | memchr |
| `Zlib` | 1 | foldhash |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 | unicode-ident |

- **Aucune** dépendance sous GPL, AGPL, SSPL, BUSL, CC-BY-NC, Commons Clause ou
  « proprietary ».
- Aucune dépendance sans métadonnée de licence.
- `rusqlite`/`libsqlite3-sys` embarquent SQLite (domaine public) — compatible avec toutes
  les options ci-dessous.
- Le seul point d'attention « patent/attribution » est `Unicode-3.0` (unicode-ident), qui
  impose l'attribution de la licence Unicode en plus de MIT/Apache : à conserver dans un
  éventuel `THIRD_PARTY_LICENCES.md` si l'option permissive est choisie.

## 3. Options de licence et conséquences

| Option | Avantages | Inconvénients / implications |
|---|---|---|
| **MIT** seule | Très simple, maximale adoption, compatible avec presque tout | Pas de clause brevets explicite ; peu de protection contre les forks propriétaires |
| **Apache-2.0** seule | Clause brevets explicite, cadre contribution clair | Formellement incompatible avec GPL-2.0-only (pas avec GPL-3.0), texte plus lourd |
| **MIT OR Apache-2.0** (double) | Norme de l'écosystème Rust ; la plupart des dépendances sont déjà dans cette famille ; adoption maximale + option brevets | Deux fichiers, décision par utilisateur ; attribution requise |
| **MPL-2.0** | Copyleft par fichier, favorise le retour des améliorations du cœur | Moins courant dans l'écosystème Rust ; friction pour certains intégrateurs embarqués |
| **AGPL-3.0** | Garantit le partage des modifications réseau | Exclut la majorité des usages produits/embarqués visés (P0-G, P0-F) ; probablement incompatible avec l'objectif « universel » |
| **Propriétaire / source-available** | Contrôle commercial | Contredit l'ouverture affichée ; bloquerait les contributions externes |
| **Différenciation** : code outil permissif + spécification sous CC-BY-4.0 | Permet l'adoption large du code et la réutilisation de la spécification | Deux régimes à gérer ; nécessite une politique de contribution (DCO ou CLA) |

### Recommandation (à valider par le responsable, pas une décision)

**`MIT OR Apache-2.0` pour le code de l'outillage Rust** (le crate `ravum-*` et les apps),
et une licence documentaire séparée à décider pour la spécification du futur langage
(CC-BY-4.0 ou équivalent). Raisons : c'est la famille de 27+ dépendances déjà présentes,
la norme de l'écosystème Rust, elle maximise l'adoption tout en offrant l'option brevets,
et elle n'empêche ni usage commercial ni embarqué. À compléter par une politique de
contribution (DCO recommandé pour un mainteneur solo) et un `THIRD_PARTY_LICENCES.md`.

**Ne pas** publier de `LICENSE` avant décision : le script d'audit échoue volontairement
s'il en détecte un (décision implicite).

## 4. Outil de contrôle : `scripts/licence-audit.ps1`

Deux vérifications indépendantes, exécutables en CI :

1. **Décision implicite** : échec (code 1) si un fichier `LICENSE`/`COPYING`/`UNLICENSE`
   apparaît à la racine alors qu'aucune décision n'a été prise.
2. **Dépendances** : échec (code 2) si une dépendance tierce est sous licence de la
   denylist (AGPL, GPL, SSPL, BUSL, CC-BY-NC, Commons Clause, proprietary) ou sans
   métadonnée de licence. Les copyleft faibles (LGPL) sont signalés séparément en
   avertissement (obligations de reliaison), pas en échec.

Exécutions réelles du 2026-11-08 :

| Cas | Commande | Résultat |
|---|---|---|
| Dépôt réel | `powershell -File scripts/licence-audit.ps1` | 41 dépendances, 0 incompatible, 0 inconnue, verdict `ok-pending-human-decision`, code 0 |
| Test négatif « décision implicite » | création temporaire d'un `LICENSE` vide puis relance | verdict `implicit-decision`, **code 1**, fichier supprimé ensuite |
| Test du motif de denylist | échantillons `AGPL-3.0-only`, `GPL-2.0-only`, `LicenseRef-Proprietary` | refusés ; `MIT OR Apache-2.0`, `MIT/Apache-2.0`, `Unlicense OR MIT`, `Zlib` acceptés ; `LGPL-3.0-only` non bloquant |

## 5. Ce qui n'est pas fait

- Aucune licence publiée, aucun `THIRD_PARTY_LICENCES.md` généré : cela dépend de l'option
  choisie.
- Aucune revue juridique : ce document est une analyse d'ingénierie, pas un avis juridique.
- Pas de politique de contribution (CLA/DCO) formalisée ; proposée comme étape suivante.
- Les dépendances de développement uniquement (outils CI) ne sont pas toutes auditées par
  `cargo metadata` si elles ne sont pas des crates ; les actions GitHub utilisées
  (`actions/checkout`, `dtolnay/rust-toolchain`, `actions/cache`) restent des composants
  tiers à mentionner dans l'inventaire final.

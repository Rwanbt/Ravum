# Fixture S8

Petit crate Rust autonome (4 modules, 8 fonctions, 10 tests) utilisé comme
**projet identique** par les trois scénarios du benchmark S8 :

- **A** — un worktree Git par agent, intégration séquentielle par `git merge` ;
- **B** — workspace partagé, propositions `ChangeProposal` + coordinateur Ravum
  et écriture Git transactionnelle unique ;
- **C** — agents séquentiels sur une branche unique (contrôle).

Le crate déclare un `[workspace]` vide dans `Cargo.toml` pour ne jamais
appartenir au workspace racine de Ravum. Les campagnes copient ce répertoire
dans un dossier temporaire éphémère avant de l'initialiser en dépôt Git.

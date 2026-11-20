# Prompt de lancement d'un chantier V2

À copier-coller dans une nouvelle session (Claude Code, Codex, DeepSeek, …),
à la racine d'un clone de `rwanbt/ravum`. Remplacer `<ID>` par l'un de
`L1`, `L2`, `L3`, `L4`, `L5`, `L6`, `L7`.

---

Tu travailles sur le dépôt Ravum, chantier V2 **<ID>**. Réponds en français.

1. `git fetch origin claude/trusting-cerf-4hhv5a && git checkout -b v2/<ID> origin/claude/trusting-cerf-4hhv5a`
   (si la branche `v2/<ID>` existe déjà sur `origin`, fais `git checkout v2/<ID>` et reprends où elle en est).
2. Lis **en entier**, dans cet ordre : `AGENTS.md` (il prime sur tout),
   `docs/plan/v2/V2-PARALLEL-PLAN.md` (§1 règles communes, §3 ton chantier
   <ID>, §4 conflits, §5 gabarit), puis les fichiers « Contexte » de ton
   chantier.
3. Ne modifie **que** les fichiers du périmètre de <ID>. Jamais la matrice des
   gates, `docs/decision/*`, `deny.toml`, le `Cargo.toml` racine, ni les
   workflows (sauf L6 : son seul workflow). Pas de nouvelle dépendance.
4. Un RFC est écrit `PROPOSED` ; tu ne l'acceptes jamais. Tu ne supprimes,
   désactives ni n'affaiblis aucun test ni seuil ; `SKIP`/`NOT_RUN` ne sont
   jamais `PASS` ; tu n'écris aucun `PASS` ni `PROVED`.
5. Travaille par petits commits (fmt + clippy + tests des crates touchés avant
   chaque commit), sans identifiant de modèle dans les messages ; pousse
   régulièrement sur `v2/<ID>` (`git push -u origin v2/<ID>`). Pas de force
   push, pas de merge, pas de PR.
6. Si une décision du responsable est nécessaire (dépendance, licence,
   changement de spec normative, ambiguïté), arrête-toi, écris la question
   dans le rapport et pose-la-moi.
7. À la fin : `cargo test --workspace` (+ conformance si le langage est
   touché), rapport `docs/reports/V2-<ID>.md` selon le gabarit §5, commit,
   push, puis donne-moi : branche, SHA final, résumé, état des preuves,
   demandes hors périmètre et changement d'état de gate suggéré.

# Journal des sessions — Ravum UI

## Session 2026-11-24

- Branche `claude/trusting-cerf-4hhv5a`, PR #210 (vers `dev`), base 14015fe.
- Objectifs :
  - consigner les décisions UI-D1 à UI-D10 ;
  - réviser le plan (rév. 2) et RFC-UI-LANG-1 (rév. 2 : OWN-6.8, emprunts bornés) ;
  - écrire le GOAL de reprise ;
  - faire le tour 1 du banc U1-B.
- Fichiers : `docs/decision/DECISIONS-2026-11-24-UI.md`, `docs/ui/{PLAN,GOAL,BENCH-U1B-ROUND1,SESSION-LOG}.md`, `docs/rfc/RFC-UI-LANG-1.md`, `bench/ui-candidates/`.
- Commandes : `cargo run --release -p ui-bench-headless` (code de sortie 0) ; `cargo tree` sur les quatre piles (code de sortie 0).
- Résultats : banc headless `TESTED` ; aucun test Ravum modifié ; workspace Ravum inchangé (le banc est un workspace séparé).
- Décisions attendues :
  - ratifier RFC-UI-LANG-1, dont OWN-6.8 ;
  - décider de la licence de Slint (écarter ou non) ;
  - autoriser la fusion de la PR #210.
- Prochaine action : tour 2 du banc U1-B (même scène dans chaque pile, rendu CPU, mémoire, compilation), et scripts du tour 3 pour vos machines.

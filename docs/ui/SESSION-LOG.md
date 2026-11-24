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

## Session 2026-11-24 (suite)

- Branche `claude/ravum-ui`, PR #211 (UI séparée de la V2, UI-D15). La PR #210 annule les commits UI par un revert.
- Fait :
  - décisions UI-D11 à UI-D16 ;
  - RFC-UI-LANG-1 rév. 3 (règles normatives : vues bornées, déréférencement explicite, points sûrs de hot reload, `.await`, représentation dans l'IR) ;
  - justification sur Slint corrigée ;
  - plan : réactivité A/B et portabilité du hot reload ;
  - tour 2, partie 1, du banc.
- Commande : `cargo run --release -p ui-bench-reactivity` (code de sortie 0).
- Résultats clés :
  - B granulaire à 0,5 µs par changement, contre 1,8 ms pour A sur 10 000 lignes ;
  - Taffy à environ 1,1 ms par changement local sur 10 000 nœuds, même avec des lignes de hauteur fixe.
- Décisions attendues :
  - ratifier les règles normatives de RFC-UI-LANG-1 ;
  - autoriser la fusion de #210 et de #211.
- Prochaine action : tour 2, parties 2 et 3.

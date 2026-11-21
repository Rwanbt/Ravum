# Consigne — session locale : vérifier la priorité temps réel (V1-A3)

À copier-coller tel quel dans une session Claude Code **locale** (machine avec
sortie audio), à la racine d'un clone de `rwanbt/ravum`.

---

Tu reprends une tâche du plan V1 de Ravum (décisions V1-L2/V1-A3,
`docs/decision/DECISIONS-2026-11-19-V1.md`). Lis d'abord `AGENTS.md`, puis
`docs/reports/V1-AUDIO-HOST.md` (section « Priorité temps réel ») et
`crates/ravum-lang-audio/src/device.rs`. Réponds en français.

Contexte : la feature `rt-priority` (désactivée par défaut, implique `cpal`)
promeut **une fois**, au premier rappel audio, le thread du backend en temps
réel via `audio_thread_priority` ; le résultat se lit avec
`CpalHost::rt_status()` (`Disabled`/`Pending`/`Promoted`/`Refused`). Jamais
exécutée sur périphérique : état `NOT_RUN`. Ton travail : la faire tourner ici
et rapporter honnêtement.

1. `git fetch origin claude/trusting-cerf-4hhv5a && git checkout claude/trusting-cerf-4hhv5a`
   (pas de force push, pas de PR, pas de merge).
2. Note OS, noyau (`uname -a` ; est-ce un noyau `PREEMPT_RT` ?), matériel
   audio, rustc, SHA (`git rev-parse HEAD`). Linux : installe
   `libdbus-1-dev` et `libasound2-dev`, vérifie que `rtkit-daemon` tourne.
3. `cargo test -p ravum-lang-audio --features rt-priority`, puis
   `cargo run --release -p ravum-lang-audio --features rt-priority --example play`.
   Relève la ligne `rt_status=…`. Compare avec `--features cpal` seul
   (doit afficher `Disabled`).
4. Vérifie la politique **réelle** du thread pendant la lecture (ne te fie
   pas seulement à `rt_status`) :
   - Linux : `ps -eLo pid,tid,cls,rtprio,comm | grep -i play` (ou
     `chrt -p <tid>`) : attendu `FF`/`RR` avec une priorité > 0 si `Promoted`.
   - Windows : Process Explorer / `Get-Process` → thread avec la classe
     MMCSS « Pro Audio » ; macOS : `Promoted` suffit (time-constraint), note-le.
5. Mesure sous charge : pendant la lecture, charge tous les cœurs (Linux :
   `stress-ng --cpu 0 --timeout 20s`, sinon une boucle de compilation
   `cargo build` d'un autre projet). Avec et sans `rt-priority` : entends-tu
   des craquements ? Si possible, ajoute à `play` (sans changer la bibliothèque)
   une mesure de la durée max de `process` et du nombre de rappels dont la
   durée dépasse l'échéance du bloc ; rapporte les deux séries.
6. Si `Refused` : donne la cause probable (pas de RTKit, limite
   `RLIMIT_RTPRIO`, conteneur…) — c'est un résultat valable, pas un échec à
   masquer. Ne modifie pas le code pour forcer un `Promoted`.
7. Mets à jour `docs/reports/V1-AUDIO-HOST.md` (section « Priorité temps
   réel » : machine, commandes, politique observée, résultats sous charge,
   limites) et la mention `NOT_RUN` de la ligne T16 de
   `docs/roadmap/CORE-1.0-GATE-MATRIX.md` → `TESTED` **seulement** si la
   politique RT a été observée sur le thread ; sinon dire précisément ce qui a
   été observé. Ne revendique aucune garantie hard-RT (un noyau non
   `PREEMPT_RT` peut toujours préempter).
8. Règles : ne désactive ni ne supprime aucun test, n'affaiblis aucun seuil ;
   `SKIP`/`NOT_RUN` ne sont jamais `PASS` ; pas d'identifiant de modèle dans
   les commits ; commit + push sur `claude/trusting-cerf-4hhv5a` à la fin.

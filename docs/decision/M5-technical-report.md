# M5 — rapport technique de décision bootstrap (DRAFT, non ratifié)

> **Statut : DRAFT / EXPERIMENTAL.** Ce rapport pré-enregistre la méthode et
> publie les mesures réellement obtenues. Il ne ratifie aucun ADR, ne choisit
> aucune licence et ne publie aucune release. La décision finale appartient au
> responsable du projet (voir `DECISIONS_REQUIRED.md`). Date : 2026-11-09.

## 1. Question

Faut-il construire le compilateur Ravum sur (A) Rust outillé, (B) un frontend
Ravum propre produisant du Rust auditable, ou (C) un frontend Ravum produisant une
IR propre puis Cranelift/LLVM ? Le critère dominant est le temps total jusqu'au
livrable vérifié (P0-I), sans dégrader les garanties réellement imposées (P0-D).

## 2. Protocole pré-enregistré

- Corpus identique par programme, mêmes algorithmes, mêmes sorties attendues :
  `fib` (récursion + entiers), `calculator` (args, f64, Result, `?`, comparaison
  de texte), `stats` (fichier, UTF-8, lignes, parse f64), `dsp` (slices, Vec,
  boucles, f64).
- A = `rustc -O -C overflow-checks=on` sur `benchmarks/m5/rust/*.rs`.
- B = `ravum-lang build` sur `examples/*` (lexer → parser → checker Ravum →
  codegen Rust → `rustc -O -C overflow-checks=on`).
- C = **non implémenté** dans cette itération ; aucun chiffre C n'est inventé.
- Mesures : compilation (7 réplicats, p50/p95), exécution (15 réplicats,
  p50/p95), taille binaire, égalité des sorties A/B.
- Machine : Windows 11 x86-64, i7-13620H, 15,7 Go, rustc 1.95.0.
- Script : `scripts/m5-bench.ps1`, résultats bruts : `evidence/m5-results.json`.

## 3. Résultats (exécutés le 2026-11-09, Windows 11, i7-13620H)

Script : `scripts/m5-bench.ps1 -Out evidence/m5-results.json` (7 réplicats
compilation, 15 réplicats exécution, `rustc -O -C overflow-checks=on`).
Sorties A et B comparées textuellement : **MATCH sur les 4 programmes**.

| Programme | A compile p50/p95 (ms) | B compile p50/p95 (ms) | A run p50 (ms) | B run p50 (ms) | Taille A/B (octets) | Parité |
|---|---|---|---|---|---|---|
| fib | 180,9 / 506,8 | 216,7 / 280,2 | 6,50 | 6,04 | 125 952 / 145 920 | MATCH |
| calculator | 232,5 / 239,1 | 260,0 / 272,2 | 6,85 | 6,09 | 177 664 / 218 112 | MATCH |
| stats | 267,1 / 269,2 | 250,0 / 279,7 | 6,25 | 5,62 | 190 976 / 243 200 | MATCH |
| dsp | 194,1 / 297,8 | 254,6 / 301,6 | 7,72 | 5,33 | 148 480 / 175 616 | MATCH |

Lecture honnête :
- B ajoute le coût du frontend+checker+codegen : +12 à +31 % sur la compilation
  p50 (stats est dans le bruit, −6 %), pour un binaire 16 à 26 % plus gros
  (prélude inclus). Aucun gain de performance d'exécution revendiqué : les écarts
  de run sont dans le bruit (processus courts).
- C n'a **aucun** chiffre : `NOT_IMPLEMENTED`. Le tableau n'est donc pas complet
  et ne permet pas de conclure que B est le meilleur backend possible, seulement
  qu'il est une chaîne réelle et vérifiée.
- Les p95 de compilation sont bruités par la charge machine (un pic à 506 ms pour
  fib A) ; les réplicats bruts sont conservés dans le JSON.

## 4. Différences sémantiques assumées entre A et B

- B vérifie la sémantique Ravum **avant** émission : moves, borrows non
  échappants, exhaustivité, effets. Un programme Rust valide peut être refusé par
  Ravum (sous-ensemble conservateur) ; un programme Ravum accepté est un
  sous-ensemble Rust sûr.
- B impose `overflow-checks=on` en debug et release ; A dépend du profil.
- Les références ne peuvent être ni stockées ni retournées en Core 1.0 (UNSUPPORTED) :
  ce n'est pas une différence de performance mais de garantie.
- B ne prétend pas à un ABI stable ; A est Rust.

## 5. Décision provisoire et alternatives

- **B retenue provisoirement** comme backend de référence : elle fournit une
  chaîne `.rv` réelle et vérifiée, avec un code Rust auditable, sans dépendre de
  `rustc_private` ni d'un linker maison. Rust n'est qu'un générateur de code
  machine après vérification Ravum.
- **C reste expérimentale** : interface backend réservée, aucun résultat publié,
  aucun statut supported tant qu'elle n'aura pas passé les mêmes tests de
  sémantique (mémoire, ABI, unwinding, cibles).
- **A reste la référence de mesure**, pas le produit : elle ne satisfait pas
  l'exigence d'un langage Ravum autonome.
- Si B ne tient pas ses engagements (parité, sûreté, ergonomie), A reste le
  fallback honnête et le langage reste ALPHA/PARTIAL ; aucun PASS ne sera simulé.

## 6. Limites

- Corpus petit et déterministe : ne généralise pas à toutes les charges.
- Une seule machine, un seul OS qualifié ici (Windows) ; Linux = à exécuter.
- C non mesuré : le tableau de décision est incomplet par construction.
- Aucun audit humain indépendant de ce rapport.

## 7. Proposition d'ADR (non acceptée)

« Utiliser un frontend Ravum autonome et un backend Rust auditable comme chaîne
de référence Core 1.0 ; garder une interface backend versionnée pour une cible
native future ; ne déclarer aucun backend "stable" sans passer la conformance,
la parité debug/release et les tests ABI/mémoire. » — **PROPOSED**, numéro à
attribuer par le responsable, aucun effet normatif tant que non ratifié.

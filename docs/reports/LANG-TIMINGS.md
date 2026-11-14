# LANG-TIMINGS — mesure par phase de la boucle `.rv` (2026-11-13)

> **Document de mesure, pas une promesse de performance.** Tous les chiffres
> viennent de `scripts/lang-timings.ps1`, exécuté sur cette machine, à ce SHA.
> `NOT_RUN` et les limites sont explicites. Aucun chiffre n'est arrondi en faveur
> du produit : les valeurs défavorables sont en tête de chaque tableau.

## 1. Identités

| Élément | Valeur |
|---|---|
| SHA de la mesure | `3b6eb755e8dfe0255bcdb1b8ffa83e0716abb455` |
| Branche | `goal/production-ready` |
| Machine | Windows 11 Professionnel 10.0.26100 x86-64 |
| CPU | 13th Gen Intel Core i7-13620H, 10 cœurs / 16 threads |
| RAM | 16 868 962 304 octets (≈ 15,7 Gio) |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)` |
| Campagne | 20 exécutions à froid + 20 après une ligne modifiée, par exemple et par commande |
| Sortie brute | `target/lang-timings/lang-timings.json` (non versionnée ; schéma ci-dessous) |

Commande exacte :

```powershell
pwsh -File scripts/lang-timings.ps1 -Runs 20
```

Le script copie chaque paquet dans `target/lang-timings/work/` (jamais
`examples/`), supprime `.ravum/` avant chaque exécution « froide », ajoute une
ligne `// lang-timings edit N` avant chaque exécution « éditée », puis calcule
p50/p95 par la méthode du rang le plus proche (20 valeurs ⇒ p95 = 19ᵉ valeur
triée). Les durées mesurées commencent **dans** le binaire : le lancement du
processus et l'ordonnancement du système ne sont pas comptés.

## 2. `check` — durée totale (ms)

| Exemple | froid p50 | froid p95 | édition p50 | édition p95 |
|---|---|---|---|---|
| `calculator` | 1,7 | 1,9 | 11,1 | **49,8** |
| `dsp-gain-iir` | 1,8 | 2,0 | 11,2 | 13,9 |
| `ffi-abs` | 1,1 | 1,3 | 8,9 | 11,0 |
| `fib` | 1,3 | 1,3 | 10,1 | 15,0 |
| `file-store` | 5,6 | 6,7 | 16,0 | 21,4 |
| `file-tool` | 2,4 | 8,1 | 18,3 | 20,7 |
| `hello` | 1,4 | 1,7 | 14,2 | 16,2 |
| `package-demo` | 2,5 | 2,9 | 16,6 | 20,4 |
| `stats` | 2,9 | 3,3 | 16,7 | 20,5 |
| `tests-demo` | 2,1 | 2,5 | 15,2 | 17,8 |

**Lecture honnête.** À froid, `check` reste sous **8,1 ms** de p95 sur tous les
exemples. En mode « édition », le p95 de `calculator` atteint **49,8 ms**, soit
juste sous la cible P0-A de 50 ms ; le poste de coût est `load` (~10-20 ms),
c'est-à-dire la lecture d'un fichier **venant d'être écrit** (cache disque,
antivirus, journalisation NTFS), pas une phase du compilateur : les phases
`parse`/`resolve`/`typeck`/`effects`/`own` sont identiques dans les deux modes
(§4). Un seul exemple frôle la cible ; trois dépassent 20 ms. La cible P0-A
concerne le **diagnostic LSP après édition**, qui n'est pas mesuré ici : la
phase 7 le mesurera séparément. **Aucune revendication P0-A n'est faite dans ce
rapport.**

## 3. `build` — durée totale (ms) et part de `rustc`

| Exemple | froid p50 | froid p95 | `rustc` p50 | part `rustc` | édition p50 | édition p95 |
|---|---|---|---|---|---|---|
| `calculator` | 232,4 | **613,9** | 230,1 | 99,0 % | 235,7 | 497,5 |
| `dsp-gain-iir` | 199,2 | **646,0** | 196,8 | 98,8 % | 209,0 | 231,0 |
| `ffi-abs` | 179,4 | 188,6 | 177,7 | 99,1 % | 186,9 | 191,9 |
| `fib` | 180,1 | 201,4 | 178,3 | 99,0 % | 188,4 | 191,8 |
| `file-store` | 263,3 | 269,2 | 255,7 | 97,1 % | 278,6 | 375,1 |
| `file-tool` | 330,8 | 403,3 | 328,0 | 99,2 % | 321,9 | 341,2 |
| `hello` | 241,4 | 258,4 | 239,8 | 99,3 % | 249,1 | 264,2 |
| `package-demo` | 247,3 | 263,1 | 243,8 | 98,6 % | 260,1 | 282,4 |
| `stats` | 304,9 | 331,6 | 301,4 | 98,9 % | 314,6 | 339,6 |
| `tests-demo` | 243,0 | 266,6 | 239,8 | 98,7 % | 263,5 | 272,7 |

**Deux p95 aberrants assumés** : `calculator` 613,9 ms et `dsp-gain-iir`
646,0 ms, pour des p50 de 232,4 et 199,2 ms. Le détail brut montre une seule
exécution lente parmi les 20 (probablement antivirus/IO) ; la valeur est publiée
telle quelle, pas filtrée. Le frontend complet (load + parse + resolve + typeck +
effects + own + codegen) représente **0,7 % à 2,9 %** de la durée ; `rustc`
domine à **97,1-99,3 %**. C'est la justification factuelle de la décision D4 de
l'audit (mesurer avant d'optimiser) et la raison pour laquelle
l'incrémentalité frontend (phase 7) ne peut pas, seule, rendre `build` rapide.

## 4. Répartition par phase (échantillons)

`check`, `hello`, à froid (ms) :

| Phase | p50 | p95 |
|---|---|---|
| load | 1,100 | 1,385 |
| parse | 0,077 | 0,102 |
| resolve | 0,065 | 0,084 |
| typeck | 0,078 | 0,109 |
| effects | 0,033 | 0,046 |
| own | 0,017 | 0,026 |
| **total** | **1,385** | **1,699** |

`build`, `file-store`, à froid (ms) :

| Phase | p50 | p95 |
|---|---|---|
| load | 1,594 | 1,696 |
| parse | 1,577 | 1,800 |
| resolve | 0,222 | 0,244 |
| typeck | 1,475 | 1,703 |
| effects | 0,237 | 0,272 |
| own | 0,948 | 1,024 |
| codegen | 1,511 | 1,632 |
| rustc | 255,703 | 261,529 |
| **total** | **263,259** | **269,228** |

En mode « édition », seule la phase `load` augmente (≈ 20 ms pour
`file-store`) : la lecture d'un fichier fraîchement modifié coûte ce que le
système de fichiers et l'antivirus décident, pas le compilateur.

## 5. `-C incremental` : mesure et décision (désactivé par défaut)

Protocole : même script, `RAVUM_INCREMENTAL=1`, 5 exécutions par mode, sur trois
exemples ; plus une mesure contrôlée sur un programme synthétique de
**800 fonctions** (4 constructions par fonction) avec/sans incrémental, à froid
et après une ligne modifiée. Résultats (p50, ms) :

| Programme | froid sans | froid avec | édition sans | édition avec |
|---|---|---|---|---|
| `hello` | 241,4 | **262,9** | 249,1 | **274,3** |
| `file-store` | 263,3 | **379,9** | 278,6 | **398,7** |
| `package-demo` | 247,3 | **257,6** | 260,1 | **272,6** |
| synthétique 800 fonctions (`rustc` seul) | 784,9 | **863,3** | 789,1 | **868,0** |

**Le gain est nul ou négatif partout** (jusqu'à +44 % sur `file-store`). Cause
probable : un unique fichier Rust par programme, un `rustc` déjà dominé par le
link/codegen, et des sessions incrémentales qui paient leur propre lecture/écriture
sans réutiliser grand-chose. **Décision (3.4)** : l'option existe
(`RAVUM_INCREMENTAL=1`) mais reste **désactivée par défaut** ; les builds
`--release` et `test` ne la reçoivent jamais. `ravum-lang cache clean` supprime
le répertoire `incr/` avec le reste de `.ravum/`.

## 6. Reproductibilité `--release` (3.5)

`pwsh -File scripts/repro-check.ps1` (deux racines, deux `target/`, drapeaux
`CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0`, `--remap-path-prefix`,
`-C link-arg=/Brepro`) sur l'arbre de ce SHA :

```
A = 0C7B63C66C27B0874AF5679FB0576C63A3DFC7B4B76C0E32A7AD8337A6AB64A8
B = 0C7B63C66C27B0874AF5679FB0576C63A3DFC7B4B76C0E32A7AD8337A6AB64A8
PASS — the two builds are bit-for-bit identical
```

Ce digest est celui du **binaire `ravum-lang`** construit depuis deux racines
différentes, pas le digest d'un programme `.rv` compilé. Les builds release ne
reçoivent jamais `-C incremental` (la branche retourne `None` pour
`options.release`), donc la reproductibilité release n'est pas affectée par la
phase 3.3 ; le contrôle ci-dessus le mesure sur l'arbre courant. Le digest est
propre à cette machine et à cette toolchain, comme documenté dans
`docs/qualification/REPRODUCIBILITY.md`.

## 7. Ce que cette mesure ne prouve pas

- Une seule machine, un seul OS, une seule toolchain ; aucun test sous charge
  concurrente, aucun test multi-images.
- Les exemples sont petits (1-5 fichiers, ≤ quelques centaines de lignes) ; les
  durées ne s'extrapolent pas à un gros programme.
- Le p95 sur 20 valeurs est instable par construction (une exécution lente le
  déplace) ; c'est pourquoi p50 et valeurs brutes sont publiées ensemble.
- La latence LSP après édition (cible P0-A) n'est **pas** mesurée ici :
  `NOT_RUN`, phase 7.
- Aucune mesure de mémoire, de CPU consommé ni de temps de linking séparé.

## 8. Corpus à l'échelle (2026-11-14) — la valeur défavorable est en tête

> **Base : `c61cb92`** (T4), branche `goal/consolidation-2`, scripts T3 non encore
> commités au moment de la mesure. Commande :
> `pwsh -File scripts/lang-timings.ps1 -Scale -ScaleRuns 2 -Output target/lang-timings/scale.json`.
> Les paquets sont générés par `tools/gen_scale_corpus.py` (déterministe) dans
> `target/lang-timings/work/` ; chaque module déclare une struct et F fonctions,
> la première fonction d'un module appelle celle du module précédent
> (`crate::mK-1::fK-1_0`), `main` appelle la dernière. `ScaleRuns = 2` (médiane
> de deux exécutions, donc moyenne des deux) : c'est peu, et publié comme tel.
> Les valeurs sont en **millisecondes**, `p50` sur deux exécutions.

**Le fait défavorable, en premier : `check` n'est pas linéaire.** Entre 5 110 et
50 110 lignes (×9,8 de code), `check` passe de **365 ms à 26 424 ms** (×72). La
phase `own` (analyse d'ownership) porte l'essentiel : **25 758 ms sur 26 424 ms**
(97 %) à 50 000 lignes, contre 262 ms sur 365 ms (72 %) à 5 000 lignes. Ce n'est
pas `rustc` : à 50 000 lignes il ne représente plus que **1 546 ms sur 27 632 ms
(5,6 %)** d'un build complet, contre 88 % à 1 000 lignes.

| Taille (lignes) | Modules | Fonctions/module | Fichiers | `check` p50 | `check` wall | Pic RSS | dont `own` | `own` % |
|---|---|---|---|---|---|---|---|---|
| **50 110** | 150 | 66 | 152 | **26 423,5** | 26 659,5 | **151 Mo** | 25 758,3 | 97,5 % |
| **20 110** | 150 | 26 | 152 | **4 625,0** | 4 818,0 | 66 Mo | 4 342,6 | 93,9 % |
| **5 110** | 150 | 6 | 152 | **364,9** | 411,1 | 21 Mo | 261,6 | 71,7 % |
| **1 025** | 35 | 5 | 37 | **34,0** | 62,1 | 10 Mo | 11,5 | 33,8 % |

Répartition complète de `check` (p50, ms) :

| Taille | load | parse | resolve | typeck | effects | own | total |
|---|---|---|---|---|---|---|---|
| **50 110** | 159,4 | 213,9 | 49,9 | 159,7 | 40,0 | **25 758,3** | 26 423,5 |
| **20 110** | 88,0 | 85,0 | 18,9 | 64,2 | 16,8 | **4 342,6** | 4 625,0 |
| **5 110** | 50,1 | 26,1 | 5,1 | 15,5 | 3,2 | **261,6** | 364,9 |
| **1 025** | 11,9 | 5,4 | 0,9 | 2,7 | 0,5 | **11,5** | 34,0 |

`build` (p50, ms) — froid, froid avec `RAVUM_INCREMENTAL=1` (il n'existe **pas**
d'option `--incremental` ; l'option est la variable d'environnement), rebuild
après édition d'une ligne, et le même avec la variable :

| Taille | froid | dont `rustc` | froid + incrémental | édition | édition + incrémental | Pic RSS |
|---|---|---|---|---|---|---|
| **50 110** | 27 632,1 | 1 545,9 (5,6 %) | 27 394,9 | 26 716,6 | 27 178,4 | **236 Mo** |
| **20 110** | 5 238,8 | 836,5 (16,0 %) | 5 538,3 | 5 347,7 | 5 438,1 | 101 Mo |
| **5 110** | 903,0 | 510,5 (56,5 %) | 1 062,3 | 901,6 | 1 091,9 | 33 Mo |
| **1 025** | 338,6 | 299,0 (88,3 %) | 469,2 | 360,7 | 425,8 | 13 Mo |

Constats mesurés, sans extrapolation :

1. **`own` est superlinéaire** dans cette forme de paquet : ×6,3 de lignes entre
   1 025 et 5 110 donne ×22,7 de temps `own` (11,5 → 261,6 ms) ; ×1,33 entre
   20 110 et 50 110 donne ×5,9 (4 342,6 → 25 758,3 ms). La cause n'est **pas**
   établie par cette mesure (pas de profil) : c'est un fait, pas un diagnostic.
2. **`rustc` est négligeable à l'échelle** sur cette machine : la décision
   d'incrémental `-C` ne peut pas restaurer la boucle tant que le frontend
   domine (audit N2 était « inconnu à l'échelle » ; il est maintenant mesuré
   défavorable).
3. **L'option incrémentale ne rend rien** à aucune taille mesurée : elle est
   plus lente à froid (1 062,3 vs 903,0 ms à 5 k ; 469,2 vs 338,6 ms à 1 k) et
   au mieux égale après édition (27 178,4 vs 26 716,6 ms à 50 k, soit +1,7 %).
   La décision « désactivée par défaut » du §5 reste valable et est renforcée.
4. **Mémoire** : le pic du processus `ravum-lang` est de 151 Mo (`check`) et
   236 Mo (`build`) à 50 000 lignes — mesuré par échantillonnage du working set
   vivant, **hors processus `rustc`** (les enfants ne sont pas comptés).
5. **LSP de bout en bout à l'échelle : `NOT_RUN`.** La mesure existante
   (§7, p95 2,826 ms) porte sur une édition d'une ligne dans un petit paquet ;
   aucun client n'a été écrit pour un corpus de cette taille dans cette passe.

Limites de cette section : corpus **synthétique** régulier (pas un programme
réel, aucun `Vec`/trait/closure) ; deux exécutions par point ; une machine ;
M ≤ 150 parce que `MAX_SOURCE_FILES` valait 200 au moment de la mesure (T4 le
porte à 65 536, la campagne n'a pas été rejouée après) ; les `p50` des phases
viennent des `timings` du CLI, pas d'un profileur.


## 9. LSP de bout en bout à l'échelle (2026-11-14, T3)

> **Base : `379b6fe`**, branche `goal/consolidation-2`. Le paquet est généré par
> le test lui-même, avec la même forme que `tools/gen_scale_corpus.py`
> (déterminisme vérifié contre le générateur Python : **5 110 lignes**, 150
> modules × 6 fonctions, 152 fichiers). Commande :
> `cargo test -p ravum-lang-cli --test lsp_client end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture`.
> 20 éditions d'une ligne dans `main` (un `let` inutilisé ajouté à chaque tour),
> chaque tour attendant `textDocument/publishDiagnostics` ; cadrage JSON-RPC,
> serveur, analyse et publication inclus ; fichiers temporaires uniquement.

**Le fait défavorable, en premier : la latence suit le paquet, pas la ligne
éditée.** p95 **337,6 ms** à 5 110 lignes contre **2,826 ms** sur
`package-demo` (2 fichiers) : **×119**. La cible P0-A « diagnostic p95 < 50 ms »
est tenue sur les petits paquets, **pas à cette échelle** ; le coût est celui du
`check` complet (365 ms p50 à 5 110 lignes, §8), donc l'analyse sémantique reste
**globale** et le cache de parse ne réduit que le parsing.

| Corpus | Profil | Lignes | Modules | Éditions | p50 | p95 | Cible 50 ms |
|---|---|---|---|---|---|---|---|
| `package-demo` | debug | 338 (2 fichiers) | — | 20 | **2,061 ms** | **2,826 ms** | tenue |
| échelle générée | debug | **5 110** | 150 | 20 | **322,4 ms** | **337,6 ms** | **non tenue** |
| échelle générée | **release** | **5 110** | 150 | 20 | **116,7 ms** | **163,5 ms** | **non tenue** |

Le profil `release` est mesuré séparément le 2026-11-14 sur `214bf5e`
(`cargo test --release -p ravum-lang-cli --test lsp_client
end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture`) :
le coût est divisé par ~2 par rapport au binaire debug, mais **reste ×3,3
au-dessus de la cible** à 5 110 lignes. Publier 337,6 ms comme chiffre produit
serait faux (c'est un binaire debug) ; publier 163,5 ms comme « cible tenue »
serait faux aussi. La décision de périmètre (cible réservée aux petits paquets)
ou d'incrémentalité sémantique est humaine
([`HUMAN-DECISIONS-REQUIRED.md`](HUMAN-DECISIONS-REQUIRED.md),
`D-2026-11-14-5`).

Preuve fonctionnelle associée : `scale_package_navigation_and_diagnostics_work_end_to_end`
(35 modules, 1 025 lignes) vérifie qu'une définition traversant les modules
aboutit dans `m34.rv` et qu'un aller-retour diagnostic (erreur d'arité puis
retour au texte sain) publie puis efface le diagnostic.

Ce que cela ne prouve pas : ni réseau, ni éditeur réel, ni corpus au-delà de
5 110 lignes, ni plusieurs machines.

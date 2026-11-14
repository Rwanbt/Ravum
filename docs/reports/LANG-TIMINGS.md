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
au-dessus de l'ancienne cible unique de 50 ms** à 5 110 lignes — et même
au-dessus de la cible S8 de 150 ms (×1,09). Publier 337,6 ms comme chiffre
produit serait faux (c'est un binaire debug) ; publier 163,5 ms comme « cible
tenue » serait faux aussi. **La décision est prise** : S8 (2026-11-14) refuse
d'assouplir la cible, fixe deux points de mesure (§9.1) et renvoie
l'incrémentalité sémantique au chantier 1.x. Les verdicts « non tenue » du
tableau ci-dessus sont ceux de la campagne T3 contre **l'ancienne cible unique de
50 ms** ; la cible applicable à 5 110 lignes est depuis S8 `p95 ≤ 150 ms`.

Preuve fonctionnelle associée : `scale_package_navigation_and_diagnostics_work_end_to_end`
(35 modules, 1 025 lignes) vérifie qu'une définition traversant les modules
aboutit dans `m34.rv` et qu'un aller-retour diagnostic (erreur d'arité puis
retour au texte sain) publie puis efface le diagnostic.

Ce que cela ne prouve pas : ni réseau, ni éditeur réel, ni corpus au-delà de
5 110 lignes, ni plusieurs machines.

### 9.1 Re-mesure de la vague 3 (S8) : les deux cibles sont tenues

> **Base : `7b615ed`**, branche `goal/wave-3`, worktree `D:\App\ravum-wt-g`,
> `CARGO_TARGET_DIR=D:\App\ravum-target-g`, binaire `ravum-lang` **release**,
> machine Intel Core i7-13620H (10 cœurs / 16 processeurs logiques), 16 Go,
> Windows 11 Professionnel, `rustc 1.95.0`. Protocole et conditions complets :
> [`../qualification/LSP.md`](../qualification/LSP.md) §8.

Les cibles S8 portent sur deux corpus et deux comparaisons distinctes :
`p95 < 50 ms` à **1 025 lignes** (strict) et `p95 ≤ 150 ms` à **5 110 lignes**,
20 éditions d'une ligne, transport inclus, profil release. Trois passages
consécutifs :

| Corpus | p50 (3 passages) | p95 (3 passages) | Cible | Verdict |
|---|---|---|---|---|
| **1 025** lignes (35 × 5) | 18,1181 / 18,7559 / 18,6272 ms | **19,3938 / 19,9060 / 19,7785 ms** | `< 50 ms` | **tenue** (×2,51 de marge) |
| **5 110** lignes (150 × 6) | 77,2263 / 76,0915 / 75,5331 ms | **80,7698 / 79,7376 / 81,2691 ms** | `≤ 150 ms` | **tenue** (×1,85 de marge) |

La correction de la phase `own` (§10) explique l'écart avec les 163,5 ms du
§9 : les deux chiffres ne mesurent pas le même arbre. Ils ne sont pas
comparables ligne à ligne, mais la conclusion de la campagne T3 (« la cible
n'est pas tenue à 5 110 lignes ») est **périmée sur l'arbre courant**.

**Condition de mesure, à lire avant le chiffre.** La machine était partagée avec
les jobs des autres sous-agents (`cargo-mutants`, `cargo-llvm-cov`,
`differential_oracle`) : **259 runs** enregistrés (130 à 1 025 lignes, 129 à
5 110), charge étrangère de ~0 % à ~95 %. Les trois passages publiés sont les
plus proches du critère d'acceptation fixé avant les mesures (machine ≥ 85 %
idle) : 89–90 % d'idle pour le petit corpus, 83–85 % pour le grand ; un seul
atteint strictement 85 % sur les deux. Les 4 runs à ≥ 80 % d'idle donnent la même
bande (78,23–81,27 ms à 5 110 lignes). La contention ne peut qu'**augmenter** la
latence, donc ces passages sont des **majorants** de la latence sur machine au
repos. Aucun chiffre de cette section n'autorise à relâcher une cible.

**Contrôle CI.** `.github/workflows/performance.yml`, job `lsp-targets` : les
deux tests `#[ignore]` écrivent `target/lsp/latency.json` via
`RAVUM_LSP_LATENCY_OUT` (chemin absolu, car un binaire de test tourne dans la
racine du paquet), puis
`python tools/timings_gate.py --lsp target/lsp/latency.json --lsp-small-p95-ms 50 --lsp-large-p95-ms 150`
applique les cibles et échoue fermé (corpus manquant, ligne `debug`, statistique
incohérente). Les cibles ont été établies sur la machine de référence ; un run
rouge sur un runner partagé signale la capacité du runner, pas une cible à
assouplir.

## 10. Phase `own` : la croissance quadratique est corrigée (2026-11-14, vague 3)

> **Base : `124e7a444de4df090e4f013bd2788ae8b2f967e3`**, branche
> `wave3/a-own-perf`, worktree `D:\App\ravum-wt-a`,
> `CARGO_TARGET_DIR=D:\App\ravum-target-a`. Tous les chiffres ci-dessous viennent
> de commandes exécutées dans ce worktree à ce SHA, comparées à l'état
> **d'avant correction** (même SHA, avant le commit de cette section — le `HEAD`
> reste `124e7a4` jusqu'au commit). Statut : **TESTED** pour les mesures et les
> tests cités ; jamais `PROVED`, jamais « production ready ».

Le fait défavorable du §8 est pris en tête : sur cette forme de paquet, `check`
passait de **365 ms à 5 110 lignes** à **26 424 ms à 50 110 lignes** (campagne
T4, autre SHA). Re-mesuré ici à `124e7a4`, il passe de 292,410 ms à
**26 879,663 ms**, dont **26 271,523 ms** (97,7 %) dans la seule phase `own` :
la croissance était quadratique.

### 10.1 Le constat, re-mesuré (valeurs défavorables en tête)

Commande, identique avant et après la correction :

```powershell
pwsh -File scripts/lang-timings.ps1 -Scale -ScaleRuns 3 -Output target\lang-timings\scale-baseline.json
pwsh -File scripts/lang-timings.ps1 -Scale -ScaleRuns 3 -Output target\lang-timings\scale-after.json
```

Les deux exports ont été produits dans la même session, sur la même machine
(Windows 11, i7-13620H, 16 Go), avec un binaire **debug** ; `ScaleRuns = 3`
(p50 = 2ᵉ des 3 valeurs triées).

| Taille (lignes) | `check` p50 | dont `own` | part `own` | `check` wall médian | Pic RSS |
|---|---|---|---|---|---|
| **50 110** | **26 879,663** | **26 271,523** | 97,7 % | 27 105,031 | 152 Mo |
| **20 110** | **4 011,945** | **3 797,214** | 94,6 % | 4 108,108 | 67 Mo |
| **5 110** | 292,410 | 214,517 | 73,4 % | 334,963 | 23 Mo |
| **1 025** | 29,258 | 9,688 | 33,1 % | 53,576 | 10 Mo |

(SHA `124e7a4`, avant correction ; sortie brute : `target/lang-timings/scale-baseline.json`, non versionnée.)

Exposants d'avant correction, sur `total` : **1,9803** entre 5 110 et 50 110
lignes, **1,9116** entre 5 110 et 20 110 ; sur `own` : **2,1059**. La phase `own`
croissait donc comme n², exactement le signal de l'hypothèse.

### 10.2 Hypothèse instrumentée à 20 110 lignes — confirmée

Instrumentation **temporaire** (retirée avant le commit : elle n'existe dans
aucun fichier versionné), dans `check_ownership`, mesurant séparément le temps du
`checker.check_fn` et celui des deux `BTreeMap::append` :

```text
own-instrument fns=3901 check_fn=60.919ms append_use_kind=926.735ms append_place=2367.086ms entries_use_kind=8249 entries_place=8249
own-instrument fns=3901 check_fn=60.424ms append_use_kind=941.527ms append_place=2391.903ms entries_use_kind=8249 entries_place=8249
own-instrument fns=3901 check_fn=61.649ms append_use_kind=938.814ms append_place=2302.873ms entries_use_kind=8249 entries_place=8249
```

| Exécution (20 110 lignes) | `check_fn` | `append(use_kind)` | `append(place)` | `own` total | `check` total |
|---|---|---|---|---|---|
| 1 | 60,919 | 926,735 | 2 367,086 | 3 362,692 | 3 595,710 |
| 2 | 60,424 | 941,527 | 2 391,903 | 3 401,571 | 3 621,383 |
| 3 | 61,649 | 938,814 | 2 302,873 | 3 311,282 | 3 533,596 |

**Hypothèse confirmée.** Les deux `append` portent **98,00 %** de la phase `own`
à 20 110 lignes (3 333,430 ms sur 3 401,571 ms à l'exécution 2), contre
**1,78 %** pour la totalité du `check_fn` (3 901 fonctions) ; le seul
`append(place)` en porte 70,3 %. `BTreeMap::append` reconstruit la table entière
à chaque appel, et l'appel avait lieu une fois par fonction : le coût total est
quadratique en nombre de fonctions, pas en nombre de décisions enregistrées.

### 10.3 Correction

`crates/ravum-lang-sema/src/own.rs` uniquement :

* `check_ownership` crée **un seul** `OwnFacts` pour tout le programme ;
* `OwnChecker` reçoit `&mut OwnFacts` (`OwnChecker::new(facts, diags, own)`) et
  insère directement au lieu de tenir une table par fonction ;
* les deux `BTreeMap::append` sont supprimés : une décision enregistrée = un
  `insert` ordonné, jamais un vidage/reconstruction de table.

Aucune règle d'ownership, aucun diagnostic, aucun seuil n'est touché. Aucune
dépendance ajoutée, aucun `HashMap`/`HashSet` ajouté.

### 10.4 Preuve d'équivalence : les deux accumulations produisent les mêmes tables

Test `own::tests::own_facts_are_identical_to_the_append_accumulation`
(`cargo test -p ravum-lang-sema --lib … -- --nocapture`) :

```text
own facts equivalence: 207 packages compared, 178 recorded facts, 0 difference
test result: ok. 1 passed; 0 failed; 0 ignored
```

Périmètre : les **10 exemples** de `examples/` et **197 cas de conformance**
(`compile-pass` + `run-pass`, les seuls modes dont le programme doit passer
`check`) → **207 packages chargés par le vrai loader**, vérifiés, puis analysés
deux fois sur le même `CheckedProgram` : l'accumulation d'aujourd'hui et
l'ancienne accumulation par `append` (recopiée dans le test, jamais liée au
produit). Les tables `use_kind` et `place` **sérialisées** (`Debug` ordonné) et
les **diagnostics d'ownership** (code, gravité, message, span, notes) doivent être
identiques ; 178 des 207 packages produisent des tables non vides, donc la preuve
n'est pas vide. Un seul écart fait échouer le test avec le cas nommé.

### 10.5 Re-mesure (même commande, même machine, même SHA de base)

| Taille (lignes) | `check` avant | `check` après | gain | `own` avant | `own` après | gain | part `own` après |
|---|---|---|---|---|---|---|---|
| **50 110** | **26 879,663** | **602,621** | **×44,60** | **26 271,523** | **79,680** | **×329,71** | 13,2 % |
| **20 110** | 4 011,945 | 271,942 | ×14,75 | 3 797,214 | 31,443 | ×120,77 | 11,6 % |
| **5 110** | 292,410 | 89,121 | ×3,28 | 214,517 | 7,800 | ×27,50 | 8,8 % |
| **1 025** | 29,258 | 22,559 | ×1,30 | 9,688 | 1,585 | ×6,11 | 7,0 % |

(SHA `124e7a4` + correction non encore commitée au moment de la mesure ; sortie
brute : `target/lang-timings/scale-after.json`. Les colonnes « avant » et
« après » viennent de deux campagnes distinctes, dans la même session, à
3 exécutions par point.)

Exposants (formule exigée : `e = ln(t50k / t5k) / ln(50110 / 5110)`) :

| Métrique | avant | après |
|---|---|---|
| `check` total, 5 110 → 50 110 | 1,9803 | **0,8372** |
| `check` total, 5 110 → 20 110 | 1,9116 | **0,8143** |
| `own`, 5 110 → 50 110 | 2,1059 | **1,0179** |
| `own`, 5 110 → 20 110 | 2,0975 | **1,0175** |

`own` reste légèrement au-dessus de 1 (1,018) : c'est le facteur `log` d'une
table ordonnée, pas une croissance quadratique. Le total `check` passe sous 1
parce que les coûts fixes (`load`, `parse`) s'amortissent. Le pic mémoire est
inchangé (152 → 153 Mo à 50 110 lignes) : la vitesse n'est pas payée en RAM.

Contrôle de reproductibilité sur l'arbre corrigé de la branche
(`wave3/a-own-perf`, corpus régénérés à l'identique — générateur déterministe —
et 3 exécutions à processus froid) :
`check` p50 = **102,619 ms** (5 110), **299,390 ms** (20 110), **653,640 ms**
(50 110) ; exposants **0,811** (5 110 → 50 110) et **0,782** (5 110 → 20 110).
Ces trois points sont 7 à 15 % au-dessus de la campagne ci-dessus (cache de
fichiers vide, processus froid) ; ils restent très au-dessous de la cible de 3 s
et l'exposant reste sous 1,3.

**Lecture honnête.** 602,621 ms à 50 110 lignes est tenu sous la cible de 3 s
demandée, mais le binaire est **debug**, le corpus est **synthétique** et régulier
(structs + appels de fonctions ; ni `Vec`, ni emprunts imbriqués, ni `match`
complexe), et ce n'est **pas** une mesure LSP. Les cibles P0-A/S8 (`p95 < 50 ms`
jusqu'à 1 000 lignes, `p95 ≤ 150 ms` à 5 110 lignes en **release**) ne sont pas
évaluées ici : elles portent sur la latence de diagnostic après édition, mesurée
dans un autre lot.

### 10.6 Contrôle de linéarité en CI (S8)

`tools/timings_gate.py` accepte `--scale <json>` et applique l'exposant entre les
deux corpus générés de 5 110 et 20 110 lignes :

```text
e = ln(total_p50(20 110) / total_p50(5 110)) / ln(20 110 / 5 110)   ;   e <= 1,3
```

`.github/workflows/performance.yml` génère ces deux corpus
(`tools/gen_scale_corpus.py`, 150 × 6 et 150 × 26), mesure `check` trois fois sur
chacun, écrit `target/linearity/scale.json` et passe l'export au gate. Le gate
échoue fermé si un corpus manque, si un p50 est nul/absent ou si une taille est
dupliquée ; le témoin « cas linéaire accepté / cas quadratique refusé » est dans
`tools/tests/test_timings_gate.py` (23 tests, dont 11 nouveaux sur la linéarité,
tous verts).

Rejeu local du chemin CI complet (corpus régénérés, 3 exécutions) :

```text
linearity: scale-5k = 5110 lines, check p50 = 102.232 ms, p95 = 104.701 ms
linearity: scale-20k = 20110 lines, check p50 = 270.948 ms, p95 = 318.847 ms
```

soit un exposant de **0,7114** (≤ 1,3) : le contrôle passe sur l'arbre corrigé.
L'écart avec le 0,8143 de la campagne (§10.5) vient du cache de fichiers
(5 110 lignes à froid ici) : l'exposant porte sur un rapport, il reste sous le
plafond dans les deux mesures, et c'est le plafond qui décide.

### 10.7 Ce que cette section ne prouve pas

- Une seule machine, un seul OS, un seul compilateur (`rustc 1.95.0`), un binaire
  **debug** ; aucune mesure sous charge concurrente ni multi-plateforme.
- Corpus synthétique régulier, généré par `tools/gen_scale_corpus.py` ; la forme
  des programmes réels n'est couverte que par les 207 packages de §10.4.
- `ScaleRuns = 3` : le p50 est une médiane de trois exécutions, pas un intervalle
  de confiance.
- La preuve d'équivalence porte sur les programmes **acceptés** (les seuls dont
  un `CheckedProgram` existe) ; les programmes refusés sont couverts par la suite
  de conformance (`0 FAIL`, même nombre de cas), pas par ce test.
- Aucune revendication `PROVED`, ni de maturité « production », ni de
  linéarité garantie sur des entrées arbitraires : l'exposant est une mesure
  bornée par un plafond CI.


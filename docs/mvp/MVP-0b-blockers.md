# MVP-0b — Points restants : état vérifié, blocages et décisions requises

> Document de clôture honnête, au **2026-11-08**, branche `feat/mvp0b-hardening`
> (HEAD de code `e354c22`, PR [#80](https://github.com/Rwanbt/Ravum/pull/80)).
> Ce n'est **ni un ADR, ni un certificat, ni une preuve** : chaque ligne renvoie à
> une preuve du dépôt ou à une mesure d'environnement reproductible. Les décisions
> marquées « décision requise » appartiennent au responsable et ne sont pas prises ici.

## 1. Vue d'ensemble

| # | Point restant | État | Preuve / constat | Ce qui manque | Décideur |
|---|---|---|---|---|---|
| 1 | **ASIO** (nécessaire pour la MOTU) | **BLOQUÉ** (licence SDK) | pilote ASIO MOTU présent (`MOTUCoreUACASIO.dll`) ; SDK Steinberg absent | accepter la licence Steinberg ASIO SDK puis implémenter/mesurer | responsable |
| 2 | **Prouveur externe Kani** | **BLOQUÉ** (plateforme) | `kani`/`cargo-kani` absents ; Kani ne supporte pas Windows comme hôte | runner Linux/WSL2 dédié + installation + harness borné | responsable (CI) |
| 3 | **S8 avec vrais agents IA + comptage de tokens** | **BLOQUÉ** (accès/budget) | aucune clé `ANTHROPIC/OPENAI/DEEPSEEK/GEMINI` dans l'environnement ; campagne S8 restée scriptée | clés + budget + capture d'usage fournisseur | responsable |
| 4 | **Hôte CLAP/VST3 réel** | **HORS PÉRIMÈTRE** de ce dépôt | `D:\App\Seno` est un projet distinct, non modifié ; aucune automatisation d'hôte validée | décision et travail dans le projet Seno | responsable |
| 5 | **XRUN officiels, fenêtres longues, multi-machines** | **PARTIEL** | proxy local de périodes manquées mesuré (15 runs, validation par injection) — `MVP-0b-audio-host.md` §10 | compteur officiel inexistant sous Windows ; fenêtres de minutes ; autres pilotes/machines | planification |
| 6 | **Chemin sélectif sur une PR réelle vers `dev`** | **BLOQUÉ** par l'état de `dev` | `origin/dev` = `origin/main` = `7841102` : squelette documentaire, **aucun crate**, workflow impact absent | fusion de MVP-0a (PR #78) puis PR vers `dev` | responsable |
| 7 | **Durcissement de `dev`** | **BLOQUÉ** (décision de fusion) | `dev` protégé (PR requis, pas de force push) mais ne contient que les docs + `docs-check.yml` | porter workflows produit/impact après fusion | responsable |
| 8 | **Soak multi-heures multi-clients** | **PARTIEL** | 3 clients × 120 s : 816 requêtes, 0 erreur, 0 timeout ; contrôle mono 290 requêtes | fenêtre ≥ 1 h sur machine dédiée ; plusieurs pilotes/machines | planification |
| 9 | **Compaction de l'état incrémental** | **FAIT** (défaut OFF) | −89,5 % de disque sur 20 éditions réelles, GC 20 ms ; `reuse_retained=false` | aucun pour le disque ; gain de latence non démontré, donc non activé par défaut | — |
| 10 | **Daemon multi-client** | **FAIT** (loopback) | transport loopback TCP, 3 clients concurrents, `peak_active_jobs=1` | réseau/TLS/auth, multi-hôte, multi-`work_dir` (non revendiqués) | — |
| 11 | **Réduction build+link à froid** | **FAIT** (leviers écartés) | 9 jeux d'options, 16 tours entrelacés, contrôle de bruit — `MVP-0b-cycle.md` §7 | `opt-level=0` = décision (gain build −35 % mais code généré différent, coût DSP non mesuré) | responsable |
| 12 | **GC automatique en CI** | **FAIT** | job CI dédié, 18 invariants, run vert [42523426221](https://github.com/Rwanbt/Ravum/actions/runs/42523426221) | — | — |
| 13 | **Licence, RFC espace partagé, ordre de fusion** | **RÉSERVÉ** | aucun ADR accepté, RFC #79 `PROPOSED`, PR #78 et #80 ouvertes | décision humaine | responsable |

## 2. Détail des blocages

### 2.1 ASIO — bloqué par le SDK Steinberg, pas par la technique

Le pilote ASIO de la MOTU est installé (`C:\Program Files (x86)\MOTU\CoreUAC\x64\MOTUCoreUACASIO.dll`)
et le répertoire Steinberg contient le pilote générique (`C:\Program Files\Steinberg\Asio`),
mais **aucun en-tête du SDK ASIO** n'est présent sur la machine. Le SDK ASIO de Steinberg
est propriétaire : l'implémenter suppose d'accepter sa licence (et de décider du mode de
livraison des en-têtes). Ce n'est pas une tâche technique bloquée, c'est une **décision de
licence**. Conséquence mesurée : l'endpoint WDM de la MOTU refuse **les 12 combinaisons**
de formats exclusifs WASAPI (`AUDCLNT_E_UNSUPPORTED_FORMAT` = `0x88890008`) ; la latence
faible sur cette interface exige donc ASIO, pas WASAPI exclusif.

### 2.2 Kani — bloqué par la plateforme et l'installation

`Get-Command kani, cargo-kani` → absent. Kani supporte Linux et macOS comme hôtes
d'exécution, pas Windows : la machine de référence (Windows 11) ne peut pas l'exécuter
nativement. Il faudrait un runner Linux/WSL2 dédié, installer Kani/CBMC et écrire un
harness borné versionné. Aucun harness n'a été écrit pour ne pas laisser croire à une
tentative. Aucun statut `PROVED` n'est revendiqué sans cela.

### 2.3 S8 avec de vrais agents IA — bloqué par l'accès et le budget

Les variables `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY` et
`GEMINI_API_KEY` sont vides dans l'environnement de travail (vérifié le 2026-11-08). La
campagne S8 publiée reste donc **scriptée** (agents simulés) : elle établit des coûts et
des attentes perdus, pas une supériorité du workspace partagé, et **ne compte pas de
tokens**. Verdict inchangé : A worktrees 5 660,6 ms / B partagé 6 677,6 ms / C séquentiel
4 785,6 ms, attentes 2 053,5 / 7 750,8 / 0,14 ms, changements perdus 0 / 0 / 1, workspace
partagé non supérieur.

### 2.4 Hôte CLAP/VST3 — hors périmètre du dépôt Ravum

`D:\App\Seno` existe (projets de plugins) mais n'a pas été modifié et ne fait pas partie
de ce dépôt. La qualification audio Ravum mesure la sortie du système via une sonde
indépendante de l'hôte (waveOut, WASAPI partagé/exclusif, MMCSS) ; elle ne revendique
aucun hébergement de plugin.

### 2.5 XRUN — proxy local mesuré, pas de compteur officiel

`ravum-audio-probe` détecte les **périodes manquées** via `IAudioClock` (WASAPI) et
`waveOutGetPosition` (waveOut), avec régression, ligne de base et validation par
injection (`--inject-underrun-ms`) : 120/120/150 ms injectés → 12/11/14 périodes
détectées ; sans injection, 0 à 2 selon la charge ambiante. Granularité d'une période
(10 ms WASAPI, 10,67 ms waveOut 512), **borne inférieure** : `0` ne prouve pas l'absence
de glitchs sous-période ; aucun pilote Windows n'expose de compteur XRUN officiel.
Fenêtres mesurées : 3 s, une machine ; pas de fenêtre de plusieurs minutes ni de
multi-pilotes/multi-machines.

### 2.6 PR sélective vers `dev` — bloquée par l'état de `dev`

`origin/dev` et `origin/main` pointent sur le même commit `7841102` : **squelette
documentaire** (pas de `Cargo.toml`, pas de crates, pas de `.github/workflows/mvp-impact.yml`).
Une PR vers `dev` ne portant que l'outillage CI déclencherait une décision **FULL**
(`full:ci`, légitime) puis exécuterait `cargo test --workspace` **sans workspace** :
échec pour une mauvaise raison, ce qui ne serait pas une validation. Le chemin sélectif
est donc validé sur ce qui est réellement exécutable : les **pushs `feat/**` comparés à
`origin/dev`** (ex. run impact vert [42523426172](https://github.com/Rwanbt/Ravum/actions/runs/42523426172))
et la **décision outillée** sur un diff docs-only réel
(`evidence/mvp0b-ci-docs-only-impact.json` : `selective`, `full:false`,
`docs_required:true`, une seule commande `validate_repo_docs.py` ; selftest 48 cas /
388 assertions PASS). L'ouverture d'une PR réelle vers `dev` suppose la fusion de
MVP-0a (décision humaine) ; elle n'a pas été simulée.

### 2.7 Durcissement de `dev` — décision de fusion requise

`main` et `dev` portent déjà les protections de branche (PR requise, pas de force push,
pas de suppression) posées au lot L5. En revanche `dev` ne contient que la documentation
et un workflow `docs-check` : les workflows produit et impact, les crates et les preuves
n'y existent pas. Y porter quoi que ce soit suppose la fusion de #78/#80 (ou d'une partie)
— décision humaine, pas une action d'agent.

### 2.8 Soak multi-heures — non fait, et non extrapolable

Campagne réelle du sixième lot : **3 clients concurrents × 120 s**, 816 requêtes
(272/271/273), **0 erreur, 0 timeout, 0 blocage**, p50 par client 155/155/157 ms,
p95 282/290/289 ms, p99 394/416/372 ms (global p50 155 / p95 289 / p99 386 / max 490 ms),
RSS daemon 5,57 → 7,59 Mo (+2,02 Mo), `valid=true`. Contrôle mono-client : 290 requêtes,
p50 143 / p95 183 / p99 206 ms, RSS +1,32 Mo. Le `peak_active_jobs=1` montre la file du
gate (pas de parallélisme de compilation revendiqué). **Ce n'est pas un soak de plusieurs
heures**, ni une preuve anti-fuite, ni une mesure multi-machine.

### 2.9 Compaction — faite, volontairement désactivée par défaut

Sur 20 éditions réelles : work_dir **1 821 741 B → 191 055 B** (−89,5 % ; le GC revendique
818 507 B, le reste étant l'élagage propre de rustc), GC **20 ms**, 0 erreur, cache intact
(hit mémoire 0 ms), édition suivante valide. En revanche `reuse_retained=false` :
post-compaction 140 ms contre 119 ms pour une référence à froid — **aucun gain de
réutilisation démontré**. La compaction reste donc une politique explicite
(`compact_incremental=false` par défaut) de **bornage disque**, pas une optimisation de
latence. Le format des sessions rustc n'est jamais réécrit : seuls des dossiers `s-*`
entiers et des `*.tmp` orphelins sont supprimés.

### 2.10 Build+link — quatre leviers mesurés et écartés

`MVP-0b-cycle.md` §7 : neuf jeux d'options, 16 tours entrelacés, contrôle `baseline-b`
aux options identiques (bruit +0,4 % à +2,7 %), deux tailles de source. Aucune option
conservant la sémantique ne dépasse le plancher de bruit de façon robuste (meilleures :
−5 à −7 %, 10–13/16). `-C opt-level=0` gagne **−35,5 %** sur la grande source mais change
le code généré : c'est une **décision** (validation DSP requise), pas une optimisation
appliquée. Options produit inchangées.

## 3. Ce qui n'est pas revendiqué

- Aucun compilateur Ravum fonctionnel, aucune sémantique complète, aucun ADR accepté.
- Aucune preuve générale (`PROVED`), aucun prouveur externe exécuté.
- Aucun temps réel dur, aucune borne pire-cas, aucun XRUN officiel.
- Aucune supériorité du workspace partagé, aucun comptage de tokens réel.
- Aucune fusion, aucun déploiement, aucune licence choisie.

## 4. Renvois

- [MVP-0b-incremental.md](MVP-0b-incremental.md) §13 (compaction) et §14 (multi-client)
- [MVP-0b-audio-host.md](MVP-0b-audio-host.md) §10 (XRUN / périodes manquées)
- [MVP-0b-cycle.md](MVP-0b-cycle.md) §7 (leviers build+link)
- [MVP-0b-ci.md](MVP-0b-ci.md) §9 (GC automatique en CI) et §10 (décision docs-only)
- [MVP-0b-REPORT.md](MVP-0b-REPORT.md) §26 (sixième lot consolidé)

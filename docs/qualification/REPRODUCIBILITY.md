# Reproductibilité bit-à-bit du binaire `ravum-lang`

> **Statut : `PASS` sur un périmètre explicite**, mesuré le 2026-11-09 sur
> Windows 11 x86-64 avec `rustc 1.95.0`. Ce document ne revendique **pas** la
> reproductibilité de toute la chaîne (paquets, sorties `--emit-rust`, autres
> binaires du workspace, Linux, macOS).

| Élément | Valeur |
|---|---|
| Hôte | Windows 11 x86-64 |
| Toolchain | `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0` |
| Profil | `release` |
| Cible vérifiée | `ravum-lang-cli` → `ravum-lang.exe` |
| Commande | `pwsh -File scripts/repro-check.ps1` |
| Résultat | **deux builds identiques**, `sha256 = AA6C1AF14E4056E5902C698059D388929EBF45070035B6509D5D5A2361870B0F` (arbre du 2026-11-10) |

## 1. Ce que le test fait exactement

Le script copie l'arbre source (sans `target/`, `.git`, ni dépendances Node) dans
un répertoire temporaire, puis construit **deux fois** :

1. depuis la racine d'origine, vers `target/repro-<id>-a` ;
2. depuis la **copie**, à un chemin absolu différent, vers
   `target/repro-<id>-b`.

Les deux constructions utilisent :

```
CARGO_INCREMENTAL=0
SOURCE_DATE_EPOCH=0
RUSTFLAGS="--remap-path-prefix=<racine A>=/rv --remap-path-prefix=<racine B>=/rv -C link-arg=/Brepro"
```

Puis les deux `sha256` sont comparés. Le critère est **l'égalité stricte** des
deux binaires ; un seul octet différent fait échouer le script (code de sortie
`1`).

Le test change **deux variables à la fois** — chemin source et répertoire cible —
parce que la reproductibilité « même chemin, même `target/` » ne prouve rien :
elle est vraie par construction dès que la compilation est incrémentale.

## 2. Résultats négatifs (ce qui ne suffit pas)

Mesuré sur le même arbre et la même toolchain :

| Configuration | Hashes obtenus | Verdict |
|---|---|---|
| Aucun drapeau, trois builds | `461FBB9D…`, `4864F82D…`, `0613CE34…` | **trois binaires différents** |
| `SOURCE_DATE_EPOCH=0` seul, deux builds dans deux `target/` | `70FF8E28…`, `2723431B…` | **différents** |

Analyse octet par octet des deux binaires produits avec `SOURCE_DATE_EPOCH=0` :
**20 octets diffèrent**, tous situés dans les en-têtes du fichier PE — l'horodatage
COFF (`TimeDateStamp`) et les horodatages du répertoire de débogage. Le code, les
données et les tables de relocation sont identiques.

C'est le comportement de l'éditeur de liens MSVC : il écrit l'heure murale dans
l'en-tête. `SOURCE_DATE_EPOCH` n'est pas honoré par `link.exe`.

## 3. Résultat positif

Avec `/Brepro` (qui remplace l'horodatage par un hachage du contenu) **et** la
remise à zéro des chemins absolus :

| Configuration | Hashes | Verdict |
|---|---|---|
| Deux `target/` différents, même racine, `SOURCE_DATE_EPOCH=0` + `/Brepro` | `3B606F6B…` deux fois | **identiques** |
| Racine A + `target/` A **vs** racine B (copie) + `target/` B, avec `--remap-path-prefix` | `3B606F6B…` deux fois | **identiques** |

Le second point est le seul qui compte : il montre que le binaire ne dépend ni du
chemin absolu de l'arbre, ni du répertoire de build, ni de l'heure.

## 4. Repro

```powershell
pwsh -File scripts/repro-check.ps1
# repro-check: A = 3B606F6B…
# repro-check: B = 75786AEF…
# repro-check: PASS — the two builds are bit-for-bit identical
```

`-Keep` conserve la copie et les deux `target/` pour inspection. Le script
supprime tout par défaut.

## 5. Limites explicites

- **La valeur du hash dépend de l'arbre** : elle change dès qu'une ligne de code change ; c'est le but, et ce n'est donc pas une constante à citer hors de son SHA.
- **Un seul binaire** : `ravum-lang.exe`. Les autres binaires du workspace
  (outillage, coordonnateur) ne sont pas couverts.
- **Une seule plateforme** (constat de la passe 4) : Windows x86-64. Depuis le
  2026-11-14, la branche Linux (`-Wl,--build-id=sha1`) est **réellement
  exécutée** par `.github/workflows/reproducibility.yml` sur deux runners
  (section en fin de document) ; les sorties Windows et Linux restent des
  artefacts différents, non comparés entre eux.
- **Pas de reproductibilité inter-machines** : deux machines avec des versions de
  `rustc`, de `link.exe` ou de bibliothèques système différentes produiront
  vraisemblablement des binaires différents. C'est une propriété du toolchain,
  pas du projet. **Nuance du 2026-11-14** : deux runners de **même image** et de
  **même toolchain épinglée** produisent le même digest (mesure en fin de
  document) ; c'est ce périmètre, et lui seul, qui est désormais revendiqué.
- **Les sorties `ravum-lang build --emit-rust`** ne sont pas couvertes par ce
  test ; elles sont produites par le même émetteur déterministe, mais aucune
  mesure ne le prouve ici.
- **La signature des artefacts n'existe pas** : la reproductibilité permet de
  vérifier qu'un binaire correspond à une source, elle ne prouve pas **qui** l'a
  construit. C'est une exigence distincte du gate T11, toujours ouverte.

## Mesure complémentaire — passe 4 (2026-11-10, SHA `67a2a2e`)

Rejeu après les corrections de la passe 4 (motifs flottants/négatifs, budget de
diagnostics, pile du CLI) :

```
pwsh -File scripts/repro-check.ps1
repro-check: A = 67F2934B169BEF9B47A1D947CAAAA4E4CF0239E3B63CC3D07EFABC8AAB2FA7D3
repro-check: B = 67F2934B169BEF9B47A1D947CAAAA4E4CF0239E3B63CC3D07EFABC8AAB2FA7D3
repro-check: PASS — the two builds are bit-for-bit identical
```

Contexte : Windows 11 x86-64, `rustc 1.95.0`, deux racines et deux `target/`
distincts. Les limites listées ci-dessus sont inchangées : ceci prouve la
**reproductibilité locale** du binaire courant, pas la reproductibilité
inter-machines ni l'identité de l'auteur du binaire.

## Mesure complémentaire — passe 8 (2026-11-11, SHA `7696e2a`) et passage en CI

Rejeu après la fermeture des six dernières formes `check`-pass / `build`-fail :

```
pwsh -File scripts/repro-check.ps1
repro-check: A = 847DF0D11C1E2D00ABD049E8035E371D531D86DF90C6EA30184880759335CB96
repro-check: B = 847DF0D11C1E2D00ABD049E8035E371D531D86DF90C6EA30184880759335CB96
repro-check: PASS — the two builds are bit-for-bit identical
```

Deux changements par rapport à la passe 4 :

1. **La preuve est devenue continue.** `scripts/repro-check.ps1` est exécuté par
   le job `release-evidence` de `.github/workflows/qualification-evidence.yml`
   sur `windows-latest` à chaque push et à chaque PR. Run `43064500916`
   (push, `abe7d88`) et `43064513146` (PR) : `success`, deux builds identiques
   `C281A0357024910BD4553A68472BEA87AC11149A11F094016965BA55588C2F70`. Une
   preuve qui n'existe que dans un log local ne peut pas vieillir honnêtement.
2. **Le digest est propre à l'environnement, et c'est mesuré.** Le même arbre
   donne `847DF0D1…` sur la machine de référence et `C281A035…` sur
   `windows-latest`. La revendication reste donc strictement « deux builds, même
   toolchain, même linker » — la reproductibilité **inter-machines** n'est
   toujours pas revendiquée, et cet écart en est la démonstration directe plutôt
   qu'une omission.

Le pas d'étape `repro` refuse par ailleurs un code de sortie 0 qui ne
contiendrait pas deux digests de 64 caractères identiques : un script qui ne
mesure rien sort aussi 0, et ce cas ne doit pas passer pour un succès.

## Mesure complémentaire — rounds 13/14 (2026-11-14, SHA `662f63d`)

Rejeu après les phases 4-5 (`OwnFacts`, `traps.rs`, garde de places), sur la
machine de référence (Windows 11 x64, `rustc 1.95.0`), avec deux racines et deux
`target/` distincts :

```text
pwsh -File scripts/repro-check.ps1
repro-check: A = 988521151EFE827EA62448343C08128370CA3801A3EAEF312C04B298C43FDE8A
repro-check: B = 988521151EFE827EA62448343C08128370CA3801A3EAEF312C04B298C43FDE8A
repro-check: PASS — the two builds are bit-for-bit identical
```

Lecture :

1. Le digest a changé par rapport aux passes 4/8 (`847DF0D1…`) : le **code** a
   changé. Ce qui est prouvé est l'**égalité entre deux racines**, pas la
   stabilité d'un hash entre versions. Un hash qui ne bougerait jamais malgré un
   changement de code serait au contraire suspect.
2. La mesure reste **mono-machine** ; la CI `qualification-evidence.yml` continue
   d'exécuter son propre `repro-check` sur `windows-latest` avec son propre
   digest (`C281A035…` mesuré en passe 8), et la reproductibilité
   **inter-machines** n'est pas revendiquée.
3. Le même rejeu a rafraîchi le SBOM (`46b65d9f…`) et l'installateur Windows
   (`bf55aab9…`, deux désinstallations idempotentes) — voir le rapport final §3
   et §4.4.

Limites inchangées : une machine, une toolchain, un linker ; `-C
link-arg=/Brepro` et `SOURCE_DATE_EPOCH=0` sont des conditions du résultat, pas
des options décoratives.

## Mesure complémentaire — deux runners distincts (2026-11-14, PR #111)

**Ce qui manquait** : toutes les mesures ci-dessus comparent un digest avec
lui-même sur **une** machine (deux racines, deux `target/`) ou dans **un** job
CI. R7 demande le même digest sur **deux runners distincts**. La CI de
`qualification-evidence.yml` tourne sur un seul runner Windows ; elle ne pouvait
donc pas répondre.

**Dispositif.** `.github/workflows/reproducibility.yml` lance deux jobs
`ubuntu-latest` en matrice (`a`, `b`), sans cache `target/`, chacun exécutant
`scripts/repro-build.sh` (mêmes drapeaux : `CARGO_INCREMENTAL=0`,
`SOURCE_DATE_EPOCH=0`, `--remap-path-prefix` de la racine et du cache cargo,
`-C link-arg=-Wl,--build-id=sha1`). Un troisième job télécharge les deux JSON et
échoue si le `sha256` ou la taille du binaire diffèrent.

**Run réel** : [43622599083](https://github.com/Rwanbt/Ravum/actions/runs/43622599083),
PR #111 (commit de fusion `55f11a5e`), image `ubuntu-26.04`
(`20261111.196.1`), `rustc 1.95.0 (59807616e 2026-04-14)` :

| Runner | `runner_id` | Digest | Taille | Commit | `git_dirty` |
|---|---|---|---|---|---|
| a | 1000097957 | `9b7f16cc64fdb15ea286eeb48f4351332b513293c1a3d42467ba6b9e07166cdf` | 3 887 400 | `55f11a5e…` | false |
| b | 1000097958 | **identique** | 3 887 400 | `55f11a5e…` | false |

Les deux jobs ont démarré à `17:42:55Z` et ont produit le même binaire ; le job
de comparaison est **PASS**. Les deux `runner_id` sont distincts, ce qui exclut
la réutilisation d'un même runner et un cache `target/` partagé.

Lecture :

1. **Ce qui est désormais prouvé** : pour un même OS, une même image et une
   toolchain épinglée, deux machines distinctes produisent des octets
   **identiques**. C'est la lettre de R7 pour « deux runners distincts ».
2. **Ce qui n'est pas prouvé** : reproductibilité **entre OS** (un binaire
   Windows et un binaire Linux diffèrent par construction ; les digests
   `9FC5E3F2…`/`0C7B63C6…`/`847DF0D1…`/`C281A035…` selon la plateforme et le SHA
   le montrent), entre **toolchains** différentes, ni **artefact signé**. Les
   `hostname` des deux JSON sont identiques alors que les `runner_id` diffèrent :
   la comparaison porte sur le digest, pas sur le nom d'hôte.
3. **Statut R7** : `fait sur le périmètre` (même image + toolchain épinglée,
   deux runners) ; la limite inter-OS reste écrite et non revendiquée.

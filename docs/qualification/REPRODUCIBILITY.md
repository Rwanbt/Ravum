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
| Résultat | **deux builds identiques**, `sha256 = 3B606F6BE6B76EF94D943A57169445CD1C666FFDB3B77E828E90C7801A579E9B` (arbre du 2026-11-10) |

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

- **La valeur du hash dépend de l'arbre** : elle change dès qu'une ligne de code change ; c'est le but, et ce n'est donc pas une constante à citer hors de son SHA.\n- **Un seul binaire** : `ravum-lang.exe`. Les autres binaires du workspace
  (outillage, coordonnateur) ne sont pas couverts.
- **Une seule plateforme** : Windows x86-64. Sur Linux, le drapeau `/Brepro`
  n'existe pas ; le script substitue `-Wl,--build-id=sha1`, mais **cette branche
  n'a pas été exécutée** et n'est donc pas une preuve.
- **Pas de reproductibilité inter-machines** : deux machines avec des versions de
  `rustc`, de `link.exe` ou de bibliothèques système différentes produiront
  vraisemblablement des binaires différents. C'est une propriété du toolchain,
  pas du projet.
- **Les sorties `ravum-lang build --emit-rust`** ne sont pas couvertes par ce
  test ; elles sont produites par le même émetteur déterministe, mais aucune
  mesure ne le prouve ici.
- **La signature des artefacts n'existe pas** : la reproductibilité permet de
  vérifier qu'un binaire correspond à une source, elle ne prouve pas **qui** l'a
  construit. C'est une exigence distincte du gate T11, toujours ouverte.

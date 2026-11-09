# Signature des artefacts de release (mécanisme qualifié, clé absente)

> **Statut : `PARTIAL` assumé.** Le **mécanisme** de signature et de vérification
> est implémenté et qualifié de bout en bout. La **clé de projet**, sa custody et
> la liste des clés de confiance (`allowed_signers`) n'existent pas : elles ne
> peuvent pas être décidées par un agent. Ce document ne revendique donc **aucune
> release signée**.

| Élément | Valeur |
|---|---|
| Mécanisme | signatures détachées OpenSSH (`sshsig`) : `ssh-keygen -Y sign` / `-Y verify` |
| Algorithme | `ssh-ed25519` |
| Namespace de signature | `ravum-release` (une signature produite pour un autre usage ne peut pas être rejouée) |
| Outil | `OpenSSH_for_Windows_9.5p2` (présent sur l'hôte) |
| Script | [`scripts/attest-release.ps1`](../../scripts/attest-release.ps1) |
| Artefacts visés | binaire `ravum-lang`, SBOM SPDX, manifeste de provenance, checksums |

## 1. Ce qui a été exécuté

Mesure du 2026-11-09, Windows 11 x86-64, clé **de démonstration** générée pour
l'occasion (jamais versionnée) :

```powershell
# 1. signer
pwsh -File scripts/attest-release.ps1 -Mode sign -Key <clé> -Files target\release\ravum-lang.exe
#    signed   : ...\ravum-lang.exe -> ...\ravum-lang.exe.sig

# 2. vérifier l'artefact intact
pwsh -File scripts/attest-release.ps1 -Mode verify -Signers allowed_signers -Files target\release\ravum-lang.exe
#    verified : ...\ravum-lang.exe
#    attest-release: PASS — every artifact matches its signature

# 3. vérifier un artefact altéré (un octet ajouté)
pwsh -File scripts/attest-release.ps1 -Mode verify -Signers allowed_signers -Files <copie-altérée>
#    REJECTED : ...
#    Signature verification failed: incorrect signature
#    attest-release: FAIL — 1 artifact(s) rejected   (code de sortie 1)
```

Le rejet d'un artefact modifié est la partie qui compte : une signature qui
« passe » toujours ne prouverait rien.

## 2. Format `allowed_signers` — piège vérifié

Le format est `principal [options] type-clé clé`, **le principal d'abord** :

```
ravum-release ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA…
```

Écrire `ssh-ed25519 AAAA… ravum-release` (type en premier) produit un échec
silencieux : `ssh-keygen -Y verify` répond seulement
`Could not verify signature.` sans autre détail. C'est noté ici parce que
l'erreur est indiscernable d'une signature invalide.

## 3. Ce qu'il reste — et pourquoi ce n'est pas à un agent de le faire

Une signature n'a de valeur que si la clé publique est **ancrée** quelque part
que l'attaquant ne contrôle pas. Ici :

- aucune clé de projet n'existe ;
- aucun `allowed_signers` n'est publié ;
- aucune custody (HSM, secret CI, coffre hors ligne) n'est décidée.

Générer une clé et la laisser dans le dépôt produirait une signature
**décorative** : elle attesterait qu'un agent a signé avec une clé qu'il a
lui-même créée. C'est précisément ce que le mandat interdit (« ne jamais inventer
une signature ou une provenance »).

**Décision humaine requise (D9)** : choisir le mode de custody et publier la clé
publique. Le mécanisme est prêt ; il ne manque que l'ancre de confiance.

## 4. Limites

- Le script ne couvre que les artefacts qu'on lui passe ; il ne construit pas la
  liste (SBOM + binaire + checksums + provenance).
- Aucune intégration CI : signer en CI suppose un secret de dépôt, donc D9.
- La vérification porte sur le fichier tel quel ; elle ne prouve ni la
  reproductibilité (voir
  [`REPRODUCIBILITY.md`](REPRODUCIBILITY.md)) ni l'absence de compromission de la
  machine de build.
- `ssh-keygen -Y verify` lit le message sur l'entrée standard : sous PowerShell
  la redirection est déléguée à `cmd.exe`. Un pipe PowerShell corromprait un
  binaire.

# Custody de la clé de signature de release (décision D9)

> **Statut : `PROPOSED` — procédure non exécutée.** Aucune clé de projet n'a été
> générée, aucun `allowed_signers` n'est publié, aucune release n'est signée.
> Ce document prépare une décision humaine (D9) ; il n'autorise rien et n'ajoute
> aucun secret au dépôt.
>
> Ce qui **existe** : le mécanisme de signature/vérification OpenSSH `sshsig`
> (namespace `ravum-release`), qualifié de bout en bout — il accepte un artefact
> intact et **rejette** un artefact altéré. Voir
> [`../qualification/SIGNATURES.md`](../qualification/SIGNATURES.md).

## 1. Ce que D9 doit trancher

Trois choses distinctes, souvent confondues :

1. **Où vit la clé privée** (custody) et qui peut s'en servir.
2. **Qui est cru** : la liste `allowed_signers` publiée, et par quel canal.
3. **Que faire quand la clé est compromise** (rotation, révocation, ré-émission).

Tant que ces trois réponses ne sont pas écrites, signer serait une décoration :
une signature produite par une clé de démonstration ne distingue pas le projet
d'un attaquant qui aurait la même clé.

## 2. Options de custody

| Option | Description | Avantages | Limites |
|---|---|---|---|
| **A. Clé hors ligne sur le poste du mainteneur** | `ssh-ed25519` générée sur une machine hors réseau, jamais copiée | simple, sans dépendance, auditable localement | la sécurité vaut celle du poste ; pas de séparation de rôle ; perte = clé perdue |
| **B. Clé hors ligne sur machine dédiée (air-gap)** | poste dédié non connecté, clé utilisée uniquement pour signer des artefacts apportés par clé USB | isole la clé du poste de développement | procédure manuelle, risque d'erreur humaine ; nécessite un second poste |
| **C. Matériel (YubiKey / token FIDO)** | clé privée non exportable dans un token | vol de fichier insuffisant pour signer | OpenSSH `ssh-keygen -Y sign` avec token : support à qualifier **avant** de le retenir |
| **D. KMS / HSM cloud** | signature déléguée à un service | rotation et journal d'audit natifs | dépendance externe, coût, fuite de la surface de confiance ; contraire à l'esprit « pas de réseau » des preuves actuelles |

**Recommandation (non contraignante)** : **B**, avec une clé de secours (A)
conservée hors ligne pour la récupération, et **C** seulement après qualification
du support token par `ssh-keygen -Y sign` sur l'hôte cible.

## 3. Procédure minimale si l'option B est retenue

1. **Générer** sur la machine hors ligne uniquement :
   `ssh-keygen -t ed25519 -f ravum-release -C "ravum-release <date>"`
   (pas de phrase vide ; la phrase secrète reste sur la machine hors ligne).
2. **Publier** la clé **publique** dans le dépôt :
   `docs/release/allowed_signers`, ligne `ravum-release ssh-ed25519 AAAA…`.
   Une clé publique n'est pas un secret ; la ligne est relue en revue de PR.
3. **Signer** les artefacts déjà produits par le pipeline :
   `pwsh -File scripts/attest-release.ps1 -Mode sign -Key <clé> -Files <binaire>,<SBOM>,<manifeste>,<checksums>`.
4. **Vérifier** avant publication :
   `pwsh -File scripts/attest-release.ps1 -Mode verify -Signers docs/release/allowed_signers -Files <les mêmes>`.
   Un artefact altéré doit être **rejeté** (test négatif déjà qualifié).
5. **Publier** signature, artefact, checksums, SBOM et `allowed_signers` **ensemble**.
   Une signature publiée sans la liste de confiance ne vérifie rien.

## 4. Rotation, révocation, compromission

- **Rotation planifiée** (ex. tous les 12 mois) : nouvelle clé, nouvel identifiant
  de principal (`ravum-release-2027`), ancienne ligne conservée le temps de
  vérifier les artefacts déjà publiés, puis retirée avec la date de retrait.
- **Compromission suspectée** : retirer la ligne `allowed_signers`, publier un
  avis daté, ré-émettre les artefacts avec la clé de secours, et **ne pas**
  réécrire l'historique Git pour « effacer » la trace.
- **Perte de clé sans compromission** : ré-émettre avec la clé de secours ; la
  clé perdue est retirée, les artefacts non re-signés restent vérifiables par
  leur checksum publié mais **non attribuables** au projet.

## 5. Critères d'acceptation de la décision

D9 est résolue quand les cinq points suivants sont vrais :

1. une clé existe hors ligne, avec un propriétaire nommé et un lieu de stockage décrit ;
2. `docs/release/allowed_signers` est publié et relu en revue ;
3. un artefact de test est signé puis vérifié, et un artefact altéré est **rejeté** (rejeu documenté) ;
4. la procédure de rotation/révocation est datée et rattachée à un responsable ;
5. la gate T11 cite la mesure (artefact + signature + vérification) avec le SHA et l'environnement.

## 6. Ce que ce document ne fait pas

- Il ne génère aucune clé et ne stocke aucun secret.
- Il ne rend pas T11 `PASS` : T11 reste `PARTIAL` tant que D9 n'est pas résolue.
- Il ne remplace pas [`PROVENANCE.md`](PROVENANCE.md), qui décrit ce que
  l'installateur ne peut **pas** vérifier aujourd'hui.

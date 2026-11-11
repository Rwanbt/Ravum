# Feuille de réponse — décisions humaines ouvertes (2026-11-11, passe 9)

> **But de ce document** : te permettre de trancher chaque point ouvert en le
> lisant une fois, sans rouvrir le code. Pour chaque décision : ce qui est en jeu,
> ce qui existe déjà, les options avec leur conséquence réelle, la recommandation
> de l'agent et ce que coûte l'absence de décision. **Aucune n'est prise ici** :
> les agents n'ont ni le droit ni les moyens de les ratifier.
>
> **Comment répondre** : une ligne par identifiant dans le chat, par exemple
> `D2: b — ratifier le backend seul`, `D11: ok pour corriger les comptes`.
> Tout ce qui n'est pas listé ici est **déjà résolu** ou n'attend qu'un clic
> (section finale).
>
> **État mesuré au commit `de6f0de`** (Windows 11 x64, `rustc 1.95.0`) :
> conformance **456 cas : 455 PASS / 0 FAIL / 1 SKIP**, périmètre build
> **195/195**, `build_verified: true` ; workspace **113 suites, 1537 passés /
> 0 échec / 2 ignorés** ; gates **4 PASS / 7 PARTIAL / 2 BLOCKED** ; FeatureIds
> **188/188**, MUST **463/807** ; registre **108 codes / 126 émis**.

| ID | Sujet | Recommandation de l'agent | Effort de la décision |
|---|---|---|---|
| **D2** | Ratifier les ADR / choisir le backend de référence | Ratifier le **contrat de backend** (voie B pour 1.0, voie C hors 1.0) et le **socle normatif prouvé par la conformance** ; garder le reste `PROPOSED` | 1 édition de registre |
| **D3** | Fusion dans `main` et publication | **Pas de fusion `main`, pas de release publique** ; produire une **RC interne** liée à un SHA, avec checksums | 1 décision + 1 artefact |
| **D4** | Audit externe (T4/T10/T12) | **Oui, périmètre minimal** (borrow checker, codegen, prélude) ; en attendant, verdict plafonné `CORE_BETA` | mandat + budget |
| **D5** | Périmètre Tier 1 | **Clore** : Windows x86-64 local + Linux x86-64 sur `ubuntu-latest` ; le reste `NOT_RUN` | 0 travail |
| **D6** | Périmètre des fonctionnalités universelles | **Geler** le sous-ensemble documenté ; toute extension = 1.x | 1 texte |
| **D7** | Ratifier la frontière FFI/`unsafe` | **Expérimentale** pour 1.0 (déjà refusée par le profil `pure`) | 1 texte |
| **D9** | Custody de la clé de signature | **Clé hors ligne du mainteneur** + `allowed_signers` publié (procédure déjà écrite) | 1 choix + 1 clé |
| **D10** | Dénominateur de la traçabilité MUST | **Deux métriques officielles** + plan écrit pour les 344 MUST | 1 texte + plan |
| **D11** | Contradictions internes de spec | **Corriger les deux comptes** (non normatif), **ne pas renuméroter** les `rule_id` | ~20 min si autorisé |
| **D13** | Provenance de `&'static str` | **Garder le refus** et reformuler TYP-6.3/OWN-6.2 | 1 texte + 2-3 cas |

---

## D2 — Ratifier les ADR et choisir le backend de référence

**Ce qui est en jeu.** Les 43 fichiers de `docs/adr/` sont `PROPOSED` : aucune
règle du langage n'est « stable », tout reste « candidate ». En même temps, la
voie B (Ravum → `CheckedProgram` → Rust sûr généré → `rustc` → binaire natif) est
la seule réellement utilisée par `build`/`run`/`test`, et la voie C (backend natif
direct) n'existe pas. Tant que rien n'est ratifié, aucun document ne peut dire
« le langage Core 1.0 fait X » sans ajouter « sous réserve ».

**Ce qui existe déjà.** Le dossier ratifiable
[`docs/rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md`](../rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md)
décrit le contrat source → frontend → `CheckedProgram` → émission → binaire, avec
les métriques mesurées (check 225/375/1058 ms ; build 2027/2758/6109 ms ; `rustc`
77-87 % du build, sous charge déclarée). Les 188 FeatureIds sont tracés vers des
cas exécutés ; 456 cas de conformance passent, dont le build vérifié.

**Options.**
- **(a) Tout ratifier d'un bloc** : rapide, mais ratifie aussi des ADR
  d'architecture dont plusieurs ne sont prouvés par rien (T1 `PARTIAL`, voie C
  absente). Un `PROPOSED` ratifié sans preuve est exactement ce que le mandat
  interdit.
- **(b) Ratifier par étage** *(recommandé)* : (1) le **contrat de backend** —
  voie B pour Core 1.0, voie C explicitement hors 1.0 ; (2) le **socle normatif
  prouvé par la conformance** (les règles citées par les 188 FeatureIds et
  satisfaites par un cas d'acceptation ou de refus) ; (3) tout le reste reste
  `PROPOSED`. Tu obtiens une phrase défendable — « ceci est Core 1.0 DRAFT
  ratifié, voici les règles et leurs cas » — sans ratifier l'architecture.
- **(c) Ne rien ratifier** : statu quo. Coût réel : chaque rapport doit répéter
  « candidate », et T1/T12 restent ouverts pour une raison qui n'est pas
  technique.

**Recommandation : (b).** Le texte normatif existe déjà (spec + 456 cas) ; il
manque seulement la signature. C'est 1 édition de `docs/adr/README.md` + 1
décision écrite. Si tu veux aller plus vite : ratifie d'abord le backend seul
(une page), le reste plus tard.

**Sans décision** : T1 reste `PARTIAL`, la sémantique reste « candidate », et
aucune release ne peut être présentée comme autre chose qu'expérimentale.

---

## D3 — Fusion dans `main` et publication de release

**Ce qui est en jeu.** `feat/lang-core-1.0-rc` porte 9 passes de travail, la PR
[#101](https://github.com/Rwanbt/Ravum/pull/101) est en **draft** vers `dev`,
`main` n'a pas bougé. Faut-il fusionner et publier ?

**Ce qui existe déjà.** 8 workflows, CI verte sur les commits poussés (14/14 sur
`000708f`, run en cours sur `de6f0de`) ; installateur qualifié 16/16 ; SBOM
déterministe (64 paquets / 170 relations) ; provenance émise et vérifiée, avec
refus d'un binaire altéré ; reproductibilité bit-à-bit **locale** (deux racines,
deux `target/`) ; signature **non** opérationnelle (pas de clé, D9) ; audit
externe absent (D4).

**Options.**
- **(a) Fusionner dans `main` maintenant** : `main` est la référence
  documentaire, pas une preuve de maturité (`AGENTS.md`). Fusionner avant audit
  et avant custody de clé revient à publier un compilateur non audité comme
  référence du dépôt.
- **(b) RC interne attachée à un SHA** *(recommandé)* : un commit marqué, un
  dossier d'artefacts (binaire Windows, checksums `sha256`, SBOM, manifeste de
  provenance, rapport de conformance), **sans tag stable, sans release publiée**.
- **(c) Ne rien faire** : la branche continue de vivre ; `dev` reste
  l'intégration. Acceptable, mais le travail n'est pas « livré » au sens de la
  charte.

**Recommandation : (b).** Elle donne un objet vérifiable (le SHA + les
checksums) sans franchir aucune des deux lignes rouges (audit, signature). Coût :
1 h. À noter : la fusion `dev ← branche` est une décision distincte et de faible
risque, si tu veux d'abord intégrer sans publier.

**Sans décision** : aucun tag, aucune release, la branche reste la seule copie
du travail.

---

## D4 — Audit externe / revue indépendante (bloque T4, T10 et T12)

**Ce qui est en jeu.** Un agent ne peut pas certifier seul un compilateur inédit :
les tests couvrent un corpus fini (456 cas), pas l'absence de failles. T4
(`BLOCKED`), T10 (`PARTIAL`) et T12 (`BLOCKED`, Go/No-Go) attendent cet audit. Le
cadre de mission est déjà écrit :
[`docs/audit/AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md).

**Ce qui existe déjà.** 4 audits internes contradictoires (dont l'assurance
« aucun faux vert »), 8 formes de `check`-pass/build-fail trouvées et fermées,
une seconde métrique de traçabilité qui a **dégradé** T2 honnêtement, 3 défauts
d'honnêteté de nos propres tests trouvés par la CI.

**Options.**
- **(a) Audit complet** (tout le langage, specific + codegen + runtime) : le plus
  solide, le plus cher, le plus long.
- **(b) Audit minimal ciblé** *(recommandé)* : borrow checker + codegen + prélude
  runtime — les trois surfaces où une erreur devient une faille mémoire ou une
  miscompilation silencieuse. C'est le périmètre du brief existant.
- **(c) Pas d'audit** : T4/T12 restent `BLOCKED` indéfiniment et le verdict
  plafonne à `CORE_BETA` (c'est déjà la règle écrite).

**Recommandation : (b).** Et pendant qu'il n'a pas eu lieu : **ne rien
revendiquer de plus que ce que la CI mesure** (aucun « production-ready », aucune
« certification mémoire »). Coût : un mandat + un budget externe ; rien côté code.

**Sans décision** : T4, T10 et T12 restent bloqués, et la limite est *déjà*
écrite dans les rapports.

---

## D5 — Périmètre Tier 1 (déjà mesuré, à formaliser)

**Ce qui est en jeu.** « Tier 1 » n'est pas défini : est-ce Windows seul, Windows
+ Linux, davantage ?

**Ce qui existe déjà.** Windows x86-64 : tout est mesuré en local, et
`mvp.yml` exécute build + tests + démos sur `windows-latest`. Linux x86-64 :
`linux-qual.yml` (8 jobs) tourne sur `ubuntu-latest`. macOS, ARM, Android, iOS :
`NOT_RUN`. WASI : exécuté, avec Node, mais le corpus n'y est pas rejoué.

**Recommandation : clore D5 par écrit** — Tier 1 = **Windows x86-64 + Linux
x86-64 (`ubuntu-latest` uniquement)** ; tout le reste est explicitement `NOT_RUN`
et le dire suffit. Coût : 0 travail, 1 phrase dans `PLATFORMS-MATRIX.md`.

**Sans décision** : la matrice reste honnête mais la promesse reste floue, et
« Linux supporté » peut être lu comme « toutes les distributions ».

---

## D6 — Périmètre des fonctionnalités universelles

**Ce qui est en jeu.** « Langage universel » est l'objectif long terme ; Core 1.0
est un sous-ensemble. Sans périmètre écrit, la promesse dérive avec les
documents.

**Ce qui existe déjà.** Sont réellement supportés et testés : stdlib
core/io/fs/string/vec/math, FFI minimale (`extern "C"` + `unsafe`, signatures
primitives), 10 verticales `.rv` exécutées. Sont explicitement `UNSUPPORTED` :
threads, réseau, async, GPU/mobile, hard-RT, hot reload complet (préview
fail-closed seulement).

**Recommandation : geler par écrit la liste ci-dessus** comme périmètre Core 1.0,
et traiter toute extension comme 1.x. C'est déjà ce que la conformance et la CI
mesurent ; il manque la déclaration. Coût : 1 texte.

**Sans décision** : chaque lecture du README peut promettre plus que ce que la
CI prouve.

---

## D7 — Ratifier la frontière FFI / `unsafe`

**Ce qui est en jeu.** La FFI existe (inventaire réel dans
[`docs/rfc/RFC-D7-FFI-EFFECTS.md`](../rfc/RFC-D7-FFI-EFFECTS.md)) : `extern "C"` +
`unsafe { }`, signatures primitives uniquement. Il est **prouvé** que `rustc` ne
reçoit **aucun** `-L`/`-l`/`--extern`, et que `ffi_safe()` n'admet ni pointeur ni
buffer. Ce qui manque : la politique de sûreté mémoire des bibliothèques C
appelées, et une revue humaine de cette frontière.

**Options.**
- **(a) Stable** : il faudrait un audit de la frontière C (donc D4), une
  politique écrite des contrats ABI/lifetime/unwinding, et des tests négatifs.
- **(b) Expérimentale** *(recommandé)* : utilisable en interne, refusée par le
  profil `pure`, jamais présentée comme certifiée mémoire, jamais dans un profil
  `rt`. C'est l'état actuel du code — la décision ne fait que le déclarer.

**Recommandation : (b).** Le profil `pure` la refuse déjà ; la déclarer
expérimentale est cohérent avec ce que la consigne interdit de revendiquer.

**Sans décision** : FFI reste dans un flou qui permet à un lecteur de croire
qu'elle est qualifiée.

---

## D9 — Custody de la clé de signature de release

**Ce qui est en jeu.** Le mécanisme est implémenté et qualifié : signature
Ed25519 via `ssh-keygen -Y sign`, namespace `ravum-release`, et **rejet d'un
artefact altéré d'un octet**. Il manque l'**ancre de confiance** : où vit la clé
privée, qui peut l'utiliser, où est publié `allowed_signers`.

**Ce qui existe déjà.** La procédure proposée est écrite :
[`docs/release/KEY-CUSTODY.md`](../release/KEY-CUSTODY.md). Un agent n'a pas le
droit de générer de clé, et une clé générée par l'agent qui signe à côté de
l'artefact ne prouverait rien.

**Options.**
- **(a) Clé hors ligne du mainteneur** *(recommandé pour une RC interne)* :
  générée par toi sur ta machine, `allowed_signers` publié dans le dépôt, clé
  privée jamais sur un runner. Simple, vérifiable, suffisant pour une RC.
- **(b) Secret CI** : pratique, mais la clé devient accessible à tout ce qui peut
  modifier le workflow — surface plus large.
- **(c) HSM / service de signature** : le plus solide, le plus lourd, à réserver
  à une release publique.

**Recommandation : (a)** pour l'instant, avec `allowed_signers` commité. T11
passe alors de `PARTIAL` à prouvé sur la signature (le reste du gate suit son
cours).

**Sans décision** : aucune release ne peut être signée ni la provenance déclarée
vérifiable.

---

## D10 — Dénominateur de la traçabilité MUST

**Ce qui est en jeu.** Deux mesures coexistent : **188/188 FeatureIds** tracés et
satisfaits, et **807 énoncés MUST** dans la spécification dont **463 seulement
(57,4 %)** sont atteignables par un FeatureId — **344** ne le sont pas. Le gate
T2 dit « 188/188 » ; ce chiffre ne mesure donc pas la traçabilité de la
spécification, mais celle des fonctionnalités.

**Options.**
- **(a) Deux métriques officielles** *(recommandé)* : métrique 1 = FeatureIds
  (aujourd'hui 188/188, seuil inchangé), métrique 2 = couverture MUST
  (463/807), et **plan écrit** pour les 344 (lesquels sont hors périmètre et
  pourquoi, lesquels seront tracés à quelle échéance). T2 peut alors passer
  `PASS` sur la métrique 1 avec un écart publié et borné.
- **(b) Périmètre écrit excluant les 344** : plus rapide, mais il faut justifier
  chaque exclusion — c'est en pratique le même travail que (a), avec moins de
  crédibilité.
- **(c) Tracer les 344** : le plus honnête à terme, mais c'est un chantier de
  plusieurs jours (chaque énoncé doit être relié à un cas ou à une exemption
  machine-lisible).

**Recommandation : (a)**, avec (c) comme trajectoire. Coût immédiat : 1 texte de
politique + le tableau des 344 déjà produit par `tools/must_audit.py`.

**Sans décision** : T2 reste `PARTIAL`, et « 188/188 » doit toujours être lu
comme « 188 FeatureIds ».

---

## D11 — Contradictions internes de spécification

**Ce qui est en jeu.** Trois incohérences mesurées dans `spec/` :

| Point | Contradiction |
|---|---|
| `effects.md` EFF-1.1 | dit « les 9 groupes » ; son tableau et `facts.rs` en comptent **10** |
| `features.md` FEAT-12.5 | énumère **6** lignes `experimental` ; **8** lignes le sont (`F-STD-031`, `F-META-006`) |
| `grammar.md` | deux règles distinctes portent le même `rule_id` : **G-5.11** et **G-6.40** |

**Options.**
- **(a) Corriger les deux comptes** (9 → 10, 6 → 8) : non normatif, aligne le
  texte sur le tableau et sur le code. **Aucun risque contractuel.**
- **(b) Renuméroter les `rule_id` dupliqués** : change un identifiant
  contractuel (Q-DIAG-4) ; tout outil ou test qui cite `G-5.11`/`G-6.40` doit
  suivre.
- **(c) Garder les doublons et les documenter** : possible, mais un `rule_id`
  dupliqué rend toute citation ambiguë.

**Recommandation : (a) maintenant, et (c) pour les identifiants** — corriger les
comptes tout de suite (je peux le faire en ~20 min si tu autorises, c'est non
normatif), et pour les `rule_id` ajouter une note explicite « deux règles
distinctes, identifiant historique partagé » plutôt que renuméroter. Si tu
préfères renuméroter, dis-le : c'est fait en une passe avec mise à jour des
citations.

**Sans décision** : la spec reste auto-contradictoire sur trois points, ce qui
affaiblit tout audit (D4) et toute ratification (D2).

---

## D13 — Provenance de `&'static str` à travers les agrégats

**Ce qui est en jeu.** `let s = a[1];` sur `[&str; 2]` et
`fn f(a: [&str; 2]) -> &str { return a[1]; }` sont **refusés**
(`E-OWN-REF-ESCAPE`), alors que TYP-6.3 (MUST) dit qu'un `&str` est `Copy`,
stockable et retournable, et que OWN-6.2 n'exempte que le **littéral**. En
revanche `a[1] == "x"`, `enum Label { Text(&str) }` et la copie d'un littéral
local passent.

**Ce qui est mesuré.** **0 programme refusé** aujourd'hui dans `examples/` (9/9
exit 0) et 0 FAIL de conformance : le refus ne bloque rien d'existant, mais il
refuserait du code qu'un utilisateur écrirait naturellement.

**Options.**
- **(A) Accepter** : cohérent avec TYP-6.3, mais fait de « tout `&str` stocké est
  `'static` » une **hypothèse portante de la sûreté** — qu'une extension future
  (FFI, lifetimes) peut casser. Coût : traçage de la staticité par constructeur
  (index, champ, pattern, payload) + tests de l'invariant — plusieurs jours de
  travail sémantique.
- **(B) Garder le refus** *(recommandé)* : sûr, conservateur, conforme à OWN-6.2
  lu littéralement ; il faut alors **reformuler TYP-6.3** pour dire que la
  staticité ne se propage qu'aux littéraux et aux copies de locales, pas à
  travers les agrégats, avec une limitation documentée (`Q-OWN`). Coût : 1 texte
  + 2-3 cas négatifs qui verrouillent la formulation.

**Recommandation : (B)** pour Core 1.0, avec la reformulation. Si l'ergonomie
prime sur la rigueur du modèle, (A) est défendable — mais c'est un choix de
modèle, pas un correctif, et il doit être financé comme tel.

**Sans décision** : la contradiction TYP-6.3 / OWN-6.2 reste ouverte, et un
lecteur de la spec peut légitimement croire que le code est accepté.

---

## Déjà résolu (pour mémoire, rien à faire)

`D1` licence `MIT OR Apache-2.0` (23/23 membres) · `D8` codes de diagnostic
(registre canonique, `W-TYP-UNREACHABLE` retiré) · `D-PAT-1` motifs littéraux ·
`D-CMP-1` affectations composées · `D-UFCS-1` UFCS · `D-MOD-3.2` `use` relatif ·
`D-CONST-1` `const_expr` · `D-COERCE-1` coercition `String → &str` (retirée par
conformité à deux MUST) · `D12` moves de tableau (option (a), **toutes les formes
fermées**, `21cf7d4`).

## N'attend qu'un clic (pas une décision technique)

Les quatre cartes du board sont livrées en `in_review` ; seul toi peux les passer
en `done` :

| Carte | Checklist |
|---|---|
| P0 — gates honnêtes et bloquantes | 5/5 |
| P0 — chaîne check→build→activation | 7/7 |
| P1 — contrats et décisions | 4/5 — l'item [2] (« aucun comportement modifié ») est ouvert **volontairement** : la coercition `String → &str` a été retirée parce que TYP-8.5/TYP-13.1 la rendent explicite, donc l'ensemble accepté a changé. La cocher est un choix, pas une formalité |
| P2 — MVP universel | 4/4 |

## Modèle de réponse

```
D2  : b            # ratifier backend + socle normatif, garder le reste PROPOSED
D3  : b            # RC interne liée au SHA, pas de fusion main, pas de release
D4  : b            # audit minimal ciblé (borrow checker, codegen, prélude)
D5  : close        # Tier 1 = Windows x64 + Linux ubuntu-latest
D6  : gel          # périmètre gelé tel que mesuré
D7  : b            # FFI expérimentale
D9  : a            # clé hors ligne + allowed_signers publié
D10 : a            # deux métriques + plan pour les 344
D11 : a + c        # corriger les comptes, documenter les rule_id dupliqués
D13 : b            # garder le refus, reformuler TYP-6.3/OWN-6.2
Cartes P0/P0/P1/P2 : done (ou dis-moi ce qui bloque)
```

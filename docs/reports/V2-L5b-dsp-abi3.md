# V2-L5b — ABI DSP 3 à état opaque

| Champ | Valeur |
|---|---|
| Gate | T8 |
| Branche / SHA de départ / SHA final | `v2/L5b-dsp-abi3` / `60875cfb378437b62b1b8232c743ef143ffcedea` / commit qui ajoute ce rapport |
| SHA des sources validées | `f47ba14a629a7dcc0a7fcc1ddb79045c2d0853d7` |
| Décision | V2-R2, RFC-DSP-ABI-2 |
| Plateforme | Linux x86_64, Intel Core i7-13620H, Cranelift 0.116.1 |
| Toolchain | rustc `1.95.0 (59807616e 2026-04-14)`, Cargo 1.95.0 |

## Objectif et périmètre

Générer et charger des blocs DSP natifs dont le module déclare l’état, avec
les garanties P0-C, P0-D, P0-F, INV-05 et les limites DSP3-1 à DSP3-10.
L’ABI 2 historique reste disponible avec son hôte et ses anciens modules.

## Modification

- Le protocole ajoute le layout C opaque et ses bornes (64 Kio, alignement
  64). Le checker valide les champs et un `init` sans effet. La présence
  d’`init` sélectionne l’ABI 3. Le chemin historique sans `init` conserve
  l’ABI 2 et son état de deux `f32`.
- Le backend Rust émet le layout `repr(C)` des structs d’état atteignables,
  initialise le tampon de l’hôte et traite un nombre quelconque de trames.
- L’hôte contrôle l’ABI, le manifeste, le digest et le layout avant traitement.
  Le remplacement exige l’identité d’artefact, un instantané courant et une
  évidence qui l’autorise. Il conserve le même tampon à digest identique.
  Le harnais exécute le même test de fumée sur un état ABI 3 initialisé.
- La migration utilise une copie de l’ancien état et une nouvelle zone.
  Le bloc et son état courant restent actifs en cas de refus ou de trap.
  Les sites FFI sont listés dans l’inventaire et le dossier d’audit externe.
- `OpaqueEditor` et `OpaqueRunner` séparent publication et traitement.
  Le runner prend la copie dans un tampon préalloué à une frontière de
  bloc. L’éditeur migre cette copie et garde la charge de libération des
  tampons retirés. L’adoption échange le tampon au bloc suivant.

## Tests positifs / négatifs

| Vérification | Observation locale |
|---|---|
| DSP3-T1 | 10 signaux, 640 échantillons bit à bit entre cdylib, JIT et interpréteur, struct imbriquée et tableau. |
| DSP3-T2 | 10 000 blocs, 20 publications compatibles, même trajectoire d’état, zéro allocation et zéro libération Rust mesurées dans `process`. |
| DSP3-T3 | Quatre dispositions générées, schéma et layout identiques entre deux versions de chaque programme. |
| DSP3-T4 | Éditeur et audio sur deux threads. Le bloc courant continue après capture, la migration réussie est adoptée au bloc suivant, le trap et la demande périmée laissent le traitement en cours. Capture et adoption comptent zéro allocation et libération. Un schéma différent sans migration est refusé. |
| DSP3-T5 | Refus d’un champ `String`, d’un initialiseur avec division potentiellement piégeante, d’un layout de 65 540 octets, des hôtes croisés, des mises à jour sans évidence ou périmées, et d’un layout différent sous le même digest. |
| DSP3-T6 | Quatre tests CLI de l’ABI 2 historique réussis, fixtures conservées. |

## Commandes exécutées

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p ravum-lang-cli --test opaque_dsp_block --test dsp_block
cargo test -p ravum-patch-host --test opaque_block
python3 tools/unsafe_audit.py
cargo --config 'profile.test.package.sha2.opt-level=3' test \
  -p ravum-protocol -p ravum-patch-host -p ravum-dsp-harness \
  -p ravum-lang-codegen -p ravum-lang-sema -- --test-threads=1
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
cargo run -p ravum-conformance --release -- run
```

## Limites, écarts et décisions d’intégration

- Le contrat ratifié donne une migration C de retour `void` mais exige un
  refus en cas de trap. Une expérience locale entre un exécutable et son
  `cdylib` confirme que `C-unwind` et `catch_unwind` côté hôte peuvent
  avorter le processus (code 134 avec la toolchain ci-dessus). Le candidat
  intercepte donc la panique **dans la bibliothèque** et expose
  `ravum_state_migrate_status` sur le même thread éditeur. Le chargeur refuse
  une migration sans ce canal. Cet export est une extension d’ABI à examiner
  par l’intégrateur avant de figer le contrat public.
- Les échantillons traités entre la capture et l’adoption continuent de
  sortir via l’ancien bloc. Leurs changements d’état ne sont pas rejoués
  dans le nouvel état, qui vient de la capture, comme dans T16 §9. Le banc
  à deux threads ne qualifie ni un périphérique CPAL ni le temps réel dur.
- Les tests CLI et le dossier d’audit sont touchés pour vérifier la chaîne
  réelle et satisfaire DSP3-9. `ravum-protocol/src/patch.rs` porte les refus
  structurés de layout, de migration et de mise à jour en vol. Ni la matrice ni l’ABI du JIT ne sont
  modifiées.
- Les suites globales utilisent un checkout Linux natif temporaire avec le
  correctif de masquage du harnais `64efd691b2b18fd2e5cb5e86fed98a63ab0e8891`
  appliqué uniquement dans cette copie.

## État des preuves et proposition à l’intégrateur

`TESTED` pour les chemins locaux ci-dessus, avec le compilateur Rust,
la bibliothèque chargée et le module généré dans la base de confiance.
Workspace avec 2 335 réussites, 0 échec et 14 ignorés. Conformance avec
748 réussites, 0 échec et 2 `SKIP`. La dimension `--verify-build` est
`NOT_RUN`. Formatage, Clippy et contrôle d’inventaire ont réussi.
Le contrôle d’inventaire ne constitue pas l’audit externe. Proposer
l’intégration des résultats techniques sous réserve de la décision sur
le canal de statut et de l’audit indépendant des sites natifs.

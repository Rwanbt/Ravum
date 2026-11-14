# Campagne de fermeture des chantiers techniques — 2026-11-14 (soir)

> **Rapport de campagne, pas un certificat.** Branche `goal/consolidation-2`
> (PR #111), dépôt `Rwanbt/Ravum`, Windows 11 x64 + runners `ubuntu-latest`,
> `rustc 1.95.0`. Cette campagne visait les **chantiers techniques encore
> ouverts** listés dans `PRODUCTION-READINESS.md` §3 et dans la réponse du
> 2026-11-14. Elle ne promeut **aucun gate** à `PASS`, ne déclare **aucune**
> maturité « production ready », ne crée **aucun** tag, release ou signature :
> les décisions humaines restent dans
> [`HUMAN-DECISIONS-REQUIRED.md`](HUMAN-DECISIONS-REQUIRED.md) (21 fiches).
>
> Ce que chaque ligne ci-dessous vaut : la commande citée, sur l'arbre cité.
> Un `NOT_RUN` n'est jamais un `PASS`. Aucun `PROVED`.

## 1. Ce qui a été fermé

| Chantier | Avant | Après cette campagne | Statut honnête |
|---|---|---|---|
| **R7 — reproductibilité inter-machines** | digest comparé à lui-même sur une machine | **deux runners `ubuntu-latest` distincts** (`runner_id` 1000097957/58) produisent le même digest ; reconfirmé sur un second commit avec deux autres runners | **fait sur son périmètre** (même image + toolchain) ; ni inter-OS ni inter-toolchain |
| **R9 — non-régression suivie** | p50/p95 publiés, aucun contrôle répété | alarme CI `performance.yml` + `tools/timings_gate.py` (plafond `max(200 ms, ×3)`), premier run vert | `PARTIAL` : `check` seul, pas de budget, ni `build` ni `watch` |
| **R10 — politique de dépréciation** | absente | [`DEPRECATION-POLICY.md`](../release/DEPRECATION-POLICY.md) (`PROPOSED`, 626 lignes) : 7 interfaces, versionnement, cycle, registre | `PARTIAL` : politique écrite, **non ratifiée** ; aucun avertissement outillé, aucun CHANGELOG |
| **R11 — installateur Linux public** | installé par la CI, sans contrat public | [`INSTALL-LINUX.md`](../release/INSTALL-LINUX.md) + `scripts/install-contract.sh` ; exécuté par `linux-qual.yml` : **25 vérifications shell + 30 invariants du manifeste** | `PARTIAL` : reste artefact distribuable (SBOM+manifeste) et signature D9 |
| **R14 — guide et notes de version** | guide absent | [`GETTING-STARTED.md`](../guide/GETTING-STARTED.md) (767 lignes), [`RELEASE-NOTES-0.1.0-DRAFT.md`](../release/RELEASE-NOTES-0.1.0-DRAFT.md) ; audit de véracité partiel | `PARTIAL` : audit exhaustif non rejoué |
| **R5/D7 — frontière FFI** | `unsafe` non confiné mécaniquement, `&str` non verrouillé | `#![forbid(unsafe_code)]` sur syntax/sema/codegen/lsp, `#![deny]` + 2 `allow` inventoriés sur le CLI ; `tools/unsafe_audit.py` (7 tests) ; cas `types/ffi-str-rejected` ; `SAFETY.md` amendé | `PARTIAL` : D7 non ratifiée ; `i128`/`u128`, unwinding/`longjmp`/signal, Miri FFI restent ouverts |
| **R8 — mutation `emit.rs`** | jamais mesurée | première campagne ciblée : **36 mutants / 30 tués / 6 survivants triés** ; totale **219 / 180 / 30 / 9** | `PARTIAL` : portée `-F`, pas un score de crate ; 1 lacune de test fermée, 1 assumée |
| **R8 — objectif de couverture** | 89,79/89,23 ; objectif 100 % non atteint | **90,44/89,88** ; code produit **88,55 %** ; 25 tests loader (`loader.rs` **78,85 → 83,46 %** lignes, **82,19 → 86,70 %** régions) | `PARTIAL` : 10 fichiers sous l'objectif, décision D10 |
| **T2 — campagne nominale** | interrompue à 561 programmes | **3 × 2 000 = 6 000 programmes, 0 rejet, 0 divergence**, 2 217 s | `TESTED` localement ; test de puissance toujours `BLOCKED` par sa méthode |
| **LSP à l'échelle** | 337,6 ms debug, aucune mesure produit | profil **release** : p50 **116,7 ms**, p95 **163,5 ms** à 5 110 lignes | cible 50 ms **non tenue** ; décision humaine `D-2026-11-14-5` |
| **Outil de split produit/test** | bug : `loader.rs` classé 100 % test | frontière = `#[cfg(test)] mod tests` ; 5 tests ; chiffres corrigés (1 557/13 598 produit) | corrigé, aucune conséquence sur les seuils |
| **Chiffres faux** | « 43 ADR », T9 « 9 verticales », RFC obsolètes | **42** ADR, **10** verticales, RFC-NODE-ID/OWN-FACTS/D2 amendées, brief d'audit daté | corrigé, preuves à l'appui |

## 2. Preuves exécutées (valeurs réelles)

```text
cargo test --workspace --all-targets -- --test-threads=2
  102 groupes, 1 660 passés / 0 échec / 7 ignorés                       exit 0

cargo run -q -p ravum-conformance -- run --verify-build
  484 cas : 483 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN
  build: perimeter=207 covered=207 verified=124 implicit=83 failed=0
  build_verified: true                                                exit 0

cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen --json
  lignes 90,44 % (19 237/21 271) / régions 89,88 %
  seuils par fichier tenus, aucun abaissé                             exit 0
python tools/coverage_product_split.py --input target/cov-all.json
  produit 1 557 non couvertes / 13 598 (88,55 %) ; test 221
  loader.rs 83,46 % lignes / 86,70 % régions

mutation.yml (run 43622868110, artefact mutants-targeted)
  own 32/31/1/0 ; resolve 57/40/15/2 ; typeck 94/79/8/7 ; emit 36/30/6/0
  TOTAL 219 mutants / 180 tués / 30 survivants triés / 9 non viables

RAVUM_DIFF_PROGRAMS=2000 RAVUM_DIFF_RELEASE_EVERY=10
cargo test -p ravum-lang-codegen --test differential_oracle -- --ignored
  3 seeds × 2 000 = 6 000 générés, 6 000 acceptés, 0 rejet, 0 divergence
  release_every=10, statements=8, 2 217,43 s                            exit 0

cargo test --release -p ravum-lang-cli --test lsp_client \
  end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored
  [lsp-e2e] 5110 lignes, 150 modules : p50=116.7 ms p95=163.5 ms        exit 0

two distinct ubuntu-latest runners, same image, pinned 1.95.0
  run 43622599083 : digest 9b7f16cc…cdf (3 887 400 o), identique a/b   PASS
  run 43626136359 : digest c1ad5ca9…ecc (3 887 408 o), identique a/b   PASS

performance.yml (run 43622599048) : 40 comparaisons, plafond tenu      PASS
linux-qual.yml, job installer (run 43625955751, job 132541685225)
  25 vérifications shell + 30 invariants du manifeste                  PASS

python tools/unsafe_audit.py                    PASS (4 forbid, 1 deny, 2 allow)
python -m unittest discover -s tools/tests      60 tests OK            exit 0
cargo fmt --all -- --check                                             exit 0
pwsh -File scripts/evidence.ps1                 7 contrôles PASS, evidence/current.json
```

## 3. Ce que cette campagne a réellement changé

- **Sécurité** : le confinement `unsafe` n'est plus une intention — les quatre
  crates du langage ne **compilent pas** avec un `unsafe` non listé, le CLI n'a
  que deux `allow` audités, et un outil CI compare l'inventaire aux attributs.
  Six sites `// SAFETY:` documentent l'argument. Limite écrite : ce n'est pas un
  audit humain (D4/R13).
- **Mesure** : la reproductibilité a désormais une mesure **inter-machines**
  (deux runners, deux runs, deux commits) et la performance une **alarme** CI
  avec baseline versionnée. La couverture a gagné 25 tests de comportement et un
  outil de split corrigé, qui change la lecture du trou réel (88,55 % produit,
  pas 86,41 %).
- **Distribution** : l'installateur Linux a un contrat public **exercé en CI**
  (manifeste, SHA-256, désinstallation idempotente, codes d'usage), pas seulement
  « installé une fois ».
- **Documentation** : politique de dépréciation, guide utilisateur, notes de
  version DRAFT, dossier de 21 décisions humaines, et corrections factuelles
  (42 ADR, 10 verticales, `NodeId` u64, `--verify-build`).
- **Mutation** : `codegen/emit.rs` entre enfin dans la campagne ciblée ; les
  24 survivants historiques sont reproduits à l'identique, 6 nouveaux sont
  triés (4 équivalents de formatage, 1 lacune de test fermée par un nouveau
  test, 1 lacune assumée).

## 4. Ce qui reste humain (non fermé par construction)

| Sujet | Pourquoi ce n'est pas un travail d'agent |
|---|---|
| Audit externe D4/R13/T4 | responsabilité humaine indépendante ; brief rafraîchi, toujours `BLOCKED` |
| Go/No-Go T12 | acte humain ; aucun label de release |
| Ratifications D2, D7, D10, D11, NodeId, règle `rt`, AGENTS.md, RFC | `PENDING_HUMAN_APPROVAL` ; dossier §2 |
| Clé de signature D9/R12 | custody hors ligne ; aucune clé créée |
| Test de puissance T2 | méthode à trancher (substitut vs revert) ; la campagne nominale est faite |
| `OwnFacts` étape C / phase 4.5, phase 8 DSP, voie C T1 | décisions d'architecture |
| Objectif 100 % de couverture | D10 : acceptation ou changement de mesure |
| Cible LSP 50 ms à l'échelle | D-2026-11-14-5 : périmètre ou incrémentalité |
| Déviation littérale T4 (ids dans les symboles) | D-2026-11-14-4 : accepter la normalisation |
| `i128`/`u128` FFI, unwinding/`longjmp`/signal | C-FFI-2 et C-FFI-4 à trancher |

## 5. Ce que ce rapport ne prouve pas

- Aucun gate n'est passé à `PASS` : T0/T3/T5/T6/T7/T10 sont inchangés dans le
  registre, T4/T12 restent `BLOCKED`, et les `PARTIAL` le restent.
- La campagne nominale oracle est une **mesure sur un générateur déterministe**,
  pas une preuve sur tous les programmes ; le test de puissance reste bloqué.
- Les 30 survivants de mutation sont **triés**, pas éliminés ; 24 le sont par les
  classes E1–E6, 4 sont équivalents de formatage, 2 sont des lacunes de test
  (une fermée, une assumée).
- La couverture à 90,44/89,88 ne dit rien de la correction : un `match` géant
  peut être couvert sans être testé — c'est pourquoi la mutation existe.
- La reproductibilité est prouvée entre deux runners de **même image et même
  toolchain**, pas entre OS, versions de `rustc` ou linkers différents.
- Aucun artefact n'est signé, aucune release n'existe, aucun audit externe n'a
  eu lieu.

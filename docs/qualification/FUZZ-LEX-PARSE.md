# FUZZ-LEX-PARSE — fuzzing déterministe du lexer/parser/formatter Ravum

> **Statut : PARTIAL / candidat, non ratifié.** Campagne de fuzzing déterministe
> exécutée localement. Elle établit des **faits observés** sur un corpus
> reproductible ; elle ne **prouve** ni la correction, ni l'absence de panique en
> général, ni la sûreté mémoire. Trois violations de propriété ont été trouvées,
> **toutes dans le formateur** ; aucune dans le lexer ni dans le parser.

| Élément | Valeur |
|---|---|
| Branche | `feat/lang-core-1.0-rc` |
| Commit (HEAD au moment des runs) | `8fd154726ae97c172c8be55fce811cb3f499653f` |
| État du working tree | modifications **concurrentes non commitées** présentes pendant la campagne (`ast.rs`, `parser.rs`, sema/codegen/cli par d'autres workers) ; les résultats sont liés à l'arbre effectivement compilé, pas au seul commit HEAD |
| Date des runs | 2026-11-09 (Europe/Paris) |
| Toolchain | `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` |
| OS / matériel | Windows, machine de développement locale (référence 16 Go) |
| Harness | `crates/ravum-lang-syntax/tests/fuzz_lex_parse.rs` (nouveau, test d'intégration) |
| Dépendances | aucune (std uniquement ; pas de crate ajoutée, pas de `Cargo.toml` modifié) |
| Fichiers source modifiés par cette mission | aucun (`src/`, `Cargo.toml` et tests existants intacts ; seuls le harness et ce document sont ajoutés) |

## 1. Modèle de menace et objectifs

Entrée considérée : une source `.rv` **non fiable**, atteignant l'API publique
`lexer::lex`, `parser::parse_file` et `fmt::format_source` sous forme de `&str`
(UTF-8 valide par construction du type).

Objectifs vérifiés (invariants) :

1. **Pas de panique, pas de boucle infinie** sur toute entrée générée.
2. **Déterminisme** : une même entrée produit exactement les mêmes tokens, le
   même AST et les mêmes diagnostics (codes, spans, messages).
3. **Spans dans les bornes** : tout span de token ou de diagnostic vérifie
   `file == 0`, `start <= end <= src.len()` ; le token `EOF` est à
   `src.len()`.
4. **Formateur sûr sur parse propre** : si `parse_file` ne signale aucune erreur,
   alors `format_source` réussit, sa sortie **re-parse sans erreur**, le
   formateur est **idempotent** (`fmt(fmt(x)) == fmt(x)`) et l'AST re-parsé est
   **structurellement identique** (ids/spans exclus).

Hors périmètre (assumé) :

- sûreté mémoire formelle, absence d'UB, comportement sous Miri/ASan/UBSan ;
- entrées UTF-8 invalides **au niveau du lexer** : l'API reçoit un `&str`, donc
  la validation UTF-8 est en amont (LEX-2.1/LEX-2.3). Le harness teste la
  frontière : rejet déterministe par `str::from_utf8`, préfixe valide, et
  remplacement *lossy* ;
- correction sémantique (checker, codegen), résolution de modules, effets ;
- consommation mémoire/CPU mesurée précisément (seule la durée murale des runs
  est rapportée) ;
- concurrence, interruptions, filesystem.

## 2. Générateurs (déterministes)

Générateur **xorshift64** maison (décalages 13/7/17, état non nul), seeds fixes.
Aucune dépendance à l'heure, à l'adresse ou à l'ordre d'un `HashMap`.

- **Seed de base** : `short = 0x9E3779B97F4A7C15`, `long = short ^ 0xA5A5A5A5A5A5A5A5`.
- **Seed d'itération** : `base ^ (i * 0x9E3779B97F4A7C15 + 0xD1B54A32D192ED03)`.
- Familles sélectionnées par `iteration % 4` :
  1. **caractères aléatoires** — alphabet pondéré (ASCII de syntaxe, NUL, `\r`,
     `\n`, `\t`, `#`, `@`, `$`, multi-octets `é λ 😀`, BOM `U+FEFF`,
     espace de largeur nulle), avec runs longs occasionnels (64–575 caractères) ;
  2. **token soup** — assemblage aléatoire de mots-clés, identifiants, 46
     ponctuations, littéraux (dont `10z`, `1e9`, `1.5e-3f64`, `1.`,
     `"unterminated`, `0xFF_u16`), commentaires (fermés/non fermés) et blancs ;
  3. **mutations** — 8 programmes-sources (hello, struct/impl, fib, enum/match,
     boucles, attribut, commentaires, effets) mutés 1 à 4 fois : troncature,
     suppression, duplication, remplacement par caractères aléatoires,
     insertion, insertion de ponctuation. Toutes les opérations respectent les
     frontières de caractères (`char_indices`) ;
  4. **octets aléatoires** — 0–96 octets mêlant octets uniformes et «
     intéressants » (`0x00,0x0d,0x22,0x27,0x5c,0xc3,0xa9,0xef,0xbb,0xbf,0xf0,
     0x9f,0x98,0x80,0xff`), parfois suivis de `é😀\0\u{feff}`.
- **Cas limites explicites** (exécutés à chaque run) : BOM en tête et au
  milieu, NUL hors/avec littéral, CRLF et CR isolé, attribut, closure, `unsafe`,
  pointeur brut, `0..10`, `1e9`, identifiant de 100 000 octets, chaîne de
  100 000 octets (50 000 `é`), commentaire de 100 000 octets, commentaire non
  fermé de 100 000 octets, entier de 10 000 chiffres, 600 parenthèses
  imbriquées, 2 000 instructions, et **tous les préfixes** (frontières de
  caractères) d'une source Unicode.

Pour chaque entrée, le harness exécute `lex` deux fois (sans trivia), `lex` une
fois avec trivia, `parse_file` deux fois, puis, sur parse propre, `format_source`
deux fois et `parse_file` sur la sortie formatée.

### Digest inter-runs

Un digest FNV-1a 64 bits sur `(seed, itération, longueur, octets source, puis
pour chaque diagnostic : code, span, sévérité, message)` est imprimé en fin de
run. Deux exécutions avec le même budget et les mêmes seeds doivent produire le
**même digest** : c'est la preuve de déterminisme inter-processus utilisée ici.

### Échec

En mode normal, le premier échec imprime générateur, seed, itération, raison, un
hex dump (≤ 16 Kio, longueur totale indiquée) et une commande de repro, puis
fait échouer le test. En mode **collecte** (`RAVUM_FUZZ_COLLECT=1`), les échecs
sont capturés (`catch_unwind`), le run continue, et un rapport consolidé (≤ 40
échecs détaillés par run, compteur des suivants) est imprimé à la fin ; le test
échoue quand même.

## 3. Budgets et commandes

| Mode | Budget par défaut | Commande |
|---|---|---|
| court (toujours exécuté) | 512 itérations | `cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --nocapture` |
| long, fast-fail | 20 000 (ou `RAVUM_FUZZ_ITERS`) | `RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture` |
| long, collecte | idem | `RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture` |

`RAVUM_FUZZ_ITERS` remplace le budget des deux tests. Le budget long par défaut
est 20 000 si la variable est absente.

## 4. Résultats exacts (2026-11-09, commit `8fd1547`)

### 4.1 Run court (512 itérations) — **PASS**

```
[fuzz:short] base_seed=0x9e3779b97f4a7c15 budget=512 inputs=690 parse_ok=113
parse_err=577 fmt_ok=113 invalid_utf8_inputs=127 tokens=25535 diagnostics=12033
bytes=496469 digest=0x88545f7031cb510e failures=0 dropped_failures=0 elapsed_ms=229..292
```

- `cargo test` : `1 passed, 1 ignored`, durée test ≈ 0,25–0,30 s selon l'exécution.
- Digest stable entre exécutions (même valeur observée sur l'arbre courant et sur
  l'arbre antérieur aux modifications concurrentes).

### 4.2 Run long 200 000, mode fast-fail — **FAIL (attendu tant que FZ-1 est ouvert)**

- Arrêt à l'**itération 10 042**, générateur `mutation`, seed
  `0x2640fc82dbb5e575`, durée du test ≈ 1,44 s (le run s'arrête au premier
  échec, après les cas limites) ; échec **FZ-1** ci-dessous.

### 4.3 Run long 200 000, mode collecte — **FAIL, 3 échecs / 0 perte**

Sur l'arbre courant, deux exécutions indépendantes ont produit **exactement**
les mêmes statistiques, le même digest et les mêmes échecs (durées internes :
24 753 ms et 31 539 ms ; ~25 s et ~31,9 s murales) :

```
[fuzz:long] base_seed=0x3b92dc1cdaefd9b0 budget=200000 inputs=249054
parse_ok=31108 parse_err=217946 fmt_ok=31105 invalid_utf8_inputs=49003
tokens=5216641 diagnostics=4699197 bytes=23866313
digest=0x9ca81d63781e9d75 failures=3 dropped_failures=0 elapsed_ms=24753..31539
```

- 200 000 itérations + cas limites ⇒ **249 054 entrées** effectivement
  analysées (les entrées UTF-8 invalides donnent un préfixe valide + une vue
  lossy).
- `parse_ok = 31 108`, dont **31 105** ont passé l'ensemble des invariants du
  formateur ; les 3 autres sont les échecs ci-dessous.
- **0** panique/OOB/déterminisme côté lexer ou parser. Toutes les violations
  trouvées sont des propriétés **post-parse-propre du formateur**.
- Le digest couvre aussi le **texte et les spans des diagnostics** : sur un
  arbre antérieur (avant des modifications concurrentes de `ast.rs`/`parser.rs`
  par un autre worker), deux runs donnaient `0x93cf1e0e8517f091` avec des
  compteurs et **les mêmes trois échecs identiques**. Le digest est donc une
  empreinte de l'arbre compilé, pas une constante inter-arbres ; la
  reproductibilité exigée ici est « même arbre ⇒ même digest ».

## 5. Bugs trouvés (non corrigés : hors périmètre `src/`)

> Conformément à la mission, aucun fichier `src/` n'a été modifié. Les deux bugs
> concernent le formateur ; ils sont reproduits via la CLI.

### FZ-1 — le formateur fusionne `&` `&` en `&&` (sortie non re-parsable)

- **Entrée** (31 octets, hex) :
  `66 6e 20 6c 6f 67 28 73 3a 26 20 26 73 74 72 29 20 65 66 66 65 63 74 73 28 29 20 7b 20 7d 0a`
  soit `fn log(s:& &str) effects() { }` + LF.
- **itérations/seeds** : `10042` / `0x2640fc82dbb5e575` et `130310` /
  `0xef952abe3ce84f31` (deux mutations distinctes ont convergé vers le même
  défaut).
- **Parse d'entrée** : OK (aucun diagnostic).
- **Sortie du formateur** : `fn log(s: &&str) effects() {}` — l'espace entre les
  deux `&` a disparu ; la re-lexicalisation produit le token unique `&&`
  (`AmpAmp`) au lieu de deux `Amp`.
- **Re-parse** : 3 × `E-PARSE-EXPECTED` (`expected a type, found '&&'`, puis
  cascade sur `,`/`)`/`:`).
- **Hypothèse de cause** : règle d'espacement du formateur qui colle un opérateur
  unaire `&` à ce qui suit ; deux `&` consécutifs ne doivent **jamais** être
  collés, car `&&` est un autre token. Classe de risque analogue pour d'autres
  paires de ponctuations dont la concaténation forme un opérateur plus long
  (`| |` → `||`, `. .` → `..`, `= =` → `==`, etc.).
- **Repro CLI** (fichier temporaire) :

  ```bash
  printf 'fn log(s:& &str) effects() { }\n' > amp.rv
  ravum-lang fmt amp.rv
  cat amp.rv          # fn log(s: &&str) effects() {}
  ravum-lang check amp.rv
  # exit 1; 3 × E-PARSE-EXPECTED
  ```

### FZ-2 — formateur non idempotent avec un CR dans un commentaire de ligne

- **Entrée** (7 octets, hex) : `2f 2f 3c 24 61 0d 20` soit `//<$a\r ` (CR
  isolé suivi d'un espace, sans LF).
- **itération/seed** : `57042` / `0x8d54dca30ea9358d`, générateur `mutation`.
- **Comportement observé (CLI, octets du fichier après chaque `fmt`)** :
  - après `fmt` #1 : `2f 2f 3c 24 61 0d 0a` (`//<$a\r\n`) ;
  - après `fmt` #2 : `2f 2f 3c 24 61 0a` (`//<$a\n`) ;
  - donc `fmt(fmt(x)) != fmt(x)` ; l'invariant d'idempotence échoue.
- **Hypothèse de cause** : lors de l'émission d'un commentaire de ligne, le
  formateur retire un `\r` final (`trim_end_matches('\r')`) **avant** que
  `newline()` ne retire les espaces finaux ; le texte du token est `//<$a\r `
  (il finit par un espace), donc le `\r` survit au premier passage puis
  disparaît au second.
- **Repro CLI** :

  ```bash
  printf '\x2f\x2f\x3c\x24\x61\x0d\x20' > cr.rv
  ravum-lang fmt cr.rv
  xxd cr.rv           # ... 0d 0a
  ravum-lang fmt cr.rv
  xxd cr.rv           # ... 0a   (le CR a disparu -> non idempotent)
  ```

- **Note connexe, hors périmètre fuzz** : LEX-3.3 prévoit qu'un CR isolé soit
  `E-UNSUPPORTED-TOKEN` ; le lexer actuel l'accepte. Même si cette règle était
  implémentée, le formateur doit rester idempotent sur toute entrée qu'il
  accepte (ou refuser le formatage avec `Err`).

## 6. Verdict par composant

| Composant | Invariants vérifiés | Verdict |
|---|---|---|
| Lexer (`lex`, trivia vrai/faux) | pas de panique, déterminisme, spans/EOF, bornes | **PASS** sur le corpus (249 054 entrées, 5 216 641 tokens) |
| Parser (`parse_file`) | pas de panique, déterminisme AST+diags, bornes | **PASS** sur le corpus (4 699 197 diagnostics) |
| Formateur (`format_source`) | succès si parse propre, re-parse propre, idempotence, AST préservé | **FAIL** : 3 violations / 200 000 itérations, 2 causes racines (FZ-1, FZ-2) |

Le run court (512 itérations) est vert et peut rester dans la suite de tests
comme smoke test rapide ; le run long échoue volontairement tant que FZ-1/FZ-2
sont ouverts (il documente alors l'input reproductible).

## 7. Limites honnêtes

- **Ce n'est pas une preuve.** Un corpus déterministe, même de 249 054 entrées,
  ne couvre pas l'espace des entrées possibles ; aucune métrique de couverture
  (lignes/branches) n'est calculée, et l'absence d'échec sur ce corpus ne
  garantit pas l'absence de bug.
- Les seeds et les générateurs sont fixés par conception : changer un seed ou
  l'algorithme change le corpus (reproductibilité, pas exhaustivité).
- Les budgets et durées dépendent de la machine (build `debug`, non
  instrumenté) ; seuls les invariants (pas les durées) sont des assertions.
- **UTF-8 invalide** : l'API lexer prend un `&str` ; le harness ne peut pas lui
  transmettre des octets invalides. Il vérifie le rejet déterministe à la
  frontière, le préfixe valide et la vue *lossy* — ce n'est pas un test du
  lexer sur UTF-8 invalide.
- L'égalité d'AST après formatage compare un rendu `Debug` **normalisé**
  (ids/spans effacés). Un littéral de chaîne contenant `id: ` ou `Span {`
  pourrait être normalisé à tort, mais les deux côtés subissent la même
  normalisation ; le test reste valide pour la préservation de structure
  observée.
- Le dump d'échec est tronqué (16 Kio en fast-fail, 4 Kio dans le rapport
  collecté) ; la longueur totale et la seed sont toujours fournies, et les cas
  limites sont nommés (`edge-*`) pour une reproduction exacte.
- Le mode collecte est un outil d'exploration : il continue après panique
  (`catch_unwind`) et ne remplace pas le mode fast-fail par défaut. Les
  compteurs incluent le travail partiel des itérations en échec.
- Aucune exécution sous Miri, ASan/UBSan, ni mesure de RSS/CPU ; aucun test de
  concurrence, de filesystem ou d'entrées non-UTF-8 au-delà de la frontière
  décrite.
- La correction sémantique (types, ownership, effets, codegen) n'est **pas**
  couverte par ce harness.

## 8. Repro général

```bash
# Smoke test court (vert)
cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --nocapture

# Long fast-fail (s'arrête sur FZ-1 à l'itération 10042)
RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture

# Long complet (collecte, 3 échecs, digest stable)
RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture
```

Les résultats ci-dessus sont liés au commit `8fd1547`, à `rustc 1.95.0`, à
l'état du working tree au moment des runs (modifications concurrentes d'autres
workers incluses) et à la machine locale citée ; ils ne constituent pas un
résultat de benchmark ni une preuve de correction du langage.

## 9. Correctifs et re-run (Lead, 2026-11-09)

Les deux bugs de formateur ont été corrigés dans `fmt.rs` :

- **FZ-1** : `space_between` force un espace entre deux tokens `&` (et deux
  tokens `|`) ; `& &str` n'est plus fusionné en `&&str`.
- **FZ-2** : les commentaires de ligne sont trimmés de `[' ', '\t', '\r']` en
  une passe ; plus de CR résiduel, idempotence en deux passes.
- Tests de régression ajoutés : `consecutive_amp_tokens_are_not_merged`,
  `line_comment_with_trailing_cr_is_idempotent`.

Re-run long collecte sur l'arbre corrigé :
`RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture`
→ `inputs=249054 parse_ok=31108 parse_err=217946 fmt_ok=31108 tokens=5216641
diagnostics=4699197 digest=0x9ca81d63781e9d75 failures=0 elapsed_ms=26305`.

Statut : **PASS sur ce corpus déterministe** (2 runs identiques, 0 échec),
toujours **pas une preuve** ; le fuzz ne couvre ni la sémantique, ni Miri/ASan,
ni les entrées non-UTF-8 au-delà de la frontière décrite.

# Spécification exploratoire — universalité, web, backend, bases et distribution

## Profils de cibles et capacités

| Target | Runtime autorisé | Principales garanties | Fonctionnalités différées |
|---|---|---|---|
| Linux/Windows natif | std/async, permissions explicites | ownership, effets, contrats, replay | bare-metal |
| Browser WASM | JS bridge / Web APIs contrôlées | UI/API typées ; client sans secret serveur | accès direct filesystem arbitraire |
| WASI Component | imports/exports WIT, hôte à capacités | frontières composants typées | runtime support dépend version |
| Server API | I/O réseau, DB en capacités | authz, transaction, backpressure | WCET hard-RT |
| Desktop natif | toolkit adapter | UI/threads/state/hot reload | modèle pixel perfect intrinsèque |
| Audio plugin | runtime RT-safe + nonRT | no-alloc/no-block sous modèle | modules SQL dans callback |
| Bare-metal/MCU | core/no_std, allocation optional | modèle mémoire + effets embarqués | daemon/hotpatch obligé |
| Mobile ARM | adaptateurs OS, signatures stores | permissions, wrappers et packaging | couverture immédiate complète |
| GPU | kernels restrictions, shader bridge | contraintes ciblées | preuve universelle GPU |

## Frontend et interfaces

Un composant `@client` ne peut importer une capacité `Secret`/`DbWrite` serveur. Les `server_fn` exposent une signature RPC typée versionnée ; le serveur revalide systématiquement identité, droits et données. Vérifier CSP/CSRF/XSS et hydratation, pas seulement les types.

## SQL et migrations

- Schéma canonique signé/versionné, snapshots `dev/staging/prod` avec sources explicites.
- Paramétrisation des valeurs ; validité des requêtes au moment du build, avec version/dialecte de référence.
- En déploiement multi-version, appliquer expand/contract et tester clients N/N-1 ; migration destructive interdite sans protocole spécifique et sauvegarde.
- Isolation transactionnelle, contraintes d'unicité, retry et idempotence testés sur un vrai moteur.
- Les propriétés de transaction ne sont pas entièrement prouvées sans modèle fiable de SGBD ni effets externes.

## ABI et composants

- ABI interne versionnée, signatures et layout hashés ; « type identique » ne signifie pas « même ABI ».
- Interfaces externes WIT/WASI lorsqu'adaptées, fallback JS/C stable explicite. WASI 0.3 support async/streams/futures mais la prise en charge varie par runtime/toolchain.
- Toute frontière FFI ou native comprend manifest de capacités, modèle d'erreur et propriétaire de mémoire.

## Déploiements de référence requis

1. UI WASM + service HTTP natif + PostgreSQL, authentification factice, migrations réversibles et tests de compatibilité.
2. Plugin DSP native avec host réel ou harness compatible VST3/CLAP choisi par ADR ultérieur et instrumenté RT.
3. Service concurrent streaming avec simulation de pannes, métriques mémoire et latence.
4. Exemple no_std bare-metal dès que la sémantique noyau se stabilise.

Aucun de ces quatre déploiements de référence n'existe dans le pack documentaire. Les
plateformes réellement qualifiées aujourd'hui sont limitées à **Windows x86-64**, **Linux
x86-64** (une image CI `ubuntu-latest`) et **WASI preview 1** (`wasm32-wasip1`) — voir
[`docs/qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) ; les autres
cibles de ce document restent des exigences, pas des supports annoncés.

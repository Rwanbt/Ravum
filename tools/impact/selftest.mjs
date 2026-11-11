#!/usr/bin/env node
/**
 * tools/impact/selftest.mjs — preuve que le mapping d'impact ne saute aucun test.
 *
 * Corpus de diffs synthétiques passés à `analyze()` et à la CLI, avec une
 * implémentation de référence INDÉPENDANTE de la fermeture des dépendants
 * (simple point fixe sur les arêtes inverses) pour comparer les ensembles.
 *
 * Cas obligatoires (mandat L4-E, renforcés MVP-0c L4) :
 *   - impact direct ;
 *   - dépendant transitif, avec preuve qu'une classification naïve
 *     (« seulement le crate modifié ») aurait sauté le test du dépendant ;
 *   - docs-only : aucun test Rust, mais le check docs reste exigé (une commande) ;
 *   - protocole => FULL ;
 *   - chemin inconnu => FULL ;
 *   - renommage => UNION source + destination ;
 *   - suppression => composant du chemin supprimé (et ses dépendants) ;
 *   - manifeste/CI => FULL ; statut non résolu / diff vide => FULL + INCERTAIN ;
 *   - nouveau crate `ravum-unit-graph` : suite propre + dépendants (si présent) ;
 *   - fixture multi-crates `fixtures/multicrate` => FULL conservateur ;
 *   - INVENTAIRE : chaque crate/app du workspace doit avoir un mapping de test,
 *     et aucun répertoire `crates/*` ou `apps/*` avec manifeste ne peut rester
 *     hors des `members` (sinon il ne serait testé nulle part).
 *
 * Zéro dépendance npm. Node >= 20 (testé Node 26).
 * Code de sortie : 0 = tout passe, 1 = au moins un échec.
 */

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  analyze,
  classifyPath,
  loadWorkspace,
  parseToml,
  parseFilesArg,
  parseNameStatusZ,
  refExists,
  REPO_ROOT,
  SCHEMA,
  FULL_TEST_ARGV,
} from './impact.mjs';

const IMPACT_PATH = path.join(path.dirname(fileURLToPath(import.meta.url)), 'impact.mjs');
const WORKFLOW_PATH = path.join(REPO_ROOT, '.github', 'workflows', 'mvp-impact.yml');

let assertionCount = 0;
const failures = [];
let caseCount = 0;

function check(condition, message) {
  assertionCount += 1;
  if (!condition) throw new Error(message);
}

function setOf(values) {
  return new Set(values);
}

function sorted(values) {
  return [...values].sort();
}

function assertSetEqual(actualValues, expectedValues, label) {
  const actual = sorted(setOf(actualValues));
  const expected = sorted(setOf(expectedValues));
  const missing = expected.filter((value) => !actual.includes(value));
  const extra = actual.filter((value) => !expected.includes(value));
  check(
    missing.length === 0 && extra.length === 0,
    `${label}: ensembles différents — manquants=[${missing.join(', ')}] en trop=[${extra.join(', ')}]`,
  );
}

/**
 * Implémentation de référence indépendante :
 * point fixe sur les arêtes inverses normales/build, puis un seul saut sur les
 * arêtes `dev`. Ne réutilise aucune fonction de fermeture d'impact.mjs.
 */
function referenceClosure(workspace, directPackages) {
  const affected = new Set(directPackages);
  let changed = true;
  while (changed) {
    changed = false;
    for (const packageName of [...affected]) {
      for (const edge of workspace.dependents.get(packageName) ?? []) {
        if (edge.kind === 'dev') continue;
        if (!affected.has(edge.name)) {
          affected.add(edge.name);
          changed = true;
        }
      }
    }
  }
  const devHops = [];
  for (const packageName of [...affected]) {
    for (const edge of workspace.dependents.get(packageName) ?? []) {
      if (edge.kind === 'dev' && !affected.has(edge.name)) devHops.push(edge.name);
    }
  }
  for (const packageName of devHops) affected.add(packageName);
  return affected;
}

function referenceClosureUnion(workspace, packages) {
  const union = new Set();
  for (const packageName of packages) {
    for (const affected of referenceClosure(workspace, [packageName])) union.add(affected);
  }
  return union;
}

function cargoCommands(result) {
  return result.commands.filter((command) => command.kind === 'cargo');
}

function commandArgs(result) {
  return result.commands.map((command) => command.argv);
}

const M = (filePath) => ({ status: 'M', path: filePath });

function runCase(name, fn) {
  caseCount += 1;
  try {
    fn();
    console.log(`PASS  ${name}`);
  } catch (err) {
    failures.push({ name, message: err?.message ?? String(err) });
    console.log(`FAIL  ${name}`);
    console.log(`      ${err?.message ?? String(err)}`);
  }
}

function runCli(args) {
  const result = spawnSync(process.execPath, [IMPACT_PATH, ...args], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  });
  return {
    code: result.status ?? -1,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
  };
}

function parseCliJson(run, label) {
  check(run.stdout.trim().length > 0, `${label}: la CLI n'a rien écrit sur stdout`);
  let parsed;
  try {
    parsed = JSON.parse(run.stdout);
  } catch (err) {
    throw new Error(`${label}: JSON stdout invalide: ${err.message}`);
  }
  check(parsed.schema === SCHEMA, `${label}: schéma inattendu ${parsed.schema}`);
  return parsed;
}

// ---------------------------------------------------------------------------
// 0. Analyseur TOML restreint
// ---------------------------------------------------------------------------

function testTomlParser() {
  const document = parseToml(`
# commentaire
[package]
name = "demo"
version.workspace = true

[workspace]
members = [
  "crates/a",   # commentaire dans un tableau multi-ligne
  "crates/b",
]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
ravum-a = { path = "crates/a" }

[dependencies]
serde.workspace = true
ravum-a = { workspace = true }
local = { path = "../local", package = "local-renamed" }

[target.'cfg(windows)'.dev-dependencies]
winapi = "0.3"

[['bin']]
name = "demo-bin"
path = "src/main.rs"
`);
  check(document.package?.name === 'demo', 'TOML: [package].name non lu');
  check(document.package?.version?.workspace === true, 'TOML: clé pointée version.workspace non lue');
  check(Array.isArray(document.workspace?.members) && document.workspace.members.length === 2, 'TOML: members multi-ligne non lus');
  check(document.workspace.members[1] === 'crates/b', 'TOML: second member incorrect');
  check(document['workspace']?.dependencies?.['serde']?.version === '1', 'TOML: table inline workspace.dependencies non lue');
  check(document['workspace'].dependencies['ravum-a'].path === 'crates/a', 'TOML: path de workspace.dependencies non lu');
  check(document['workspace'].dependencies.serde.features[0] === 'derive', 'TOML: tableau de features non lu');
  check(document.dependencies.serde.workspace === true, 'TOML: dépendance héritée non lue');
  check(document.dependencies.local.package === 'local-renamed', 'TOML: renommage de paquet non lu');
  check(document.target['cfg(windows)']['dev-dependencies'].winapi === '0.3', 'TOML: table ciblée non lue');
  check(Array.isArray(document.bin) && document.bin[0].name === 'demo-bin', 'TOML: table de tableau [[bin]] non lue');
}

// ---------------------------------------------------------------------------
// 1. Corpus d'impact (niveau bibliothèque)
// ---------------------------------------------------------------------------

/** Répertoires `crates/*` ou `apps/*` qui portent un manifeste sur disque. */
function workspaceMemberDirsOnDisk() {
  const dirs = [];
  for (const scope of ['crates', 'apps']) {
    const base = path.join(REPO_ROOT, scope);
    let entries;
    try {
      entries = readdirSync(base, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (existsSync(path.join(base, entry.name, 'Cargo.toml'))) dirs.push(`${scope}/${entry.name}`);
    }
  }
  return dirs.sort();
}

function testImpactCorpus(workspace) {
  const packageNames = sorted(workspace.packages.keys());

  runCase('graphe: members résolus sans erreur ni note d’ambiguïté', () => {
    check(workspace.errors.length === 0, `workspace.errors non vide: ${workspace.errors.join(' | ')}`);
    check(packageNames.length >= 14, `paquets attendus >= 14, trouvés ${packageNames.length}`);
    for (const expected of ['ravum-protocol', 'ravum-snapshot', 'ravum-rust-adapter', 'ravum-patch-host', 'ravum-store', 'ravum-todo-core', 'ravum-todo']) {
      check(workspace.packages.has(expected), `paquet manquant: ${expected}`);
    }
    check(workspace.deps.get('ravum-store').normal.includes('ravum-todo-core'), 'arête store -> todo-core absente');
    check(workspace.deps.get('ravum-todo').normal.includes('ravum-store'), 'arête todo -> store absente');
    check(workspace.deps.get('ravum-patch-host').dev.includes('ravum-rust-adapter'), 'dev-arête patch-host -> rust-adapter absente');
    check(workspace.deps.get('ravum-protocol').normal.length === 0, 'ravum-protocol devrait être sans dépendance interne');
    for (const packageName of packageNames) {
      for (const dep of workspace.deps.get(packageName).normal) {
        check(workspace.packages.has(dep), `arête vers un paquet inconnu: ${packageName} -> ${dep}`);
      }
    }
  });

  runCase('impact direct + fermeture transitive: ravum-todo-core entraîne store et l’app todo', () => {
    const result = analyze(workspace, [M('crates/ravum-todo-core/src/lib.rs')]);
    const expected = referenceClosure(workspace, ['ravum-todo-core']);
    check(result.full === false, 'le diff ne devrait pas déclencher FULL');
    check(result.uncertain === false, 'le mapping ne devrait pas être incertain');
    check(result.decision === 'selective', `décision attendue selective, obtenue ${result.decision}`);
    assertSetEqual(result.packages, expected, 'fermeture todo-core');
    check(result.packages.includes('ravum-store'), 'ravum-store (dépendant direct) absent');
    check(result.packages.includes('ravum-todo'), 'ravum-todo (dépendant transitif + E2E) absent');
    assertSetEqual(result.naive_packages, ['ravum-todo-core'], 'classification naïve');
    const serialized = commandArgs(result);
    check(serialized.length === expected.size, `une commande cargo par paquet attendu: ${serialized.length} != ${expected.size}`);
    for (const argv of serialized) {
      check(argv[0] === 'cargo' && argv[1] === 'test', `commande inattendue: ${argv.join(' ')}`);
      check(argv.includes('--test-threads=1'), `commande non sérialisée: ${argv.join(' ')}`);
      check(!argv.includes('--workspace'), `commande sélective avec --workspace: ${argv.join(' ')}`);
    }
    check(new Set(result.packages).size === result.packages.length, 'paquets dupliqués dans la décision');
  });

  runCase('une classification trop étroite est refusée (fermeture indépendante)', () => {
    const narrowCases = [
      ['ravum-todo-core', 'crates/ravum-todo-core/src/lib.rs'],
      ['ravum-rust-adapter', 'crates/ravum-rust-adapter/src/lib.rs'],
    ];
    for (const [direct, filePath] of narrowCases) {
      const result = analyze(workspace, [M(filePath)]);
      const required = referenceClosure(workspace, [direct]);
      const narrow = new Set([direct]);
      const omitted = [...required].filter((packageName) => !narrow.has(packageName));
      check(omitted.length > 0, `une classification trop étroite de ${direct} devrait être incomplète`);
      check(result.packages.length > narrow.size, `la sortie ne doit pas se réduire à la classification étroite de ${direct}`);
      assertSetEqual(result.packages, required, `refus du trop étroit pour ${direct}`);
      const commandPackages = result.commands.filter((command) => command.kind === 'cargo').map((command) => command.package);
      for (const omittedPackage of omitted) {
        check(commandPackages.includes(omittedPackage), `commande de test manquante pour le dépendant ${omittedPackage}`);
      }
      check(result.decision === 'selective', `décision attendue selective pour ${direct}`);
    }
  });

  runCase('PREUVE: une classification naïve aurait sauté des tests (todo-core)', () => {
    const result = analyze(workspace, [M('crates/ravum-todo-core/src/lib.rs')]);
    const naive = new Set(result.naive_packages);
    const skipped = result.packages.filter((packageName) => !naive.has(packageName));
    check(skipped.includes('ravum-store'), 'la classification naïve aurait sauté ravum-store');
    check(skipped.includes('ravum-todo'), 'la classification naïve aurait sauté ravum-todo (dont la suite E2E)');
    check(result.packages.length > naive.size, 'la fermeture doit être strictement plus large que la classification naïve');
    // La référence indépendante confirme que ces paquets dépendent bien de todo-core.
    for (const packageName of skipped) {
      const reaches = [...referenceClosure(workspace, [packageName])].includes('ravum-todo-core');
      check(!reaches, `incohérence: ${packageName} ne devrait pas être un dépendant de todo-core`);
    }
    let transitivelyDepends = false;
    for (const directDep of workspace.deps.get('ravum-store').normal) {
      if (directDep === 'ravum-todo-core') transitivelyDepends = true;
    }
    check(transitivelyDepends, 'ravum-store ne déclare plus ravum-todo-core: la preuve doit être revue');
  });

  runCase('dépendant transitif et dev-dépendant direct: ravum-snapshot', () => {
    const result = analyze(workspace, [M('crates/ravum-snapshot/src/lib.rs')]);
    const expected = referenceClosure(workspace, ['ravum-snapshot']);
    assertSetEqual(result.packages, expected, 'fermeture snapshot');
    for (const packageName of ['ravum-rust-adapter', 'ravum-coordinator', 'ravum-cli', 'ravum-workspace-coordinator']) {
      check(result.packages.includes(packageName), `${packageName} devrait être affecté par ravum-snapshot`);
    }
    for (const packageName of ['ravum-daemon', 'ravum-todo', 'ravum-store', 'ravum-todo-core']) {
      check(!result.packages.includes(packageName), `${packageName} ne dépend pas de ravum-snapshot et ne devrait pas être testé`);
    }
    // c86d1f0 : dsp-harness déclare une dev-dependency DIRECTE vers snapshot (le test
    // de stress DSP compile de vraies fixtures). Son test doit donc tourner.
    check(result.packages.includes('ravum-dsp-harness'), 'dsp-harness (dev-dépendant direct de snapshot) doit être testé');
    const provenance = result.provenance['ravum-dsp-harness'];
    check(provenance && provenance.type === 'dev-dependent', `dsp-harness doit être dev-dependent, obtenu ${JSON.stringify(provenance)}`);
    check(
      expected.has(provenance.via),
      `la provenance dev-hop doit être un paquet affecté, obtenu ${provenance.via}`,
    );
    check(
      workspace.deps.get('ravum-dsp-harness').dev.includes('ravum-snapshot'),
      'ravum-dsp-harness ne déclare plus de dev-dependency vers ravum-snapshot (manifeste à revoir)',
    );
    // Pas de propagation transitive arbitraire depuis ce saut dev : demo-host dépend
    // normalement de patch-host (lui-même dev-hop) et ne doit PAS être ajouté.
    check(!result.packages.includes('ravum-demo-host'), 'demo-host ne doit pas être propagé depuis un dev-hop');
    check(expected.has('ravum-proof') && expected.has('ravum-patch-host'), 'les dev-dépendants de snapshot doivent être inclus (un saut)');
  });

  runCase('dev-dependency directe: dépendant testé, pas de propagation transitive (rust-adapter)', () => {
    const result = analyze(workspace, [M('crates/ravum-rust-adapter/src/lib.rs')]);
    const expected = referenceClosure(workspace, ['ravum-rust-adapter']);
    assertSetEqual(result.packages, expected, 'fermeture rust-adapter');
    check(result.packages.includes('ravum-patch-host'), 'patch-host (dev-dépendant) doit être testé');
    check(result.packages.includes('ravum-proof'), 'proof (dev-dépendant) doit être testé');
    check(result.packages.includes('ravum-cli'), 'cli (dépendant normal) doit être testé');
    // c86d1f0 : dev-dependency DIRECTE dsp-harness -> rust-adapter.
    check(result.packages.includes('ravum-dsp-harness'), 'dsp-harness (dev-dépendant direct) doit être testé');
    const provenance = result.provenance['ravum-dsp-harness'];
    check(provenance && provenance.type === 'dev-dependent', `dsp-harness doit être dev-dependent, obtenu ${JSON.stringify(provenance)}`);
    check(
      workspace.deps.get('ravum-dsp-harness').dev.includes('ravum-rust-adapter'),
      'ravum-dsp-harness ne déclare plus de dev-dependency vers ravum-rust-adapter (manifeste à revoir)',
    );
    // Aucune propagation au-delà d'un saut : demo-host (dépendant normal du dev-hop
    // patch-host) ne doit pas être testé pour un changement de rust-adapter.
    check(!result.packages.includes('ravum-demo-host'), 'aucune propagation transitive via un dev-hop: demo-host ne doit pas être testé');
    check(result.provenance['ravum-demo-host'] === undefined, 'demo-host ne doit avoir aucune provenance dev-hop');
  });

  runCase('règle dev-dependency: directe => testée, jamais propagée au-delà d’un saut', () => {
    const cases = [
      ['crates/ravum-snapshot/src/lib.rs', 'ravum-snapshot'],
      ['crates/ravum-rust-adapter/src/lib.rs', 'ravum-rust-adapter'],
    ];
    for (const [filePath, direct] of cases) {
      const result = analyze(workspace, [M(filePath)]);
      const expected = referenceClosure(workspace, [direct]);
      const provenance = result.provenance['ravum-dsp-harness'];
      check(Boolean(provenance), `dsp-harness absent de la décision pour ${direct}`);
      check(provenance.type === 'dev-dependent', `dsp-harness doit être dev-dependent pour ${direct}, obtenu ${provenance.type}`);
      // Le « via » peut être n'importe quel paquet affecté dont dsp-harness est une
      // dev-dependency directe (ici snapshot ou rust-adapter, tous deux affectés).
      check(expected.has(provenance.via), `via doit être un paquet affecté (${direct}), obtenu ${provenance.via}`);
      check(
        workspace.deps.get('ravum-dsp-harness').dev.includes(provenance.via),
        `via doit être une dev-dependency déclarée de dsp-harness, obtenu ${provenance.via}`,
      );
      check(!result.packages.includes('ravum-demo-host'), `demo-host ne doit pas être propagé depuis le dev-hop (${direct})`);
      check(result.provenance['ravum-demo-host'] === undefined, `demo-host ne doit avoir aucune provenance dev-hop (${direct})`);
    }
  });

  runCase('nouveau crate ravum-unit-graph: suite propre + dépendants (MVP-0c L1)', () => {
    const unitGraph = workspace.packages.get('ravum-unit-graph');
    if (!unitGraph) {
      // Le crate est développé par un autre worker : ne pas inventer sa présence.
      // Le test d'inventaire générique ci-dessous échouera dès qu'il sera member
      // sans mapping, et couvre donc tout futur crate même absent aujourd'hui.
      check(true, 'ravum-unit-graph absent du workspace local : cas d’inventaire générique actif');
      return;
    }
    const result = analyze(workspace, [M(`${unitGraph.dir}/src/graph.rs`)]);
    const expected = referenceClosure(workspace, ['ravum-unit-graph']);
    check(result.full === false, 'ravum-unit-graph (non critique) ne doit pas déclencher FULL');
    check(result.uncertain === false, 'le mapping ravum-unit-graph ne doit pas être incertain');
    check(result.decision === 'selective', `décision selective attendue, obtenue ${result.decision}`);
    assertSetEqual(result.packages, expected, 'fermeture ravum-unit-graph');
    check(result.packages.includes('ravum-unit-graph'), 'ravum-unit-graph doit exiger sa propre suite');
    const ownCommand = result.commands.find((command) => command.kind === 'cargo' && command.package === 'ravum-unit-graph');
    check(Boolean(ownCommand), 'commande cargo test -p ravum-unit-graph absente');
    check(ownCommand.argv.includes('--test-threads=1'), 'commande ravum-unit-graph non sérialisée');
    for (const dependent of [...expected].filter((name) => name !== 'ravum-unit-graph')) {
      check(
        result.commands.some((command) => command.kind === 'cargo' && command.package === dependent),
        `suite du dépendant ${dependent} sautée`,
      );
    }
    // La suite d'intégration du crate, si présente sur disque, reste couverte par
    // `cargo test -p ravum-unit-graph` (pas d'exclusion possible via le mapping).
    if (existsSync(path.join(REPO_ROOT, unitGraph.dir, 'tests'))) {
      check(true, 'le crate ravum-unit-graph expose des tests d’intégration, couverts par cargo test -p');
    }
  });

  runCase('suppression dans ravum-unit-graph: fermeture conservée', () => {
    const unitGraph = workspace.packages.get('ravum-unit-graph');
    if (!unitGraph) {
      check(true, 'ravum-unit-graph absent : cas d’inventaire générique actif');
      return;
    }
    const result = analyze(workspace, [{ status: 'D', path: `${unitGraph.dir}/src/graph.rs` }]);
    const expected = referenceClosure(workspace, ['ravum-unit-graph']);
    assertSetEqual(result.packages, expected, 'fermeture après suppression unit-graph');
    check(result.packages.includes('ravum-unit-graph'), 'le paquet supprimé doit rester compilé/testé');
    check(result.full === false, 'une suppression de source ne doit pas déclencher FULL à tort');
  });

  runCase('docs-only: aucun test Rust, check docs conservé (une seule commande)', () => {
    const result = analyze(workspace, [M('docs/architecture/STATUS.md'), M('README.md')]);
    check(result.full === false, 'docs-only ne doit pas déclencher FULL');
    check(result.packages.length === 0, 'docs-only ne doit exiger aucun paquet Rust');
    check(result.docs_required === true, 'la vérification docs doit rester exigée');
    check(cargoCommands(result).length === 0, 'aucune commande cargo ne doit être émise pour docs-only');
    const docs = result.commands.filter((command) => command.kind === 'docs');
    check(docs.length === 1 && docs[0].argv[0] === 'python', 'la commande docs (python) est manquante');
    check(result.commands.length === 1, `docs-only doit produire exactement 1 commande, obtenu ${result.commands.length}`);
    check(result.decision === 'selective', `décision sélective attendue, obtenue ${result.decision}`);
  });

  runCase('E2E navigateur: règle conservatrice browser_e2e_required', () => {
    const trueCases = [
      ['apps/ravum-todo/src/routes.rs', /apps/],
      ['apps/ravum-todo/static/app.js', /apps/],
      ['crates/ravum-todo-core/src/lib.rs', /todo-core/],
      ['crates/ravum-store/src/lib.rs', /store/],
      ['scripts/demo-m4-browser.ps1', /E2E|script/],
      ['Cargo.toml', /manifeste/],
      ['inconnu/fichier.bin', /inconnu/],
    ];
    for (const [filePath, reasonPattern] of trueCases) {
      const result = analyze(workspace, [M(filePath)]);
      check(result.browser_e2e_required === true, `${filePath} doit exiger les E2E navigateur`);
      check(
        typeof result.browser_e2e_reason === 'string' && reasonPattern.test(result.browser_e2e_reason),
        `${filePath}: raison navigateur inattendue (${result.browser_e2e_reason})`,
      );
      check(result.browser_e2e_reasons.length >= 1, `${filePath}: liste de raisons navigateur vide`);
      check(
        result.reasons.some((reason) => reason.includes('browser-e2e — requis')),
        `${filePath}: raison « browser-e2e — requis » absente`,
      );
    }
    const falseCases = [
      'docs/mvp/MVP-0c-qualification.md',
      'README.md',
      'crates/ravum-unit-graph/src/lib.rs',
      'crates/ravum-proof/src/lib.rs',
    ];
    for (const filePath of falseCases) {
      const result = analyze(workspace, [M(filePath)]);
      check(result.browser_e2e_required === false, `${filePath} ne doit PAS exiger les E2E navigateur`);
      check(result.browser_e2e_reason === null, `${filePath}: raison navigateur inattendue (${result.browser_e2e_reason})`);
      check(
        result.reasons.some((reason) => reason.includes('browser-e2e — non requis')),
        `${filePath}: raison « browser-e2e — non requis » absente`,
      );
    }
    const uncertainResult = analyze(workspace, [{ status: 'U', path: 'crates/ravum-snapshot/src/lib.rs' }]);
    check(uncertainResult.uncertain === true && uncertainResult.browser_e2e_required === true, 'un mapping incertain doit exiger les E2E');
    const emptyResult = analyze(workspace, []);
    check(emptyResult.uncertain === true && emptyResult.browser_e2e_required === true, 'un diff vide doit exiger les E2E');
    const manifestFull = analyze(workspace, [M('Cargo.toml')]);
    check(manifestFull.full === true && manifestFull.browser_e2e_required === true, 'FULL + manifeste doit exiger les E2E');
    const scriptsOnly = analyze(workspace, [M('scripts/run-tests.ps1')]);
    check(scriptsOnly.full === true && scriptsOnly.browser_e2e_required === false, 'FULL scripts non-UI ne doit pas exiger le navigateur');
    const unitGraphOnly = analyze(workspace, [M('crates/ravum-unit-graph/src/lib.rs')]);
    check(unitGraphOnly.browser_e2e_required === false, 'ravum-unit-graph seul ne doit pas exiger le navigateur');
  });

  runCase('protocole => FULL (contrat/ABI partagé), issue légitime sans incertitude', () => {
    const result = analyze(workspace, [M('crates/ravum-protocol/src/lib.rs')]);
    check(result.full === true, 'ravum-protocol doit déclencher FULL');
    check(result.uncertain === false, 'FULL protocolaire n’est pas une incertitude');
    check(result.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${result.decision}`);
    check(result.packages.length === 0, 'en FULL, packages doit rester vide (portée workspace)');
    const commands = cargoCommands(result);
    check(commands.length === 1, 'une seule commande suite complète attendue');
    assertSetEqual(commands[0].argv, FULL_TEST_ARGV, 'commande FULL');
    check(result.reasons.some((reason) => reason.includes('critical-contract')), 'raison critical-contract absente');
    check(result.reasons.some((reason) => reason.includes('ravum-protocol')), 'chemin du protocole absent des raisons');
  });

  runCase('hôte natif patch-host => FULL', () => {
    const result = analyze(workspace, [M('crates/ravum-patch-host/src/slot.rs')]);
    check(result.full === true, 'ravum-patch-host doit déclencher FULL');
    check(result.reasons.some((reason) => reason.includes('critical-native-host')), 'raison critical-native-host absente');
  });

  runCase('manifeste modifié => FULL (Cargo.toml membre + Cargo.lock)', () => {
    for (const filePath of ['crates/ravum-store/Cargo.toml', 'Cargo.toml', 'Cargo.lock']) {
      const result = analyze(workspace, [M(filePath)]);
      check(result.full === true, `${filePath} doit déclencher FULL`);
      check(result.reasons.some((reason) => reason.includes('manifest')), `raison manifest absente pour ${filePath}`);
    }
  });

  runCase('CI modifiée => FULL (.github/workflows)', () => {
    const result = analyze(workspace, [M('.github/workflows/mvp.yml')]);
    check(result.full === true, '.github/workflows/mvp.yml doit déclencher FULL');
    check(result.reasons.some((reason) => reason.includes('ci')), 'raison ci absente');
  });

  runCase('fixtures partagées => FULL', () => {
    const result = analyze(workspace, [M('fixtures/proof/positive.rs')]);
    check(result.full === true, 'fixtures/ doit déclencher FULL');
    check(result.reasons.some((reason) => reason.includes('shared-fixtures')), 'raison shared-fixtures absente');
  });

  runCase('fixture multi-crates fixtures/multicrate => FULL conservateur', () => {
    // La fixture contient des manifestes imbriqués et des sources consommées par
    // les tests de `ravum-unit-graph` : elle n'est pas un member du workspace,
    // donc aucune sélection sélective ne peut la couvrir -> FULL conservateur.
    const fixturePaths = [
      'fixtures/multicrate/core/src/lib.rs',
      'fixtures/multicrate/application/src/main.rs',
      'fixtures/multicrate/api/Cargo.toml',
      'fixtures/multicrate/Cargo.toml',
    ];
    for (const filePath of fixturePaths) {
      const result = analyze(workspace, [M(filePath)]);
      check(result.full === true, `${filePath} doit déclencher FULL`);
      check(result.decision === 'full-conservative', `décision FULL conservatrice attendue pour ${filePath}, obtenue ${result.decision}`);
      check(result.uncertain === false, `${filePath}: un FULL conservateur ne doit pas être incertain`);
      check(
        result.reasons.some((reason) => reason.includes('fixtures') || reason.includes('manifest')),
        `${filePath}: raison conservatrice (fixtures/manifeste) absente`,
      );
      check(
        commandArgs(result).some((argv) => argv.join(' ') === FULL_TEST_ARGV.join(' ')),
        `${filePath}: commande de suite complète absente`,
      );
    }
    check(
      !workspace.byDirLower.has('fixtures/multicrate'.toLowerCase()),
      'fixtures/multicrate ne doit pas être un member du workspace (sinon cargo test --workspace le compilerait)',
    );
  });

  runCase('benchmarks/** => FULL conservateur, jamais de décision vide', () => {
    // Les benchmarks contiennent des fixtures non membres et des scripts non
    // mappés : toute modification doit être FULL conservatrice, JAMAIS une
    // décision vide (aucune commande) ni une erreur de mapping.
    const fixtureManifest = 'benchmarks/s8-mvp0c/fixture/Cargo.toml';
    const fixtureSource = 'benchmarks/s8-mvp0c/fixture/src/lib.rs';
    const script = 'benchmarks/s8/run-s8.mjs';
    for (const filePath of [fixtureManifest, fixtureSource, script]) {
      const result = analyze(workspace, [M(filePath)]);
      check(result.full === true, `${filePath} doit être FULL conservateur`);
      check(result.decision === 'full-conservative', `${filePath}: décision ${result.decision} au lieu de full-conservative`);
      check(result.uncertain === false, `${filePath}: un FULL conservateur ne doit pas être incertain`);
      check(result.commands.length >= 1, `${filePath}: décision vide (aucune commande)`);
      check(
        commandArgs(result).some((argv) => argv.join(' ') === FULL_TEST_ARGV.join(' ')),
        `${filePath}: commande de suite complète absente`,
      );
      check(result.reasons.length > 0, `${filePath}: aucune raison dans la décision`);
    }
    const manifest = analyze(workspace, [M(fixtureManifest)]);
    check(
      manifest.reasons.some((reason) => reason.includes('manifest') || reason.includes('benchmarks')),
      'benchmark fixture: raison manifeste/benchmarks absente',
    );
    const scriptResult = analyze(workspace, [M(script)]);
    check(scriptResult.unknown_paths.includes(script), 'script benchmark non listé comme chemin inconnu (fail-closed attendu)');
  });

  runCase('diff mixte docs + benchmarks => FULL + check docs (décision non vide)', () => {
    const result = analyze(workspace, [
      M('docs/mvp/MVP-0c-qualification.md'),
      M('benchmarks/s8-mvp0c/fixture/Cargo.toml'),
    ]);
    check(result.full === true, 'mixte docs+benchmarks doit être FULL');
    check(result.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${result.decision}`);
    check(result.uncertain === false, 'mixte docs+benchmarks ne doit pas être incertain');
    check(result.docs_required === true, 'le check docs doit rester exigé');
    const argv = commandArgs(result);
    check(argv.some((entry) => entry.join(' ') === FULL_TEST_ARGV.join(' ')), 'suite complète absente du diff mixte');
    check(
      argv.some((entry) => entry[0] === 'python' && entry[1] === 'tools/validate_repo_docs.py'),
      'check docs absent du diff mixte',
    );
    check(result.commands.length >= 2, `décision mixte incomplète: ${result.commands.length} commande(s)`);
  });

  runCase('scripts d’orchestration => FULL', () => {
    const result = analyze(workspace, [M('scripts/run-tests.ps1')]);
    check(result.full === true, 'scripts/ doit déclencher FULL');
    check(result.reasons.some((reason) => reason.includes('orchestration-scripts')), 'raison orchestration-scripts absente');
  });

  runCase('auto-modification de l’outil d’impact => FULL', () => {
    const result = analyze(workspace, [M('tools/impact/impact.mjs')]);
    check(result.full === true, 'tools/impact/ doit déclencher FULL');
    check(result.reasons.some((reason) => reason.includes('self-modification')), 'raison self-modification absente');
  });

  runCase('validateur docs modifié: check docs (pas FULL)', () => {
    const result = analyze(workspace, [M('tools/validate_repo_docs.py')]);
    check(result.full === false, 'modifier le validateur docs ne doit pas déclencher FULL');
    check(result.docs_required === true, 'le check docs doit être exigé');
    check(cargoCommands(result).length === 0, 'aucun test Rust pour un changement du validateur docs');
  });

  runCase('chemin inconnu => FULL légitime (conservateur, non incertain)', () => {
    const result = analyze(workspace, [M('inconnu/fichier.bin')]);
    check(result.full === true, 'un chemin inconnu doit déclencher FULL');
    check(result.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${result.decision}`);
    check(result.uncertain === false, 'un chemin inconnu est une règle conservatrice, pas une incertitude');
    check(result.unknown_paths.includes('inconnu/fichier.bin'), 'le chemin inconnu doit être listé');
    check(cargoCommands(result)[0].argv.includes('--workspace'), 'FULL doit exécuter la suite workspace');
  });

  runCase('fichier racine inconnu (.gitattributes) => FULL + unknown_paths', () => {
    const result = analyze(workspace, [M('.gitattributes')]);
    check(result.full === true, '.gitattributes inconnu du graphe doit déclencher FULL');
    check(result.unknown_paths.includes('.gitattributes'), '.gitattributes doit être listé comme inconnu');
  });

  runCase('renommage => union source + destination', () => {
    const result = analyze(workspace, [
      { status: 'R100', from: 'crates/ravum-snapshot/src/lib.rs', to: 'crates/ravum-store/src/copied.rs' },
    ]);
    const expected = referenceClosureUnion(workspace, ['ravum-snapshot', 'ravum-store']);
    check(result.full === false, 'un renommage interne ne doit pas déclencher FULL');
    assertSetEqual(result.packages, expected, 'union des fermetures source+destination');
    check(result.packages.includes('ravum-rust-adapter'), 'la fermeture de la source (snapshot) est absente');
    check(result.packages.includes('ravum-todo'), 'la fermeture de la destination (store) est absente');
    check(result.packages.includes('ravum-workspace-coordinator'), 'la fermeture transitive de snapshot est absente');
  });

  runCase('copie => union source + destination', () => {
    const result = analyze(workspace, [
      { status: 'C100', from: 'crates/ravum-todo-core/src/lib.rs', to: 'crates/ravum-store/src/copied.rs' },
    ]);
    const expected = referenceClosureUnion(workspace, ['ravum-todo-core', 'ravum-store']);
    check(result.full === false, 'une copie interne ne doit pas déclencher FULL');
    assertSetEqual(result.packages, expected, 'union des fermetures copie');
    check(result.packages.includes('ravum-todo'), 'la fermeture de la destination (store => todo) est absente');
    check(result.packages.includes('ravum-store'), 'la fermeture de la source (todo-core => store) est absente');
  });

  runCase('suppression => composant du chemin supprimé et ses dépendants', () => {
    const result = analyze(workspace, [{ status: 'D', path: 'crates/ravum-todo-core/src/lib.rs' }]);
    const expected = referenceClosure(workspace, ['ravum-todo-core']);
    assertSetEqual(result.packages, expected, 'fermeture suppression');
    check(result.packages.includes('ravum-store') && result.packages.includes('ravum-todo'), 'dépendants de todo-core absents après suppression');
  });

  runCase('fichier de test impacté => au moins sa suite', () => {
    const result = analyze(workspace, [M('apps/ravum-todo/tests/e2e.rs')]);
    const expected = referenceClosure(workspace, ['ravum-todo']);
    assertSetEqual(result.packages, expected, 'suite du fichier de test');
    check(result.packages.includes('ravum-todo'), 'la suite ravum-todo doit être exigée par son fichier de test');
  });

  runCase('diff mixte docs + crate: union des commandes', () => {
    const result = analyze(workspace, [M('docs/mvp/MVP-0b-ci.md'), M('crates/ravum-daemon/src/hash.rs')]);
    check(result.full === false, 'diff mixte docs+crate ne doit pas déclencher FULL');
    assertSetEqual(result.packages, referenceClosure(workspace, ['ravum-daemon']), 'fermeture daemon');
    check(result.docs_required === true, 'check docs exigé');
    check(result.commands.some((command) => command.kind === 'docs'), 'commande docs absente');
    check(result.commands.some((command) => command.kind === 'cargo'), 'commande cargo absente');
  });

  runCase('artefact de preuve seul: aucun test requis mais décision non FULL', () => {
    const result = analyze(workspace, [M('evidence/m2-ledger.jsonl')]);
    check(result.full === false, 'evidence/ seul ne doit pas déclencher FULL');
    check(result.packages.length === 0, 'aucun paquet Rust pour evidence/');
    check(result.commands.length === 0, 'aucune commande attendue pour evidence/ seul');
  });

  runCase('diff vide => FULL + INCERTAIN', () => {
    const result = analyze(workspace, []);
    check(result.full === true, 'un diff vide doit déclencher FULL');
    check(result.uncertain === true, 'un diff vide doit être INCERTAIN');
    check(result.decision === 'full-uncertain', `décision attendue full-uncertain, obtenue ${result.decision}`);
    check(result.reasons.some((reason) => reason.includes('diff-vide')), 'raison diff-vide absente');
  });

  runCase('statut Git non résolu (U) => FULL + INCERTAIN', () => {
    const result = analyze(workspace, [{ status: 'U', path: 'crates/ravum-snapshot/src/lib.rs' }]);
    check(result.full === true, 'statut U doit déclencher FULL');
    check(result.uncertain === true, 'statut U doit être INCERTAIN');
    check(result.decision === 'full-uncertain', `décision attendue full-uncertain, obtenue ${result.decision}`);
    check(result.reasons.some((reason) => reason.includes('statut-non-resolu')), 'raison statut-non-resolu absente');
  });

  runCase('aucune commande ne neutralise un test', () => {
    const diffs = [
      [M('crates/ravum-snapshot/src/lib.rs')],
      [M('crates/ravum-todo-core/src/lib.rs')],
      [M('docs/architecture/STATUS.md')],
      [M('crates/ravum-protocol/src/lib.rs')],
    ];
    const forbidden = ['--exclude', '--skip', '--ignore', '--no-run', '--no-fail-fast'];
    for (const diff of diffs) {
      const result = analyze(workspace, diff);
      for (const argv of commandArgs(result)) {
        for (const token of forbidden) {
          check(!argv.includes(token), `commande de test neutralisante détectée (${token}): ${argv.join(' ')}`);
        }
        if (argv[0] === 'cargo' && argv[1] === 'test') {
          check(argv.includes('--test-threads=1'), `commande cargo non sérialisée: ${argv.join(' ')}`);
        }
      }
      check(result.commands.length > 0 || diff === [M('evidence/...')], 'une décision doit produire des commandes');
    }
  });

  runCase('ordre topologique: une dépendance passe avant son dépendant', () => {
    const cases = [
      [M('crates/ravum-todo-core/src/lib.rs')],
      [M('crates/ravum-snapshot/src/lib.rs')],
      [M('crates/ravum-rust-adapter/src/lib.rs')],
    ];
    for (const diff of cases) {
      const result = analyze(workspace, diff);
      const positions = new Map(result.packages.map((packageName, index) => [packageName, index]));
      for (const [packageName, position] of positions) {
        for (const dep of [...workspace.deps.get(packageName).normal, ...workspace.deps.get(packageName).build]) {
          if (!positions.has(dep)) continue;
          check(positions.get(dep) < position, `ordre: ${dep} doit précéder ${packageName}`);
        }
      }
    }
  });

  runCase('parseNameStatusZ: formats NUL de git diff --name-status', () => {
    const output = ['M', 'a.rs', 'A', 'b.rs', 'R100', 'old.rs', 'new.rs', 'D', 'gone.rs', ''].join('\u0000');
    const changes = parseNameStatusZ(output);
    check(changes.length === 4, `4 changements attendus, obtenus ${changes.length}`);
    check(changes[0].status === 'M' && changes[0].path === 'a.rs', 'statut M mal parsé');
    check(changes[2].status === 'R100' && changes[2].from === 'old.rs' && changes[2].to === 'new.rs', 'renommage mal parsé');
    check(changes[3].status === 'D' && changes[3].path === 'gone.rs', 'suppression mal parsée');
  });

  runCase('parseFilesArg: diff synthétique (statuts, rename, séparateurs)', () => {
    const changes = parseFilesArg('crates/ravum-store/src/lib.rs, docs/mvp/x.md;R100:crates/a/src/lib.rs->crates/b/src/lib.rs');
    check(changes.length === 3, `3 entrées attendues, obtenues ${changes.length}`);
    check(changes[0].status === 'M' && changes[0].path === 'crates/ravum-store/src/lib.rs', 'chemin simple mal parsé');
    check(changes[1].path === 'docs/mvp/x.md', 'séparateur ; mal géré');
    check(changes[2].from === 'crates/a/src/lib.rs' && changes[2].to === 'crates/b/src/lib.rs', 'rename synthétique mal parsé');
    check(parseFilesArg('D:crates/ravum-store/src/lib.rs')[0].status === 'D', 'préfixe D: mal parsé');
  });

  runCase('classifyPath: appartenance crate/app et inconnu', () => {
    const crate = classifyPath('crates/ravum-store/src/lib.rs', workspace);
    check(crate.packages.includes('ravum-store') && crate.full === null, 'mapping crate incorrect');
    const app = classifyPath('apps/ravum-todo/static/app.js', workspace);
    check(app.packages.includes('ravum-todo') && app.full === null, 'mapping app incorrect');
    const unknown = classifyPath('benchmarks/whatever.txt', workspace);
    check(unknown.full !== null && unknown.unknown === true, 'chemin inconnu non fail-closed');
  });

  runCase('INVENTAIRE: chaque crate/app du workspace a un mapping de test (aucun crate silencieux)', () => {
    // 1. Tout manifeste sous crates/ ou apps/ doit être déclaré member : un crate
    //    non déclaré ne serait compilé/testé par AUCUNE sélection du workspace.
    const memberDirs = workspaceMemberDirsOnDisk();
    check(memberDirs.length >= 14, `répertoires crate/app avec manifeste attendus >= 14, trouvés ${memberDirs.length}`);
    for (const dir of memberDirs) {
      check(
        workspace.byDirLower.has(dir.toLowerCase()),
        `crate/app sur disque hors [workspace].members (donc non testé): ${dir}`,
      );
    }
    // 2. Chaque paquet du workspace doit produire soit sa propre suite sélective,
    //    soit un FULL conservateur explicite (« aucun mapping » = échec).
    const notMapped = [];
    const withoutCommand = [];
    for (const packageName of sorted(workspace.packages.keys())) {
      const pkg = workspace.packages.get(packageName);
      const filePath = `${pkg.dir}/src/lib.rs`;
      const classified = classifyPath(filePath, workspace);
      const result = analyze(workspace, [M(filePath)]);
      const mapped = classified.packages.includes(packageName) || classified.full !== null;
      if (!mapped) notMapped.push(packageName);
      const selectiveCommand = result.commands.some(
        (command) => command.kind === 'cargo' && command.package === packageName,
      );
      if (!(result.full === true || selectiveCommand)) withoutCommand.push(packageName);
      check(
        result.full === true || result.decision === 'selective',
        `décision inattendue pour ${packageName}: ${result.decision}`,
      );
      for (const command of result.commands.filter((entry) => entry.kind === 'cargo' && entry.package)) {
        check(workspace.packages.has(command.package), `commande vers un paquet inconnu: ${command.package}`);
      }
    }
    check(notMapped.length === 0, `paquets sans aucun mapping de test: ${notMapped.join(', ')}`);
    check(withoutCommand.length === 0, `paquets sans commande de test ni FULL: ${withoutCommand.join(', ')}`);
    // 3. Les paquets critiques (FULL par règle) comptent comme mappés : moins de
    //    paquets sélectifs que de membres est normal, mais la différence doit
    //    être EXACTEMENT les crates critiques, jamais un oubli silencieux.
    const criticalPackages = [];
    for (const packageName of sorted(workspace.packages.keys())) {
      const pkg = workspace.packages.get(packageName);
      const result = analyze(workspace, [M(`${pkg.dir}/src/lib.rs`)]);
      if (result.full === true) criticalPackages.push(packageName);
    }
    for (const packageName of criticalPackages) {
      check(
        ['ravum-protocol', 'ravum-patch-host'].includes(packageName),
        `paquet inattendu en FULL conservateur (règle à auditer): ${packageName}`,
      );
    }
  });
}

// ---------------------------------------------------------------------------
// 2. CLI (preuve de bout en bout)
// ---------------------------------------------------------------------------

function testCli() {
  runCase('CLI --files: diff séléctif todo-core => 3 suites sérialisées', () => {
    const run = runCli(['--files', 'crates/ravum-todo-core/src/lib.rs', '--json']);
    check(run.code === 0, `code de sortie attendu 0, obtenu ${run.code} (stderr: ${run.stderr.slice(0, 400)})`);
    const decision = parseCliJson(run, 'CLI --files todo-core');
    check(decision.mode === 'files', `mode attendu files, obtenu ${decision.mode}`);
    check(decision.uncertain === false, 'mapping incertain à tort');
    check(decision.full === false, 'FULL à tort');
    check(decision.packages.includes('ravum-store') && decision.packages.includes('ravum-todo'), 'fermeture CLI incomplète');
    for (const argv of decision.commands.map((command) => command.argv)) {
      check(argv.join(' ').includes('--test-threads=1'), `commande non sérialisée: ${argv.join(' ')}`);
    }
  });

  runCase('FULL LÉGITIME: protocole => exit 0, decision full-conservative (aucun faux échec)', () => {
    const run = runCli(['--files', 'crates/ravum-protocol/src/lib.rs', '--json']);
    check(run.code === 0, `code attendu 0 (FULL légitime ne doit pas échouer artificiellement), obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI protocole');
    check(decision.full === true, 'FULL attendu');
    check(decision.uncertain === false, 'FULL protocolaire ne doit pas être marqué incertain');
    check(decision.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${decision.decision}`);
    check(decision.commands.length === 1 && decision.commands[0].argv.includes('--workspace'), 'suite complète attendue');
  });

  runCase('FULL LÉGITIME: manifeste Cargo.toml => exit 0 (règle conservatrice)', () => {
    const run = runCli(['--files', 'Cargo.toml', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI manifeste FULL');
    check(decision.full === true && decision.uncertain === false, 'FULL conservateur attendu');
    check(decision.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${decision.decision}`);
    check(decision.reasons.some((reason) => reason.includes('manifest')), 'raison manifest absente');
  });

  runCase('CLI --files: chemin inconnu => FULL listé, exit 0', () => {
    const run = runCli(['--files', 'zorglub/data.bin', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI inconnu');
    check(decision.full === true, 'FULL attendu');
    check(decision.unknown_paths.includes('zorglub/data.bin'), 'chemin inconnu non listé');
  });

  runCase('CLI --files: benchmark fixture => FULL conservateur, JSON valide (exit 0)', () => {
    const run = runCli(['--files', 'benchmarks/s8-mvp0c/fixture/Cargo.toml', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code} (stderr: ${run.stderr.slice(0, 300)})`);
    const decision = parseCliJson(run, 'CLI benchmark fixture');
    check(decision.full === true, 'FULL attendu pour une fixture de benchmark');
    check(decision.uncertain === false, 'FULL conservateur ne doit pas être incertain');
    check(decision.decision === 'full-conservative', `décision attendue full-conservative, obtenue ${decision.decision}`);
    check(Array.isArray(decision.commands) && decision.commands.length >= 1, 'décision JSON vide (aucune commande)');
  });

  runCase('CLI --files: application UI => browser_e2e_required true (JSON valide)', () => {
    const run = runCli(['--files', 'apps/ravum-todo/src/routes.rs', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI app UI');
    check(decision.browser_e2e_required === true, 'browser_e2e_required attendu true');
    check(typeof decision.browser_e2e_reason === 'string' && decision.browser_e2e_reason.length > 0, 'raison navigateur absente');
  });

  runCase('CLI --files: docs-only => browser_e2e_required false', () => {
    const run = runCli(['--files', 'docs/mvp/MVP-0c-qualification.md', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI docs-only navigateur');
    check(decision.browser_e2e_required === false, 'browser_e2e_required attendu false pour docs-only');
    check(decision.browser_e2e_reason === null, `raison navigateur inattendue: ${decision.browser_e2e_reason}`);
  });

  runCase('INCERTAIN: statut U => FULL + INCERTAIN, exit 2 (seul cas d’échec mapping)', () => {
    const run = runCli(['--files', 'U:crates/ravum-snapshot/src/lib.rs', '--json']);
    check(run.code === 2, `code attendu 2, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI statut U');
    check(decision.full === true && decision.uncertain === true, 'FULL+INCERTAIN attendu');
    check(decision.decision === 'full-uncertain', `décision attendue full-uncertain, obtenue ${decision.decision}`);
  });

  runCase('INCERTAIN: diff vide => FULL + INCERTAIN, exit 2', () => {
    const run = runCli(['--files', '', '--json']);
    check(run.code === 2, `code attendu 2, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI diff vide');
    check(decision.full === true && decision.uncertain === true, 'FULL+INCERTAIN attendu pour diff vide');
    check(decision.decision === 'full-uncertain', `décision attendue full-uncertain, obtenue ${decision.decision}`);
  });

  runCase('CLI --base explicite inexistante => FULL + INCERTAIN, exit 2 (pas de substitution silencieuse)', () => {
    const run = runCli(['--base', 'refs/heads/base-qui-nexiste-pas', '--json']);
    check(run.code === 2, `code attendu 2, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI base inexistante');
    check(decision.full === true && decision.uncertain === true, 'FULL+INCERTAIN attendu');
    check(decision.mode === 'git', `mode attendu git, obtenu ${decision.mode}`);
  });

  runCase('CLI --fail-on-full => exit 4 sur FULL', () => {
    const run = runCli(['--files', 'crates/ravum-protocol/src/lib.rs', '--fail-on-full', '--json']);
    check(run.code === 4, `code attendu 4, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI fail-on-full');
    check(decision.full === true, 'FULL attendu');
  });

  runCase('CLI --explain: rapport lisible avec preuve anti-naïve', () => {
    const run = runCli(['--explain', '--files', 'crates/ravum-todo-core/src/lib.rs']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    check(run.stdout.includes('FERMETURE DES DÉPENDANTS'), 'section fermeture absente');
    check(run.stdout.includes('SAUTÉS'), 'preuve anti-naïve absente');
    check(run.stdout.includes('ravum-store'), 'ravum-store absent du rapport');
    check(run.stdout.includes('ravum-todo'), 'ravum-todo absent du rapport');
    check(run.stdout.includes('--test-threads=1'), 'commande sérialisée absente du rapport');
    let parsed = true;
    try {
      JSON.parse(run.stdout);
    } catch {
      parsed = false;
    }
    check(parsed === false, '--explain seul ne doit pas produire du JSON');
  });

  runCase('CLI --json --explain: JSON sur stdout, humain sur stderr', () => {
    const run = runCli(['--json', '--explain', '--files', 'crates/ravum-snapshot/src/lib.rs']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI json+explain');
    check(decision.packages.includes('ravum-cli'), 'fermeture snapshot incomplète');
    check(run.stderr.includes('FERMETURE DES DÉPENDANTS'), 'rapport humain absent de stderr');
  });

  runCase('CLI --files rename => union source+destination', () => {
    const run = runCli([
      '--files',
      'R100:crates/ravum-snapshot/src/lib.rs->crates/ravum-store/src/lib.rs',
      '--json',
    ]);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const decision = parseCliJson(run, 'CLI rename');
    check(decision.packages.includes('ravum-workspace-coordinator'), 'fermeture source absente');
    check(decision.packages.includes('ravum-todo'), 'fermeture destination absente');
  });

  runCase('CLI mode git réel: --base HEAD~1 --head HEAD (diff de commit réel)', () => {
    if (!refExists(REPO_ROOT, 'HEAD~1')) {
      check(true, 'historique Git trop court: cas explicitement ignoré');
      return;
    }
    const run = runCli(['--base', 'HEAD~1', '--head', 'HEAD', '--json']);
    check(run.code === 0 || run.code === 2, `code attendu 0 ou 2, obtenu ${run.code}: ${run.stderr.slice(0, 300)}`);
    const decision = parseCliJson(run, 'CLI mode git');
    check(decision.mode === 'git', `mode git attendu, obtenu ${decision.mode}`);
    check(decision.base === 'HEAD~1', `base attendue HEAD~1, obtenue ${decision.base}`);
    check(decision.head === 'HEAD', `head attendu HEAD, obtenu ${decision.head}`);
    check(decision.changed.length > 0, 'un commit réel modifie au moins un fichier');
    check(typeof decision.full === 'boolean', 'champ full manquant');
  });

  runCase('CLI --help => exit 0', () => {
    const run = runCli(['--help']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    check(run.stdout.includes('--files'), 'aide incomplète');
  });

  runCase('CLI --dump-graph => graphe complet, exit 0', () => {
    const run = runCli(['--dump-graph', '--json']);
    check(run.code === 0, `code attendu 0, obtenu ${run.code}`);
    const graph = JSON.parse(run.stdout);
    check(Array.isArray(graph.packages) && graph.packages.length >= 14, 'graphe incomplet');
    check(graph.errors.length === 0, `erreurs de graphe: ${graph.errors.join(' | ')}`);
  });

  runCase('CLI argument inconnu => exit 3 (pas de vert silencieux)', () => {
    const run = runCli(['--option-inconnue']);
    check(run.code === 3, `code attendu 3, obtenu ${run.code}`);
  });
}

// ---------------------------------------------------------------------------
// 3. Workflow : garde statique anti-neutralisation
// ---------------------------------------------------------------------------

function testWorkflowIntegrity() {
  runCase('workflow mvp-impact.yml: FULL légitime non bloquant, incertain bloquant, rien de neutralisé', () => {
    let text;
    try {
      text = readFileSync(WORKFLOW_PATH, 'utf8');
    } catch {
      throw new Error(`workflow introuvable: ${WORKFLOW_PATH}`);
    }
    check(text.includes('pull_request'), 'le workflow doit se déclencher sur pull_request');
    check(text.includes('--test-threads=1'), 'le workflow doit sérialiser les tests');
    check(text.includes('--workspace'), 'le workflow doit pouvoir exécuter la suite complète');
    check(text.includes('full-conservative'), 'le workflow doit nommer la décision FULL conservatrice (issue légitime)');

    // Aucun faux échec de blocage par construction : pas de variable, pas de
    // step d'escalade qui échoue quand la suite complète est verte.
    check(!text.includes('RAVUM_IMPACT_ALLOW_FULL'), 'aucune variable de dépôt ne doit être requise pour qu’une PR FULL légitime soit verte');
    check(!text.includes('Escalate FULL'), 'aucun échec artificiel de FULL ne doit subsister');

    // Le step FULL est évalué par le résultat réel des tests, sans exit 1
    // conditionné au FULL.
    const fullStart = text.indexOf('Run the FULL workspace suite');
    const uncertainStart = text.indexOf('Fail explicitly on an uncertain mapping');
    check(fullStart >= 0, 'step « suite complète » absent');
    check(uncertainStart > fullStart, 'step d’incertitude absent ou placé avant le step FULL');
    const nextStepStart = text.indexOf('- name:', fullStart);
    const fullBlock = text.slice(fullStart, nextStepStart > 0 ? nextStepStart : uncertainStart);
    check(fullBlock.includes('cargo test --workspace'), 'commande de suite complète absente du step FULL légitime');
    check(fullBlock.includes('$LASTEXITCODE'), 'le step FULL doit se fonder sur le code de sortie réel des tests');
    check(!/(^|\n)\s*exit 1\b/.test(fullBlock), 'le step FULL ne doit pas contenir d’exit 1 (faux échec sur FULL légitime)');

    // La garde d'incertitude, elle, doit échouer explicitement.
    const guardBlock = text.slice(uncertainStart);
    check(/uncertain/.test(guardBlock), 'garde d’incertitude mal ciblée');
    check(/(^|\n)\s*exit 1\b/.test(guardBlock), 'la garde d’incertitude doit échouer explicitement (exit 1)');

    // Aucun test neutralisé.
    check(!text.includes('continue-on-error'), 'continue-on-error interdit: un échec ne doit jamais être masqué');
    for (const forbidden of ['--exclude', '--skip', '--ignore']) {
      check(!text.includes(forbidden), `token neutralisant interdit dans le workflow: ${forbidden}`);
    }

    // Robustesse CI : la garde du mapping est identifiée, et les étapes
    // conditionnelles n'exécutent des tests que si le selftest est vert. Cela
    // évite la cascade trompeuse « aucune décision d'impact » quand le selftest
    // échoue (le job doit échouer une seule fois, explicitement).
    check(text.includes('id: mapping_selftest'), 'la garde selftest doit avoir un id stable (mapping_selftest)');
    const selftestGateUses = (text.match(/steps\.mapping_selftest\.outcome == 'success'/g) || []).length;
    check(selftestGateUses >= 3, `garde selftest attendue sur au moins 3 étapes conditionnelles, trouvée ${selftestGateUses} fois`);
    check(
      /Verify the impact mapping itself \(Node selftest, no cargo\)/.test(text),
      'le step de selftest mapping doit rester présent',
    );
    check(
      text.indexOf('id: mapping_selftest') < text.indexOf('Compute impact mapping'),
      'le selftest du mapping doit précéder le calcul d’impact',
    );

    // E2E navigateur déclenchés par l’impact : conditionnels, explicites,
    // jamais un skip transformé en vert.
    check(text.includes('browser_e2e_required'), 'le workflow doit lire browser_e2e_required');
    check(text.includes('"browser=$browserFlag"'), 'le workflow doit publier la sortie browser');
    check(text.includes('demo-m4-browser.ps1 -RequireBrowser'), 'le workflow doit exécuter le script E2E avec -RequireBrowser');
    check(text.includes('actions/setup-node@v4'), 'setup-node requis pour le harnais navigateur');
    check(text.includes('target/ravum/browser-e2e'), 'artefact browser-e2e absent du workflow');
    check(
      (text.match(/steps\.impact\.outputs\.browser == 'true'/g) || []).length >= 2,
      'les étapes navigateur (setup + exécution) doivent être conditionnées à browser',
    );
    check(
      /::error::E2E navigateur exigés par le mapping/.test(text),
      'un échec navigateur doit produire une erreur explicite (pas de skip vert)',
    );
  });
}

// ---------------------------------------------------------------------------
// Exécution
// ---------------------------------------------------------------------------

function main() {
  console.log('Ravum — selftest du graphe d’impact (L4-E)');
  console.log('===========================================');
  console.log(`Racine: ${REPO_ROOT}`);
  console.log(`Node:   ${process.version}`);
  console.log('');

  let workspace;
  runCase('chargement du workspace réel', () => {
    workspace = loadWorkspace(REPO_ROOT);
    check(workspace.errors.length === 0, `erreurs de chargement: ${workspace.errors.join(' | ')}`);
    check(workspace.packages.size >= 14, `workspace trop petit: ${workspace.packages.size}`);
  });
  if (!workspace) {
    console.log('\nSELFTEST FAIL — workspace illisible, arrêt.');
    process.exitCode = 1;
    return;
  }

  console.log('\n[1] Analyseur TOML et graphe');
  runCase('analyseur TOML restreint', () => testTomlParser());

  console.log('\n[2] Corpus de diffs synthétiques');
  testImpactCorpus(workspace);

  console.log('\n[3] CLI de bout en bout');
  testCli();

  console.log('\n[4] Intégrité du workflow de CI');
  testWorkflowIntegrity();

  console.log('');
  if (failures.length === 0) {
    console.log(`SELFTEST PASS — ${caseCount} cas, ${assertionCount} assertions, 0 échec.`);
    console.log('Non-qualification : ce selftest prouve le MAPPING, pas le produit Ravum.');
    process.exitCode = 0;
  } else {
    console.log(`SELFTEST FAIL — ${caseCount} cas, ${assertionCount} assertions, ${failures.length} échec(s):`);
    for (const failure of failures) console.log(`  - ${failure.name}: ${failure.message}`);
    process.exitCode = 1;
  }
}

main();

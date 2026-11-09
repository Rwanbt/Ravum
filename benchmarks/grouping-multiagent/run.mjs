#!/usr/bin/env node
//! Harnais L2.2 — fréquence des conflits multi-agents et rentabilité du
//! groupement de patches (MVP-0c).
//!
//! Ce harnais fait deux choses, séparées explicitement :
//!
//! 1. il obtient les décisions **réelles** du binaire `ravum-collab` (coordinateur
//!    COLLAB-1→5) sur quatre familles de scénarios multi-agents (fichiers
//!    distincts, symboles distincts d'un même fichier, même fonction, dépendance
//!    non déclarée), avec ≥ 5 répétitions en ordre alterné, et vérifie les
//!    sorties (décisions attendues, chaîne de snapshots, digests de contenu,
//!    zéro changement perdu) ;
//! 2. il calcule l'économie du groupement dans un **MODÈLE calibré
//!    explicitement étiqueté** : les coûts unitaires (cycle d'analyse 191 ms,
//!    refus de lot 184 ms, reprise ~1596 ms) proviennent de la campagne L2
//!    phase 1 (`evidence/mvp0c-grouping-2026-11-09.json`), pas de cette
//!    campagne-ci. Le modèle ne mesure pas bout en bout le groupement : il
//!    combine les fréquences de conflit **mesurées** avec ces coûts mesurés.
//!
//! Les scénarios (c) et (d) ne sont pas des cas d'échec du harnais : ce sont des
//! situations réelles où le lot groupé contiendrait une proposition non
//! AUTO_SAFE, ce qui rend le lot invalide et déclenche la reprise.
//!
//! Usage :
//!   node benchmarks/grouping-multiagent/run.mjs [--reps 5] [--keep]
//!        [--bin target/debug/ravum-collab.exe]
//!        [--out evidence/mvp0c-grouping-multiagent-2026-11-09.json]
//!
//! Code de sortie : 0 si toutes les décisions sont celles attendues, que les
//! digests correspondent, qu'aucun changement n'est perdu et que le modèle a pu
//! être calculé ; 1 sinon. Aucune commande git n'est exécutée.
//!
//! Aucune dépendance npm : Node 26 et ses modules standard uniquement.

import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const FIXTURES = path.join(HERE, 'fixtures');
const DEFAULT_OUT = path.join(ROOT, 'evidence', 'mvp0c-grouping-multiagent-2026-11-09.json');
const COST_SOURCE = path.join(ROOT, 'evidence', 'mvp0c-grouping-2026-11-09.json');
const DEFAULT_BIN = path.join(ROOT, 'target', 'debug', 'ravum-collab.exe');

// Coûts calibrés de la phase 1, vérifiés dans la preuve source au démarrage.
const EXPECTED_COSTS = { cycleMs: 191, refusalMs: 184, recoveryMs: 1596, retryMs: 158 };
const WINDOWS_MS = [0, 25, 50, 100];

/** Retourne une copie de la liste tournée de `offset` crans (ordre alterné). */
function rotated(list, offset) {
  const n = list.length;
  const start = ((offset % n) + n) % n;
  return [...list.slice(start), ...list.slice(0, start)];
}

function parseArgs(argv) {
  const options = {
    reps: 5,
    keep: false,
    bin: DEFAULT_BIN,
    out: DEFAULT_OUT,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--reps') options.reps = Number(argv[++i]);
    else if (arg === '--keep') options.keep = true;
    else if (arg === '--bin') options.bin = path.resolve(String(argv[++i]));
    else if (arg === '--out') options.out = path.resolve(String(argv[++i]));
    else if (arg === '--help' || arg === '-h') {
      console.log('usage: node benchmarks/grouping-multiagent/run.mjs [--reps N] [--keep] [--bin PATH] [--out PATH]');
      process.exit(0);
    } else throw new Error(`argument inconnu: ${arg}`);
  }
  if (!Number.isInteger(options.reps) || options.reps < 5) {
    throw new Error('--reps doit être un entier >= 5 (mandat L2.2)');
  }
  return options;
}

function sha256Hex(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

function nowUnix() {
  return Math.floor(Date.now() / 1000);
}

/**
 * Provenance Git en lecture seule (aucune commande mutante) : la campagne tourne
 * dans un arbre partagé avec les autres workers MVP-0c, le nombre de fichiers
 * dirty est donc attendu et n'entre pas dans l'identité de la campagne.
 */
function gitProvenance() {
  const head = spawnCapture('git', ['rev-parse', 'HEAD']);
  const branch = spawnCapture('git', ['branch', '--show-current']);
  const status = spawnCapture('git', ['status', '--porcelain']);
  return {
    head: head.status === 0 ? head.stdout.trim() : null,
    branch: branch.status === 0 ? branch.stdout.trim() : null,
    dirtyFiles: status.status === 0 ? status.stdout.split('\n').filter(Boolean).length : null,
    note: "arbre de travail partagé avec les autres workers MVP-0c: un nombre de fichiers dirty non nul est attendu et n'entre pas dans l'identité de la campagne",
  };
}

/** Liste récursivement les fichiers d'un répertoire en chemins relatifs '/'. */
function listFilesRecursive(root, prefix = '') {
  const out = [];
  const entries = fs.readdirSync(path.join(root, prefix), { withFileTypes: true });
  for (const entry of entries) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...listFilesRecursive(root, relative));
    else if (entry.isFile()) out.push(relative);
  }
  out.sort();
  return out;
}

/** Lit un workspace en `[{path, content}]` trié, comme `SourceSnapshot`. */
function readWorkspace(dir) {
  return listFilesRecursive(dir).map((relative) => ({
    path: relative,
    content: fs.readFileSync(path.join(dir, relative), 'utf8'),
  }));
}

/**
 * Digest de snapshot identique à `ravum_snapshot::SourceSnapshot::digest` :
 * `ravum-snapshot-v1\n` puis, par chemin trié, `path\0len\0content\0`.
 * Vérifié au démarrage contre l'identifiant réel imprimé par `ravum-collab base`.
 */
function snapshotDigest(files) {
  const sorted = [...files].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const chunks = [Buffer.from('ravum-snapshot-v1\n', 'utf8')];
  for (const file of sorted) {
    chunks.push(Buffer.from(file.path, 'utf8'));
    chunks.push(Buffer.from([0]));
    chunks.push(Buffer.from(String(Buffer.byteLength(file.content, 'utf8')), 'utf8'));
    chunks.push(Buffer.from([0]));
    chunks.push(Buffer.from(file.content, 'utf8'));
    chunks.push(Buffer.from([0]));
  }
  return sha256Hex(Buffer.concat(chunks));
}

/**
 * Construit un hunk unified-diff exact qui remplace une ligne unique, avec une
 * ligne de contexte de chaque côté (même stratégie que le scénario intégré de
 * `collab_cycle.rs`). Échoue si la ligne cible n'est pas unique.
 */
function makePatch(relPath, content, find, replace) {
  const occurrences = content.split(find).length - 1;
  if (occurrences !== 1) {
    throw new Error(`édition ${relPath}: le texte ${JSON.stringify(find)} apparaît ${occurrences} fois (attendu 1)`);
  }
  let lines = content.split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') lines = lines.slice(0, -1);
  const indexes = [];
  for (let i = 0; i < lines.length; i += 1) if (lines[i] === find) indexes.push(i);
  if (indexes.length !== 1) {
    throw new Error(`édition ${relPath}: la ligne ${JSON.stringify(find)} n'est pas une ligne unique (${indexes.length})`);
  }
  const index = indexes[0];
  const first = Math.max(index - 1, 0);
  const last = Math.min(index + 1, lines.length - 1);
  const hunk = [];
  let oldLen = 0;
  let newLen = 0;
  for (let i = first; i < index; i += 1) {
    hunk.push(` ${lines[i]}`);
    oldLen += 1;
    newLen += 1;
  }
  hunk.push(`-${lines[index]}`);
  oldLen += 1;
  hunk.push(`+${replace}`);
  newLen += 1;
  for (let i = index + 1; i <= last; i += 1) {
    hunk.push(` ${lines[i]}`);
    oldLen += 1;
    newLen += 1;
  }
  return `+++ b/${relPath}\n@@ -${first + 1},${oldLen} +${first + 1},${newLen} @@\n${hunk.join('\n')}\n`;
}

/** Applique une édition unique `find` -> `replace` sur le contenu d'un fichier. */
function applyEdit(files, edit) {
  const target = files.find((file) => file.path === edit.path);
  if (!target) throw new Error(`fichier cible introuvable dans le snapshot attendu: ${edit.path}`);
  const occurrences = target.content.split(edit.find).length - 1;
  if (occurrences !== 1) {
    throw new Error(`édition ${edit.path}/${edit.agent}: ${occurrences} occurrence(s) de ${JSON.stringify(edit.find)}`);
  }
  target.content = target.content.replace(edit.find, edit.replace);
}

function spawnCapture(command, args, options = {}) {
  const started = process.hrtime.bigint();
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    encoding: 'utf8',
    timeout: options.timeout ?? 180_000,
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  const wallMs = Number((process.hrtime.bigint() - started) / 1000000n);
  return {
    status: result.status,
    signal: result.signal,
    error: result.error ? String(result.error.message ?? result.error) : null,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
    wallMs,
  };
}

function rustcCalibration(samples = 3) {
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    const wall = spawnCapture('rustc', ['-vV'], { timeout: 30_000 }).wallMs;
    out.push(wall);
  }
  return out;
}

function statsOf(values) {
  if (values.length === 0) return { count: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const percentile = (p) => {
    const rank = Math.ceil((p / 100) * sorted.length);
    return sorted[Math.min(Math.max(rank - 1, 0), sorted.length - 1)];
  };
  const sum = sorted.reduce((acc, value) => acc + value, 0);
  return {
    count: sorted.length,
    min: sorted[0],
    max: sorted[sorted.length - 1],
    mean: sum / sorted.length,
    p50: percentile(50),
    p95: percentile(95),
  };
}

// ---------------------------------------------------------------------------
// Scénarios multi-agents (les patches réels sont construits depuis la base)
// ---------------------------------------------------------------------------

const SCENARIOS = [
  {
    id: 'a-distinct-files',
    label: 'fichiers distincts',
    n: 3,
    fixture: 'scenario-a',
    proposals: [
      { agent: 'agent-a1', task: 'alpha', path: 'src/alpha.rs', symbols: ['alpha'], find: '    x + 1', replace: '    x + 2' },
      { agent: 'agent-a2', task: 'beta', path: 'src/beta.rs', symbols: ['beta'], find: '    x * 2', replace: '    x * 3' },
      { agent: 'agent-a3', task: 'gamma', path: 'src/gamma.rs', symbols: ['gamma'], find: '    x - 3', replace: '    x - 4' },
    ],
    expectedDecisions: ['AUTO_SAFE', 'AUTO_SAFE', 'AUTO_SAFE'],
  },
  {
    id: 'b-distinct-symbols-one-file',
    label: 'symboles distincts, même fichier',
    n: 5,
    fixture: 'scenario-b',
    proposals: [
      { agent: 'agent-b1', task: 'f1', path: 'src/lib.rs', symbols: ['f1'], find: '    x + 1', replace: '    x + 2' },
      { agent: 'agent-b2', task: 'f2', path: 'src/lib.rs', symbols: ['f2'], find: '    x * 2', replace: '    x * 3' },
      { agent: 'agent-b3', task: 'f3', path: 'src/lib.rs', symbols: ['f3'], find: '    x + 3', replace: '    x + 4' },
      { agent: 'agent-b4', task: 'f4', path: 'src/lib.rs', symbols: ['f4'], find: '    x - 4', replace: '    x - 5' },
      { agent: 'agent-b5', task: 'f5', path: 'src/lib.rs', symbols: ['f5'], find: '    x + 5', replace: '    x + 6' },
    ],
    expectedDecisions: ['AUTO_SAFE', 'AUTO_SAFE', 'AUTO_SAFE', 'AUTO_SAFE', 'AUTO_SAFE'],
  },
  {
    id: 'c-same-function',
    label: 'même fonction (conflit)',
    n: 3,
    fixture: 'scenario-c',
    proposals: [
      { agent: 'agent-c1', task: 'shared-plus-2', path: 'src/lib.rs', symbols: ['shared'], find: '    x + 1', replace: '    x + 2' },
      { agent: 'agent-c2', task: 'shared-plus-3', path: 'src/lib.rs', symbols: ['shared'], find: '    x + 1', replace: '    x + 3' },
      { agent: 'agent-c3', task: 'shared-plus-4', path: 'src/lib.rs', symbols: ['shared'], find: '    x + 1', replace: '    x + 4' },
    ],
    expectedDecisions: ['AUTO_SAFE', 'CONFLICT', 'CONFLICT'],
  },
  {
    id: 'd-undeclared-dependency',
    label: 'dépendance non déclarée',
    n: 3,
    fixture: 'scenario-d',
    proposals: [
      { agent: 'agent-d1', task: 'helper', path: 'src/lib.rs', symbols: ['helper'], find: '    x + 1', replace: '    x + 5' },
      {
        agent: 'agent-d2',
        task: 'compose-undeclared',
        path: 'src/lib.rs',
        symbols: ['compose'],
        find: '    helper(x) * 2',
        replace: '    helper(x) * 3',
        declaredDependencies: [],
      },
      { agent: 'agent-d3', task: 'other', path: 'src/lib.rs', symbols: ['other'], find: '    x - 1', replace: '    x - 2' },
    ],
    expectedDecisions: ['AUTO_SAFE', 'MANUAL_REQUIRED', 'AUTO_SAFE'],
  },
];

function proposalId(proposal) {
  return `${proposal.agent}/${proposal.task}`;
}

// ---------------------------------------------------------------------------
// Exécution d'un scénario par le binaire réel
// ---------------------------------------------------------------------------

function runScenarioOnce({ scenario, rep, runRoot, options, costs }) {
  const workspace = path.join(runRoot, 'workspace');
  const proposalsDir = path.join(runRoot, 'proposals');
  fs.rmSync(runRoot, { recursive: true, force: true });
  fs.mkdirSync(proposalsDir, { recursive: true });
  fs.cpSync(path.join(FIXTURES, scenario.fixture), workspace, { recursive: true });

  const baseRun = spawnCapture(options.bin, ['base', '--workspace', workspace]);
  if (baseRun.status !== 0) {
    throw new Error(`ravum-collab base a échoué (${baseRun.status}): ${baseRun.stderr}`);
  }
  const basePayload = JSON.parse(baseRun.stdout);
  const fixtureFiles = readWorkspace(workspace);
  const computedBaseDigest = snapshotDigest(fixtureFiles);
  const baseDigestMatches = computedBaseDigest === String(basePayload.snapshot_id);

  const contentByPath = new Map(fixtureFiles.map((file) => [file.path, file.content]));
  const proposalRecords = scenario.proposals.map((proposal, index) => {
    const content = contentByPath.get(proposal.path);
    if (content === undefined) throw new Error(`chemin de proposition inconnu: ${proposal.path}`);
    const patch = makePatch(proposal.path, content, proposal.find, proposal.replace);
    const body = {
      schema_version: 1,
      agent_id: proposal.agent,
      task_id: proposal.task,
      base_snapshot_id: basePayload.snapshot_id,
      target_paths: [proposal.path],
      target_symbols: proposal.symbols,
      expected_hashes: { [proposal.path]: basePayload.files[proposal.path] },
      declared_dependencies: proposal.declaredDependencies ?? [],
      impacted_contracts: [],
      unified_patch: patch,
      note: `scénario ${scenario.id}, agent ${proposal.agent}`,
    };
    return { proposal, index, body };
  });
  for (const record of proposalRecords) {
    const fileName = String(record.index + 1).padStart(2, '0') + `-${record.proposal.agent}-${record.proposal.task}.json`;
    fs.writeFileSync(path.join(proposalsDir, fileName), JSON.stringify(record.body, null, 2));
  }

  const journal = path.join(runRoot, 'journal.jsonl');
  const work = path.join(runRoot, 'verify-work');
  const loadBefore = { freeMemoryBytes: os.freemem(), totalMemoryBytes: os.totalmem(), processCount: null };
  const integrate = spawnCapture(options.bin, [
    'integrate',
    '--workspace',
    workspace,
    '--proposals',
    proposalsDir,
    '--journal',
    journal,
    '--json',
    '--verify',
    '--work',
    work,
  ]);
  const loadAfter = { freeMemoryBytes: os.freemem(), totalMemoryBytes: os.totalmem(), processCount: null };

  let payload = null;
  let parseError = null;
  try {
    payload = JSON.parse(integrate.stdout);
  } catch (error) {
    parseError = String(error.message ?? error);
  }

  const observation = {
    rep,
    scenario: scenario.id,
    scenarioLabel: scenario.label,
    n: scenario.n,
    wallMs: integrate.wallMs,
    spawnExit: integrate.status,
    spawnSignal: integrate.signal,
    spawnError: integrate.error,
    stderr: integrate.stderr.slice(0, 4000),
    parseError,
    baseDigestMatches,
    baseDigest: computedBaseDigest,
    baseSnapshotId: String(basePayload.snapshot_id),
    journalLines: fs.existsSync(journal) ? fs.readFileSync(journal, 'utf8').split('\n').filter(Boolean).length : 0,
    loadBefore,
    loadAfter,
    decisions: [],
    acceptedChanges: [],
    checks: {},
  };

  if (!payload) {
    observation.checks = { decisionsExpected: false, chainOk: false, finalDigestMatches: false, noLostChanges: false };
    return observation;
  }

  const decisions = payload.decisions ?? [];
  observation.decisions = decisions.map((decision) => ({
    decision: decision.decision,
    proposalId: String(decision.proposal_id),
    agentId: decision.agent_id,
    taskId: decision.task_id,
    reasons: decision.reasons ?? [],
    integratedPaths: decision.integrated_paths ?? [],
    resultingSnapshotId: decision.resulting_snapshot_id ?? null,
  }));
  observation.acceptedChanges = (payload.accepted_changes ?? []).map((change) => ({
    proposalId: String(change.proposal_id),
    agentId: change.agent_id,
    snapshotBefore: String(change.snapshot_before),
    snapshotAfter: String(change.snapshot_after),
    journalSeq: change.journal_seq,
    paths: change.paths ?? [],
    verificationStatus: change.verification?.status ?? 'unknown',
    evidenceId: change.verification?.evidence_id ?? null,
  }));
  observation.protocol = payload.protocol;
  observation.verificationMode = payload.verification?.mode ?? null;
  observation.snapshotBefore = String(payload.snapshot_before);
  observation.snapshotAfter = String(payload.snapshot_after);
  observation.summary = payload.summary;

  // 1. Décisions réelles == attendues ?
  const actualDecisions = observation.decisions.map((decision) => decision.decision);
  const decisionsExpected =
    JSON.stringify(actualDecisions) === JSON.stringify(scenario.expectedDecisions);

  // 2. Chaîne des snapshots acceptés: before(i) = after(i-1), premier = base, dernier = snapshot_after.
  const accepted = observation.acceptedChanges;
  const autoSafeCount = actualDecisions.filter((decision) => decision === 'AUTO_SAFE').length;
  let chainOk = accepted.length === autoSafeCount;
  const acceptedIds = accepted.map((change) => change.proposalId);
  const autoSafeIds = observation.decisions
    .filter((decision) => decision.decision === 'AUTO_SAFE')
    .map((decision) => decision.proposalId);
  chainOk = chainOk && JSON.stringify(acceptedIds) === JSON.stringify(autoSafeIds);
  for (let i = 0; i < accepted.length && chainOk; i += 1) {
    const expectedBefore = i === 0 ? observation.baseSnapshotId : accepted[i - 1].snapshotAfter;
    if (accepted[i].snapshotBefore !== expectedBefore) chainOk = false;
  }
  if (chainOk) {
    const lastAfter = accepted.length > 0 ? accepted[accepted.length - 1].snapshotAfter : observation.baseSnapshotId;
    if (lastAfter !== observation.snapshotAfter) chainOk = false;
  }

  // 3. Contenu final attendu: base + éditions des seules propositions AUTO_SAFE,
  //    appliquées dans l'ordre des décisions (le coordinateur intègre dans cet ordre).
  const expectedFiles = fixtureFiles.map((file) => ({ path: file.path, content: file.content }));
  const applied = [];
  for (const decision of observation.decisions) {
    if (decision.decision !== 'AUTO_SAFE') continue;
    const record = scenario.proposals.find((proposal) => proposalId(proposal) === decision.proposalId);
    if (!record) throw new Error(`proposition AUTO_SAFE inconnue: ${decision.proposalId}`);
    applyEdit(expectedFiles, record);
    applied.push(record);
  }
  const expectedFinalDigest = snapshotDigest(expectedFiles);
  const finalDigestMatches = expectedFinalDigest === observation.snapshotAfter;

  // 4. Aucun changement perdu: chaque édition acceptée est présente dans le
  //    contenu final attendu; le marqueur d'une édition refusée est absent
  //    (elle n'a pas été appliquée, y compris quand un autre agent a modifié la
  //    même ligne — c'est le conflit attendu, pas une perte).
  const finalByPath = new Map(expectedFiles.map((file) => [file.path, file.content]));
  let noLostChanges = true;
  const lostDetails = [];
  for (const decision of observation.decisions) {
    const record = scenario.proposals.find((proposal) => proposalId(proposal) === decision.proposalId);
    if (!record) continue;
    const finalContent = finalByPath.get(record.path) ?? '';
    if (decision.decision === 'AUTO_SAFE') {
      if (!finalContent.includes(record.replace)) {
        noLostChanges = false;
        lostDetails.push(`édition acceptée absente du contenu final: ${decision.proposalId}`);
      }
    } else if (record.find !== record.replace && finalContent.includes(record.replace)) {
      noLostChanges = false;
      lostDetails.push(`édition refusée appliquée malgré tout: ${decision.proposalId}`);
    }
  }

  observation.checks = {
    decisionsExpected,
    chainOk,
    finalDigestMatches,
    noLostChanges,
    expectedFinalDigest,
    acceptedCount: accepted.length,
    autoSafeCount,
  };
  observation.lostDetails = lostDetails;
  return observation;
}

// ---------------------------------------------------------------------------
// Harnais principal
// ---------------------------------------------------------------------------

function main() {
  const options = parseArgs(process.argv.slice(2));
  const campaignStarted = process.hrtime.bigint();
  if (!fs.existsSync(options.bin)) {
    throw new Error(
      `binaire ravum-collab introuvable: ${options.bin}; construire une fois avec "cargo build -p ravum-workspace-coordinator --bins"`,
    );
  }
  if (!fs.existsSync(COST_SOURCE)) {
    throw new Error(`preuve de coûts phase 1 introuvable: ${COST_SOURCE}`);
  }
  const phase1 = JSON.parse(fs.readFileSync(COST_SOURCE, 'utf8'));
  const costs = {
    cycleMs: Number(phase1.summary?.break_even?.per_cycle_analysis_p50_ms),
    refusalMs: Number(phase1.summary?.incompatible?.batch_latency_ms?.p50),
    recoveryMs: Number(phase1.summary?.incompatible?.recovery_total_ms?.p50),
    retryMs: Number(phase1.summary?.incompatible?.retry_ms?.p50),
  };
  for (const [key, expected] of Object.entries(EXPECTED_COSTS)) {
    if (!Number.isFinite(costs[key])) throw new Error(`coût ${key} absent de ${COST_SOURCE}`);
    if (costs[key] !== expected) {
      throw new Error(`coût ${key}=${costs[key]} inattendu (phase 1 attendu ${expected}) — recalibrer explicitement`);
    }
  }

  const workRoot = path.join(ROOT, 'target', 'grouping-multiagent');
  fs.rmSync(workRoot, { recursive: true, force: true });
  fs.mkdirSync(workRoot, { recursive: true });

  const calibrationStart = rustcCalibration(3);
  const loadStart = {
    freeMemoryBytes: os.freemem(),
    totalMemoryBytes: os.totalmem(),
  };
  console.log(
    `grouping-multiagent: ${SCENARIOS.length} scénarios x ${options.reps} répétitions (séquentiel et ordre alterné)`,
  );

  const repetitions = [];
  for (let rep = 1; rep <= options.reps; rep += 1) {
    const order = rotated(SCENARIOS, rep - 1);
    const repEntry = {
      rep,
      order: order.map((scenario) => scenario.id),
      loadBefore: { freeMemoryBytes: os.freemem(), totalMemoryBytes: os.totalmem() },
      runs: [],
    };
    for (const scenario of order) {
      const runRoot = path.join(workRoot, `rep-${String(rep).padStart(2, '0')}`, scenario.id);
      const observation = runScenarioOnce({ scenario, rep, runRoot, options, costs });
      repEntry.runs.push(observation);
      const ok =
        observation.checks.decisionsExpected &&
        observation.checks.chainOk &&
        observation.checks.finalDigestMatches &&
        observation.checks.noLostChanges &&
        observation.baseDigestMatches;
      console.log(
        `  rep ${rep}/${options.reps} ${scenario.id.padEnd(30)} décisions=${observation.decisions
          .map((decision) => decision.decision)
          .join(',')} ${ok ? 'OK' : 'ECHEC'} (${observation.wallMs} ms)`,
      );
    }
    repEntry.loadAfter = { freeMemoryBytes: os.freemem(), totalMemoryBytes: os.totalmem() };
    repetitions.push(repEntry);
  }
  const calibrationEnd = rustcCalibration(3);
  const loadEnd = { freeMemoryBytes: os.freemem(), totalMemoryBytes: os.totalmem() };
  const durationMs = Number((process.hrtime.bigint() - campaignStarted) / 1000000n);

  // -------------------------------------------------------------------------
  // Agrégation: déterminisme, décisions, fréquences de conflit mesurées
  // -------------------------------------------------------------------------
  const perScenario = new Map();
  for (const scenario of SCENARIOS) {
    perScenario.set(scenario.id, {
      id: scenario.id,
      label: scenario.label,
      n: scenario.n,
      expectedDecisions: scenario.expectedDecisions,
      runs: 0,
      runsWithExpectedDecisions: 0,
      runsWithConflict: 0,
      nonAutoSafeDecisions: 0,
      totalDecisions: 0,
      runsAllChecksOk: 0,
      wallMs: [],
      decisionsByRun: [],
    });
  }
  const allRuns = [];
  for (const repetition of repetitions) {
    for (const run of repetition.runs) {
      allRuns.push(run);
      const entry = perScenario.get(run.scenario);
      entry.runs += 1;
      const decisions = run.decisions.map((decision) => decision.decision);
      entry.decisionsByRun.push(decisions);
      if (run.checks.decisionsExpected) entry.runsWithExpectedDecisions += 1;
      if (decisions.some((decision) => decision !== 'AUTO_SAFE')) entry.runsWithConflict += 1;
      entry.nonAutoSafeDecisions += decisions.filter((decision) => decision !== 'AUTO_SAFE').length;
      entry.totalDecisions += decisions.length;
      const ok =
        run.checks.decisionsExpected &&
        run.checks.chainOk &&
        run.checks.finalDigestMatches &&
        run.checks.noLostChanges &&
        run.baseDigestMatches &&
        run.spawnExit === 0;
      if (ok) entry.runsAllChecksOk += 1;
      entry.wallMs.push(run.wallMs);
    }
  }

  const scenarioAggregates = [...perScenario.values()].map((entry) => {
    const deterministic = entry.decisionsByRun.every(
      (decisions) => JSON.stringify(decisions) === JSON.stringify(entry.decisionsByRun[0]),
    );
    return {
      id: entry.id,
      label: entry.label,
      n: entry.n,
      expectedDecisions: entry.expectedDecisions,
      runs: entry.runs,
      decisions: entry.decisionsByRun[0] ?? [],
      deterministicAcrossReps: deterministic,
      runsWithExpectedDecisions: entry.runsWithExpectedDecisions,
      runsWithConflict: entry.runsWithConflict,
      batchConflictRate: entry.runsWithConflict / entry.runs,
      proposalConflictRate: entry.nonAutoSafeDecisions / Math.max(entry.totalDecisions, 1),
      nonAutoSafeDecisions: entry.nonAutoSafeDecisions,
      totalDecisions: entry.totalDecisions,
      runsAllChecksOk: entry.runsAllChecksOk,
      wallMs: statsOf(entry.wallMs),
    };
  });

  const totalRuns = allRuns.length;
  const conflictingRuns = allRuns.filter((run) => run.decisions.some((decision) => decision.decision !== 'AUTO_SAFE')).length;
  const overall = {
    runs: totalRuns,
    runsAllChecksOk: allRuns.filter(
      (run) =>
        run.checks.decisionsExpected &&
        run.checks.chainOk &&
        run.checks.finalDigestMatches &&
        run.checks.noLostChanges &&
        run.baseDigestMatches &&
        run.spawnExit === 0,
    ).length,
    batchConflictRuns: conflictingRuns,
    batchConflictRate: conflictingRuns / Math.max(totalRuns, 1),
    proposalConflictRate:
      allRuns.reduce(
        (acc, run) => acc + run.decisions.filter((decision) => decision.decision !== 'AUTO_SAFE').length,
        0,
      ) / Math.max(allRuns.reduce((acc, run) => acc + run.decisions.length, 0), 1),
    wallMs: statsOf(allRuns.map((run) => run.wallMs)),
  };

  // -------------------------------------------------------------------------
  // MODÈLE d'économie calibré (explicitement étiqueté MODÈLE)
  // -------------------------------------------------------------------------
  const C = costs.cycleMs;
  const R = costs.recoveryMs;
  const sequentialCost = (n) => n * C;
  const groupedCost = (p, n, windowMs) => windowMs + (1 - p) * C + p * R;
  const breakEvenConflictRate = (n, windowMs) => ((n - 1) * C - windowMs) / (R - C);
  const expectedGroupedAnalyses = (p, n) => (1 - p) * 1 + p * (n + 3);
  const expectedAnalysesSaved = (p, n) => n - expectedGroupedAnalyses(p, n);

  const breakEvenTable = [];
  for (const n of [3, 4, 5, 6]) {
    for (const windowMs of WINDOWS_MS) {
      breakEvenTable.push({
        n,
        windowMs,
        sequentialCostMs: sequentialCost(n),
        groupedCostAllCompatibleMs: windowMs + C,
        groupedCostRecoveryMs: windowMs + R,
        breakEvenBatchConflictRate: breakEvenConflictRate(n, windowMs),
      });
    }
  }

  const scenarioEconomics = scenarioAggregates.map((entry) => {
    const p = entry.batchConflictRate;
    const windows = WINDOWS_MS.map((windowMs) => {
      const grouped = groupedCost(p, entry.n, windowMs);
      const sequential = sequentialCost(entry.n);
      return {
        windowMs,
        sequentialCostMs: sequential,
        groupedCostMs: grouped,
        gainMs: sequential - grouped,
        gainPercent: sequential > 0 ? (100 * (sequential - grouped)) / sequential : 0,
        profitable: grouped < sequential,
      };
    });
    return {
      scenario: entry.id,
      n: entry.n,
      measuredBatchConflictRate: p,
      measuredProposalConflictRate: entry.proposalConflictRate,
      breakEvenBatchConflictRateAtWindow0: breakEvenConflictRate(entry.n, 0),
      expectedAnalysesSequential: entry.n,
      expectedAnalysesGrouped: expectedGroupedAnalyses(p, entry.n),
      expectedAnalysesSaved: expectedAnalysesSaved(p, entry.n),
      windows,
    };
  });

  // Mix global mesuré: coût total attendu sur l'ensemble des lots observés, en
  // appliquant à chaque lot son N et son taux de conflit mesuré (0 ou 1 ici).
  const mixEconomics = WINDOWS_MS.map((windowMs) => {
    let sequentialTotal = 0;
    let groupedTotal = 0;
    const details = scenarioAggregates.map((entry) => {
      const sequential = entry.n * C;
      const grouped =
        windowMs + (1 - entry.batchConflictRate) * C + entry.batchConflictRate * R;
      sequentialTotal += sequential;
      groupedTotal += grouped;
      return { scenario: entry.id, n: entry.n, p: entry.batchConflictRate, sequential, grouped };
    });
    return {
      windowMs,
      sequentialCostMsPerMix: sequentialTotal,
      groupedCostMsPerMix: groupedTotal,
      gainMs: sequentialTotal - groupedTotal,
      gainPercent: sequentialTotal > 0 ? (100 * (sequentialTotal - groupedTotal)) / sequentialTotal : 0,
      profitable: groupedTotal < sequentialTotal,
      details,
    };
  });

  const model = {
    label: 'MODÈLE CALIBRÉ — pas une mesure bout en bout du groupement',
    disclaimer:
      "Les coûts unitaires proviennent de la campagne L2 phase 1 (analyse séquentielle réelle de 6 éditions, lot refusé réel, reprise réelle), pas de cette campagne. Les fréquences de conflit proviennent des décisions réelles du binaire ravum-collab mesurées ici. Le modèle suppose: coût d'un lot compatible = 1 cycle; coût d'un lot contenant au moins une proposition non AUTO_SAFE = coût de reprise mesuré (refus + reprise individuelle + re-lot, N+3 analyses à N=6); fenêtre d'attente purement latente; pas d'extrapolation hors N=3..6 et fenêtres 0..100 ms.",
    costs: {
      cycleMs: C,
      refusalMs: costs.refusalMs,
      recoveryMs: R,
      retryMs: costs.retryMs,
      source: path.relative(ROOT, COST_SOURCE),
    },
    formulas: {
      sequential: 'E_séquentiel(N) = N × cycle',
      grouped: 'E_groupé(p, N, w) = w + (1−p) × cycle + p × reprise',
      breakEven: 'p*(N, w) = ((N−1) × cycle − w) / (reprise − cycle)',
      analyses: 'E_analyses_groupé(p, N) = (1−p) × 1 + p × (N+3)',
    },
    breakEvenTable,
    scenarioEconomics,
    mixEconomics,
    verdict: null,
  };

  // Verdict honnête, dérivé des chiffres (pas d'embellissement).
  const compatibleScenarios = scenarioEconomics.filter((entry) => entry.measuredBatchConflictRate === 0);
  const conflictingScenarios = scenarioEconomics.filter((entry) => entry.measuredBatchConflictRate > 0);
  const mixWindow50 = mixEconomics.find((entry) => entry.windowMs === 50);
  const breakEvenAtN = (n, windowMs) => breakEvenConflictRate(n, windowMs);
  model.verdict = {
    compatibleScenariosProfitable: compatibleScenarios.every((entry) =>
      entry.windows.every((window) => window.profitable),
    ),
    conflictingScenariosProfitable: conflictingScenarios.some((entry) =>
      entry.windows.some((window) => window.profitable),
    ),
    measuredMixProfitableAtWindow50: mixWindow50.profitable,
    measuredMixGainPercentAtWindow50: mixWindow50.gainPercent,
    breakEvenBatchConflictRate: {
      n3_window0: breakEvenAtN(3, 0),
      n3_window100: breakEvenAtN(3, 100),
      n4_window0: breakEvenAtN(4, 0),
      n4_window100: breakEvenAtN(4, 100),
      n5_window0: breakEvenAtN(5, 0),
      n5_window100: breakEvenAtN(5, 100),
      n6_window0: breakEvenAtN(6, 0),
      n6_window100: breakEvenAtN(6, 100),
    },
    statement:
      'Le groupement n\'est rentable que si la part des lots contenant au moins une proposition non AUTO_SAFE reste sous le seuil p*(N, w). Avec le mélange de scénarios mesuré (deux familles sur quatre sont conflictuelles par construction), la fréquence observée est de ' +
      `${(overall.batchConflictRate * 100).toFixed(1)} % : ` +
      (mixWindow50.profitable ? 'le groupement reste rentable en moyenne.' : 'le groupement n\'est PAS rentable en moyenne.') +
      ' Ces scénarios sont volontairement chargés en conflits : la fréquence réelle en production dépend des agents et n\'est pas mesurée ici.',
  };

  // -------------------------------------------------------------------------
  // Validité et preuve
  // -------------------------------------------------------------------------
  const checks = {
    allRunsHaveExpectedDecisions: allRuns.every((run) => run.checks.decisionsExpected),
    allRunsSnapshotChainsOk: allRuns.every((run) => run.checks.chainOk),
    allRunsFinalDigestsMatch: allRuns.every((run) => run.checks.finalDigestMatches),
    allRunsNoLostChanges: allRuns.every((run) => run.checks.noLostChanges),
    allRunsBaseDigestsMatch: allRuns.every((run) => run.baseDigestMatches),
    allRunsExitZero: allRuns.every((run) => run.spawnExit === 0),
    allScenariosDeterministic: scenarioAggregates.every((entry) => entry.deterministicAcrossReps),
    minimumReps: options.reps >= 5,
  };
  const valid = Object.values(checks).every(Boolean);

  const evidence = {
    schema_version: 1,
    suite: 'grouping-multiagent',
    generated_unix: nowUnix(),
    duration_ms: durationMs,
    protocol: {
      question:
        "Quelle fréquence réelle de conflits multi-agents rend le groupement de patches non rentable, et quel seuil de bascule employer ?",
      scenarios: SCENARIOS.map((scenario) => ({
        id: scenario.id,
        label: scenario.label,
        n: scenario.n,
        fixture: `benchmarks/grouping-multiagent/fixtures/${scenario.fixture}`,
        expectedDecisions: scenario.expectedDecisions,
        proposals: scenario.proposals.map((proposal) => ({
          proposalId: proposalId(proposal),
          path: proposal.path,
          symbols: proposal.symbols,
          find: proposal.find,
          replace: proposal.replace,
          declaredDependencies: proposal.declaredDependencies ?? [],
        })),
      })),
      repetitions: options.reps,
      order:
        'ordre des scénarios tourné d\'un cran à chaque répétition (alternance); ordre des propositions = ordre de soumission des fichiers 01..NN',
      decisionsSource: 'binaire ravum-collab (coordinateur réel, --verify: vérification sémantique rustc opt-in)',
      binary: path.relative(ROOT, options.bin),
      contentVerification:
        'digest de snapshot recalculé côté harnais (même formule que ravum-snapshot) comparé au snapshot_after réel; chaîne des accepted_changes; présence des éditions acceptées / absence des refusées; zéro changement perdu',
      modelLabel: 'MODÈLE CALIBRÉ — coûts unitaires phase 1 + fréquences de conflit mesurées ici',
      excluded: 'aucun build, lien ni activation mesurés ici; le groupement n\'est pas implémenté, il est modélisé',
    },
    machine: {
      os: os.platform(),
      release: os.release(),
      arch: os.arch(),
      logicalParallelism: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
      node: process.version,
      hostname: os.hostname(),
    },
    load: {
      start: loadStart,
      end: loadEnd,
      calibrationStartMs: calibrationStart,
      calibrationEndMs: calibrationEnd,
      note: 'machine de référence partagée avec d\'autres workers; charge non contrôlée, mono-exécution; mémoire libre et calibration rustc -vV enregistrées avant/après chaque répétition',
    },
    costs: model.costs,
    git: gitProvenance(),
    scenarios: scenarioAggregates,
    aggregate: overall,
    repetitions,
    model,
    checks,
    valid,
  };

  fs.mkdirSync(path.dirname(options.out), { recursive: true });
  fs.writeFileSync(options.out, `${JSON.stringify(evidence, null, 2)}\n`);
  if (!options.keep) fs.rmSync(workRoot, { recursive: true, force: true });

  // Résumé console.
  console.log('\nDécisions réelles par scénario (identiques sur toutes les répétitions) :');
  for (const entry of scenarioAggregates) {
    console.log(
      `  ${entry.id.padEnd(30)} N=${entry.n} ${entry.decisions.join(',')} (attendu ${entry.expectedDecisions.join(',')}) conflits-lots=${entry.runsWithConflict}/${entry.runs}`,
    );
  }
  console.log(
    `\nFréquence mesurée: lots avec ≥1 proposition non AUTO_SAFE = ${(overall.batchConflictRate * 100).toFixed(1)} % (${overall.batchConflictRuns}/${overall.runs}); propositions non AUTO_SAFE = ${(overall.proposalConflictRate * 100).toFixed(1)} %`,
  );
  console.log('\nMODÈLE calibré (coûts phase 1: cycle=' + C + 'ms, refus=' + costs.refusalMs + 'ms, reprise=' + R + 'ms) — seuils de bascule p* :');
  for (const n of [3, 4, 5, 6]) {
    const cells = WINDOWS_MS.map((windowMs) => `${windowMs}ms:${(breakEvenConflictRate(n, windowMs) * 100).toFixed(1)}%`);
    console.log(`  N=${n}  ${cells.join('  ')}`);
  }
  for (const window of mixEconomics) {
    console.log(
      `  mix mesuré, fenêtre ${String(window.windowMs).padStart(3)}ms: séquentiel=${window.sequentialCostMsPerMix}ms groupé=${window.groupedCostMsPerMix}ms gain=${window.gainMs}ms (${window.gainPercent.toFixed(1)}%) rentable=${window.profitable}`,
    );
  }
  console.log('\nVERDICT:');
  console.log(`  scénarios 100% compatibles rentables: ${model.verdict.compatibleScenariosProfitable}`);
  console.log(`  scénarios avec conflit rentables: ${model.verdict.conflictingScenariosProfitable}`);
  console.log(`  mix mesuré rentable (fenêtre 50 ms): ${model.verdict.measuredMixProfitableAtWindow50} (${model.verdict.measuredMixGainPercentAtWindow50.toFixed(1)}%)`);
  console.log(`\nvalid=${valid} checks=${JSON.stringify(checks)}`);
  console.log(`preuve: ${options.out}`);
  console.log(`durée: ${(durationMs / 1000).toFixed(1)} s`);

  if (!valid) process.exitCode = 1;
}

try {
  main();
} catch (error) {
  console.error(`grouping-multiagent: ${error && error.stack ? error.stack : error}`);
  process.exitCode = 2;
}

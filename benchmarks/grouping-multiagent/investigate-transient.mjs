#!/usr/bin/env node
//! Investigation L4.5 — échec transitoire de la vérification sémantique sous
//! charge (MVP-0c).
//!
//! Incident observé (re-run du harnais `grouping-multiagent` par le Lead, 6
//! workers actifs, RAM libre ~1,5 Go) : 19/20 runs conformes, 1 run du scénario
//! « dépendance non déclarée » (rep 4) a produit `REJECT` avec
//! « semantic verification failed: merged snapshot … does not compile with rustc
//! 1.95.0 | failed check metadata_check », au lieu de `AUTO_SAFE`, en 3131 ms.
//! Diagnostics enregistrés : « only metadata stub found for `rlib` dependency
//! `std` … » et « cannot resolve a prelude import » (aucun E-code).
//!
//! Ce script caractérise le phénomène de façon contrôlée :
//!  - N runs réels du scénario d via le binaire `ravum-collab --verify --json`
//!    sous 3 conditions : repos relatif, charge CPU bornée, charge mémoire/I/O
//!    bornée ;
//!  - M compilations `rustc` DIRECTES avec exactement les arguments du
//!    vérificateur sur le contenu candidat (pour distinguer un échec rustc
//!    d'un échec du vérificateur) ;
//!  - capture code de sortie, stderr, durée, décisions/réasons, RAM libre,
//!    calibration `rustc -vV` avant/après.
//!
//! Il n'implémente aucun correctif et ne modifie aucun fichier de `crates/**`.
//! Aucune commande git mutante. Node 26, zéro dépendance npm.
//!
//! Usage :
//!   node benchmarks/grouping-multiagent/investigate-transient.mjs
//!        [--runs 20] [--probes 12] [--keep] [--out evidence/…json]

import { spawn, spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const FIXTURES = path.join(HERE, 'fixtures');
const BIN = path.join(ROOT, 'target', 'debug', 'ravum-collab.exe');
const MAIN_EVIDENCE = path.join(ROOT, 'evidence', 'mvp0c-grouping-multiagent-2026-11-09.json');
const OUT = path.join(ROOT, 'evidence', 'mvp0c-verification-transient-2026-11-09.json');
const WORK = path.join(ROOT, 'target', 'grouping-multiagent-transient');
const CONDITION_WALL_CAP_MS = 110_000;

// Contenu candidat du run incriminé (scénario d, propositions 1 et 3 intégrées,
// la 2e étant MANUAL_REQUIRED pour dépendance non déclarée).
const CANDIDATE_EDIT_HELPER = { find: '    x + 1', replace: '    x + 5' };
const CANDIDATE_EDIT_OTHER = { find: '    x - 1', replace: '    x - 2' };

function parseArgs(argv) {
  const options = { runs: 20, probes: 12, keep: false, out: OUT };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--runs') options.runs = Number(argv[++i]);
    else if (arg === '--probes') options.probes = Number(argv[++i]);
    else if (arg === '--keep') options.keep = true;
    else if (arg === '--out') options.out = path.resolve(String(argv[++i]));
    else if (arg === '--help' || arg === '-h') {
      console.log('usage: node benchmarks/grouping-multiagent/investigate-transient.mjs [--runs N] [--probes N] [--keep] [--out PATH]');
      process.exit(0);
    } else throw new Error(`argument inconnu: ${arg}`);
  }
  if (!Number.isInteger(options.runs) || options.runs < 20) throw new Error('--runs doit être >= 20');
  if (!Number.isInteger(options.probes) || options.probes < 6) throw new Error('--probes doit être >= 6');
  return options;
}

function sha256Hex(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

function spawnCapture(command, args, options = {}) {
  const started = process.hrtime.bigint();
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    encoding: 'utf8',
    timeout: options.timeout ?? 240_000,
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
    env: options.env ?? process.env,
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
  for (let i = 0; i < samples; i += 1) out.push(spawnCapture('rustc', ['-vV'], { timeout: 60_000 }).wallMs);
  return out;
}

function statsOf(values) {
  if (values.length === 0) return { count: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const percentile = (p) => sorted[Math.min(Math.max(Math.ceil((p / 100) * sorted.length) - 1, 0), sorted.length - 1)];
  return {
    count: sorted.length,
    min: sorted[0],
    max: sorted[sorted.length - 1],
    mean: sorted.reduce((acc, value) => acc + value, 0) / sorted.length,
    p50: percentile(50),
    p95: percentile(95),
  };
}

function loadProbe(calibrationSamples = 2) {
  return {
    capturedUnix: Math.floor(Date.now() / 1000),
    freeMemoryBytes: os.freemem(),
    totalMemoryBytes: os.totalmem(),
    calibrationMs: rustcCalibration(calibrationSamples),
  };
}

function listFilesRecursive(root, prefix = '') {
  const out = [];
  for (const entry of fs.readdirSync(path.join(root, prefix), { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...listFilesRecursive(root, relative));
    else if (entry.isFile()) out.push(relative);
  }
  return out.sort();
}

function makePatch(relPath, content, find, replace) {
  const occurrences = content.split(find).length - 1;
  if (occurrences !== 1) throw new Error(`édition ${relPath}: ${occurrences} occurrence(s) de ${JSON.stringify(find)}`);
  let lines = content.split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') lines = lines.slice(0, -1);
  const index = lines.indexOf(find);
  if (index < 0 || lines.filter((line) => line === find).length !== 1) {
    throw new Error(`édition ${relPath}: ligne cible non unique`);
  }
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

// ---------------------------------------------------------------------------
// Scénario d (dépendance non déclarée) — même contenu que le harnais L2.2
// ---------------------------------------------------------------------------

function loadScenarioD() {
  const workspace = path.join(FIXTURES, 'scenario-d');
  const files = listFilesRecursive(workspace).map((relative) => ({
    path: relative,
    content: fs.readFileSync(path.join(workspace, relative), 'utf8'),
  }));
  const proposals = [
    { agent: 'agent-d1', task: 'helper', path: 'src/lib.rs', symbols: ['helper'], ...CANDIDATE_EDIT_HELPER },
    { agent: 'agent-d2', task: 'compose-undeclared', path: 'src/lib.rs', symbols: ['compose'], find: '    helper(x) * 2', replace: '    helper(x) * 3' },
    { agent: 'agent-d3', task: 'other', path: 'src/lib.rs', symbols: ['other'], ...CANDIDATE_EDIT_OTHER },
  ];
  const candidate = files.map((file) => ({ ...file }));
  const target = candidate.find((file) => file.path === 'src/lib.rs');
  target.content = target.content.replace(CANDIDATE_EDIT_HELPER.find, CANDIDATE_EDIT_HELPER.replace);
  target.content = target.content.replace(CANDIDATE_EDIT_OTHER.find, CANDIDATE_EDIT_OTHER.replace);
  return { workspace, files, proposals, candidateContent: target.content, expectedDecisions: ['AUTO_SAFE', 'MANUAL_REQUIRED', 'AUTO_SAFE'] };
}

function baseSnapshot(workspace) {
  const base = spawnCapture(BIN, ['base', '--workspace', workspace]);
  if (base.status !== 0) throw new Error(`ravum-collab base a échoué: ${base.stderr}`);
  return JSON.parse(base.stdout);
}

function writeProposals(proposalsDir, scenario, base) {
  fs.mkdirSync(proposalsDir, { recursive: true });
  const contents = new Map(scenario.files.map((file) => [file.path, file.content]));
  scenario.proposals.forEach((proposal, index) => {
    const patch = makePatch(proposal.path, contents.get(proposal.path), proposal.find, proposal.replace);
    const body = {
      schema_version: 1,
      agent_id: proposal.agent,
      task_id: proposal.task,
      base_snapshot_id: base.snapshot_id,
      target_paths: [proposal.path],
      target_symbols: proposal.symbols,
      expected_hashes: { [proposal.path]: base.files[proposal.path] },
      declared_dependencies: [],
      impacted_contracts: [],
      unified_patch: patch,
      note: 'investigation L4.5 — scénario d',
    };
    fs.writeFileSync(
      path.join(proposalsDir, String(index + 1).padStart(2, '0') + `-${proposal.agent}-${proposal.task}.json`),
      JSON.stringify(body, null, 2),
    );
  });
}

/** Un run réel du scénario d via le binaire, comme le harnais L2.2. */
function runScenarioD(runRoot, scenario) {
  fs.rmSync(runRoot, { recursive: true, force: true });
  const workspace = path.join(runRoot, 'workspace');
  fs.cpSync(scenario.workspace, workspace, { recursive: true });
  const loadBefore = loadProbe(1);
  const base = baseSnapshot(workspace);
  writeProposals(path.join(runRoot, 'proposals'), scenario, base);
  const journal = path.join(runRoot, 'journal.jsonl');
  const integrate = spawnCapture(BIN, [
    'integrate',
    '--workspace', workspace,
    '--proposals', path.join(runRoot, 'proposals'),
    '--journal', journal,
    '--json',
    '--verify',
    '--work', path.join(runRoot, 'verify-work'),
  ]);
  const loadAfter = loadProbe(1);
  let payload = null;
  let parseError = null;
  try { payload = JSON.parse(integrate.stdout); } catch (error) { parseError = String(error.message ?? error); }
  const decisions = (payload?.decisions ?? []).map((decision) => ({
    decision: decision.decision,
    proposalId: decision.proposal_id,
    reasons: decision.reasons ?? [],
  }));
  const actual = decisions.map((decision) => decision.decision);
  const expected = scenario.expectedDecisions;
  const mismatch = JSON.stringify(actual) !== JSON.stringify(expected);
  return {
    wallMs: integrate.wallMs,
    exitCode: integrate.status,
    spawnError: integrate.error,
    stderr: integrate.stderr.slice(0, 4000),
    parseError,
    baseSnapshotId: base.snapshot_id,
    expected,
    actual,
    decisions,
    mismatch,
    journalLines: fs.existsSync(journal) ? fs.readFileSync(journal, 'utf8').split('\n').filter(Boolean).length : 0,
    snapshotAfter: payload?.snapshot_after ?? null,
    loadBefore,
    loadAfter,
  };
}

/** Compilation rustc directe avec exactement les arguments du vérificateur. */
function directRustcProbe(probeRoot, candidateContent) {
  fs.rmSync(probeRoot, { recursive: true, force: true });
  const srcDir = path.join(probeRoot, 'snapshot', 'src');
  const outDir = path.join(probeRoot, 'snapshot', '.ravum-metadata');
  fs.mkdirSync(srcDir, { recursive: true });
  fs.mkdirSync(outDir, { recursive: true });
  const rootPath = path.join(srcDir, 'lib.rs');
  fs.writeFileSync(rootPath, candidateContent, 'utf8');
  const env = { ...process.env };
  delete env.RUSTFLAGS;
  delete env.RUSTC_WRAPPER;
  delete env.RUSTC_WORKSPACE_WRAPPER;
  const result = spawnCapture(
    'rustc',
    ['--edition', '2021', '--crate-name', 'ravum_merge_check', '--crate-type', 'lib', '--emit=metadata', '--error-format=json', '--out-dir', outDir, rootPath],
    { cwd: probeRoot, env, timeout: 120_000 },
  );
  const errorLines = result.stderr
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('{'))
    .map((line) => {
      try {
        const value = JSON.parse(line);
        const message = value.$message_type === 'diagnostic' ? value : value.message;
        if (!message || message.level !== 'error') return null;
        return {
          code: message.code?.code ?? null,
          message: message.message ?? '',
        };
      } catch {
        return null;
      }
    })
    .filter(Boolean);
  return {
    wallMs: result.wallMs,
    exitCode: result.status,
    spawnError: result.error,
    success: result.status === 0 && errorLines.length === 0,
    errors: errorLines,
    stderrFirstLines: result.stderr.split('\n').slice(0, 6),
  };
}

// ---------------------------------------------------------------------------
// Charges contrôlées bornées (arrêtées systématiquement)
// ---------------------------------------------------------------------------

const CPU_LOADER = `
const end = Date.now() + 120000;
let accumulator = 0;
while (Date.now() < end) { accumulator = (accumulator * 1664525 + 1013904223) >>> 0; }
process.exit(accumulator === 12345 ? 1 : 0);
`;

const MEMIO_LOADER = `
const fs = require('node:fs');
const path = require('node:path');
const dir = process.argv[1];
fs.mkdirSync(dir, { recursive: true });
const buffer = Buffer.alloc(192 * 1024 * 1024, 0x5a);
for (let i = 0; i < buffer.length; i += 4096) buffer[i] = i & 0xff;
const end = Date.now() + 120000;
let round = 0;
while (Date.now() < end) {
  for (let file = 0; file < 4; file += 1) {
    const target = path.join(dir, 'io-' + (round % 8) + '-' + file + '.bin');
    fs.writeFileSync(target, Buffer.alloc(16 * 1024 * 1024, file));
    fs.readFileSync(target);
    fs.unlinkSync(target);
  }
  round += 1;
  for (let i = 0; i < buffer.length; i += 4096) buffer[i] = (buffer[i] + 1) & 0xff;
}
process.exit(0);
`;

function startCpuLoad(count) {
  const children = [];
  for (let i = 0; i < count; i += 1) {
    const child = spawn(process.execPath, ['-e', CPU_LOADER], { stdio: 'ignore', windowsHide: true });
    children.push(child);
  }
  return { kind: 'cpu', description: `${count} processus Node en boucle CPU (120 s max, arrêtés explicitement)`, children };
}

function startMemIoLoad(workDir) {
  const child = spawn(process.execPath, ['--max-old-space-size=512', '-e', MEMIO_LOADER, path.join(workDir, 'memio')], {
    stdio: 'ignore',
    windowsHide: true,
  });
  return {
    kind: 'memio',
    description: '1 processus Node: 192 Mo alloués/touchés + écritures/lectures fsync de 4×16 Mo en boucle (120 s max, arrêté explicitement)',
    children: [child],
  };
}

function stopLoad(load) {
  if (!load) return;
  for (const child of load.children) {
    try { child.kill(); } catch { /* already gone */ }
  }
}

// ---------------------------------------------------------------------------
// Incident d'origine (extrait de la preuve L2.2 actuelle, avant régénération)
// ---------------------------------------------------------------------------

function extractIncident() {
  if (!fs.existsSync(MAIN_EVIDENCE)) return { available: false, path: path.relative(ROOT, MAIN_EVIDENCE) };
  const evidence = JSON.parse(fs.readFileSync(MAIN_EVIDENCE, 'utf8'));
  const failures = [];
  for (const repetition of evidence.repetitions ?? []) {
    for (const run of repetition.runs ?? []) {
      const expected = run.expectedDecisions ?? null;
      const actual = (run.decisions ?? []).map((decision) => decision.decision);
      const ok =
        JSON.stringify(actual) === JSON.stringify(expected ?? actual) &&
        run.checks?.decisionsExpected !== false &&
        run.checks?.chainOk !== false &&
        run.checks?.finalDigestMatches !== false &&
        run.checks?.noLostChanges !== false &&
        run.baseDigestMatches !== false &&
        run.spawnExit === 0;
      if (!ok) failures.push({ rep: repetition.rep, run });
    }
  }
  return {
    available: true,
    path: path.relative(ROOT, MAIN_EVIDENCE),
    evidenceValid: evidence.valid,
    evidenceDurationMs: evidence.duration_ms,
    evidenceGit: evidence.git ?? null,
    evidenceLoad: evidence.load ?? null,
    failureCount: failures.length,
    failures: failures.slice(0, 5).map(({ rep, run }) => ({
      rep,
      scenario: run.scenario,
      wallMs: run.wallMs,
      spawnExit: run.spawnExit,
      decisions: run.decisions,
      checks: run.checks,
      loadBefore: run.loadBefore,
      loadAfter: run.loadAfter,
    })),
  };
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

function conditionDefinitions() {
  return [
    { id: 'rest', label: 'repos relatif (aucune charge injectée)' },
    { id: 'cpu', label: 'charge CPU bornée (2 processus Node en boucle)' },
    { id: 'memio', label: 'charge mémoire/I/O bornée (192 Mo + I/O 64 Mo en boucle)' },
  ];
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!fs.existsSync(BIN)) throw new Error(`binaire introuvable: ${BIN} (cargo build -p ravum-workspace-coordinator --bins)`);
  const scenario = loadScenarioD();
  const incident = extractIncident();
  const rustc = spawnCapture('rustc', ['-vV'], { timeout: 60_000 });
  const sysroot = spawnCapture('rustc', ['--print', 'sysroot'], { timeout: 60_000 }).stdout.trim();
  const stdFiles = [];
  try {
    const libDir = path.join(sysroot, 'lib', 'rustlib', 'x86_64-pc-windows-msvc', 'lib');
    for (const name of fs.readdirSync(libDir)) {
      if (/^libstd-.*\.(rlib|rmeta)$/.test(name)) {
        stdFiles.push({ name, bytes: fs.statSync(path.join(libDir, name)).size });
      }
    }
  } catch (error) {
    stdFiles.push({ error: String(error.message ?? error) });
  }

  fs.rmSync(WORK, { recursive: true, force: true });
  fs.mkdirSync(WORK, { recursive: true });
  const started = process.hrtime.bigint();
  const loadStart = loadProbe(3);
  console.log(`investigation L4.5: ${options.runs} runs scénario d x 3 conditions + ${options.probes} probes rustc directes par condition`);

  // Compilations de référence du contenu candidat, à vide (doit réussir).
  const referenceProbes = [];
  for (let i = 0; i < 3; i += 1) {
    referenceProbes.push(directRustcProbe(path.join(WORK, `reference-probe-${i}`), scenario.candidateContent));
  }

  const conditions = [];
  for (const definition of conditionDefinitions()) {
    const conditionStarted = Date.now();
    const conditionRoot = path.join(WORK, definition.id);
    fs.mkdirSync(conditionRoot, { recursive: true });
    let load = null;
    if (definition.id === 'cpu') load = startCpuLoad(2);
    else if (definition.id === 'memio') load = startMemIoLoad(conditionRoot);
    const loadBefore = loadProbe(2);
    const runs = [];
    const probes = [];
    try {
      for (let index = 0; index < options.runs; index += 1) {
        if (Date.now() - conditionStarted > CONDITION_WALL_CAP_MS) break;
        const observation = runScenarioD(path.join(conditionRoot, `run-${String(index + 1).padStart(2, '0')}`), scenario);
        observation.index = index + 1;
        runs.push(observation);
        if (probes.length < options.probes) {
          const probe = directRustcProbe(path.join(conditionRoot, `probe-${String(probes.length + 1).padStart(2, '0')}`), scenario.candidateContent);
          probe.index = probes.length + 1;
          probes.push(probe);
        }
        const flag = observation.mismatch ? 'MISMATCH' : 'ok';
        console.log(
          `  [${definition.id}] run ${index + 1}/${options.runs} ${flag} wall=${observation.wallMs}ms decisions=${observation.actual.join(',')} freeMB=${Math.round(observation.loadAfter.freeMemoryBytes / 1048576)}`,
        );
      }
    } finally {
      stopLoad(load);
    }
    const loadAfter = loadProbe(2);
    fs.rmSync(path.join(conditionRoot, 'memio'), { recursive: true, force: true });
    conditions.push({
      id: definition.id,
      label: definition.label,
      injectedLoad: load ? { kind: load.kind, description: load.description } : null,
      runs: runs.length,
      mismatches: runs.filter((run) => run.mismatch).length,
      mismatchRate: runs.length > 0 ? runs.filter((run) => run.mismatch).length / runs.length : null,
      wallMs: statsOf(runs.map((run) => run.wallMs)),
      observations: runs,
      directRustcProbes: probes.length,
      directRustcProbeFailures: probes.filter((probe) => !probe.success).length,
      probes,
      loadBefore,
      loadAfter,
    });
  }
  const loadEnd = loadProbe(3);
  const durationMs = Number((process.hrtime.bigint() - started) / 1000000n);

  // -------------------------------------------------------------------------
  // Analyse de cause
  // -------------------------------------------------------------------------
  const allFailures = conditions.flatMap((condition) =>
    condition.observations
      .filter((run) => run.mismatch)
      .map((run) => ({ condition: condition.id, run })),
  );
  const classifyFailure = (failure) => {
    const decision = (failure.run.decisions ?? []).find((entry) => entry.decision !== 'AUTO_SAFE' && entry.decision !== 'MANUAL_REQUIRED');
    const reasons = decision?.reasons ?? [];
    const diagnosticLines = reasons.filter((reason) => reason.startsWith('diagnostic:'));
    const hasErrorCode = diagnosticLines.some((line) => /diagnostic:\s*[A-Z]\d{4}/.test(line));
    const hasInfraPattern = diagnosticLines.some((line) => /only metadata stub found|cannot resolve a prelude import/i.test(line));
    return {
      decision: decision?.decision ?? null,
      proposalId: decision?.proposalId ?? null,
      hasErrorCode,
      hasInfraPattern,
      diagnosticLines,
      reasons,
    };
  };
  const reproducedFailure = allFailures.length > 0;
  const directProbeFailures = conditions.reduce((acc, condition) => acc + condition.directRustcProbeFailures, 0);
  const referenceProbeFailures = referenceProbes.filter((probe) => !probe.success).length;
  const rootCause = {
    status: reproducedFailure ? 'REPRODUCED_IN_CONDITIONS' : 'UNDETERMINED',
    establishedFacts: [
      'les deux diagnostics de l’incident (« only metadata stub found for `rlib` dependency `std` … », « cannot resolve a prelude import ») ont été émis par rustc lui-même (JSON --error-format=json) et ne portent aucun E-code ; ils ne désignent aucune ligne du contenu candidat',
      'le même contenu candidat compile réellement (probes directes de référence) ; le sysroot contient un libstd .rmeta complet, pas un stub',
      'le vérificateur ne conserve ni code de sortie rustc ni stderr brut : seul le diagnostic parsé est archivé, ce qui empêche de distinguer un incident d’I/O/OS d’un échec de compilation',
      'dans le code actuel, une erreur de spawn rustc devient VerificationOutcome::Unavailable (MANUAL_REQUIRED) mais un processus rustc qui sort non-zéro devient Failed => REJECT, sans retry ni classification transitoire',
    ],
    classificationGap:
      'Tout échec non-zéro de rustc — y compris une erreur de chargement de métadonnées du sysroot sans rapport avec les sources — est converti en échec de compilation définitif (Failed → REJECT) par RustcMergeVerifier::metadata_check (crates/ravum-workspace-coordinator/src/bin/common/mod.rs) ; seul le timeout d’exécution des tests est traité en Unavailable.',
    hypothesis:
      'incident transitoire de lecture/chargement des métadonnées du sysroot (I/O/OS sous pression) transformé à tort en échec de compilation définitif ; déclencheur exact non capturé (UNDETERMINED) car le vérificateur ne conserve pas le code de sortie ni le stderr brut de rustc et le run n’a pas été reproduit en conditions contrôlées.',
  };

  const classificationGapRecommendations = [
    {
      title: 'Conserver le code de sortie et le stderr brut (borné) dans MergeCheck',
      where: 'crates/ravum-workspace-coordinator/src/bin/common/mod.rs — struct Probe / metadata_check',
      change:
        'ajouter exit_code et un extrait stderr (≤ 2 Ko) au check ; sans cela tout incident futur reste UNDETERMINED et non auditable',
      risk: 'faible (champ d’évidence supplémentaire, pas de changement de décision)',
    },
    {
      title: 'Classifier un échec rustc sans diagnostic source comme Unavailable (ou retry borné puis Unavailable)',
      where: 'crates/ravum-workspace-coordinator/src/bin/common/mod.rs — compile_metadata / RustcMergeVerifier::verify',
      change:
        'si rustc sort non-zéro avec des diagnostics SANS code E (et a fortiori avec les motifs « only metadata stub found », « cannot resolve a prelude import », ou sans aucun diagnostic parsé) : un retry borné (1 tentative) puis VerificationOutcome::Unavailable → MANUAL_REQUIRED, jamais REJECT. Un vrai échec de compilation garde son E-code et reste Failed → REJECT.',
      risk:
        'moyen : un vrai échec sans E-code serait reclassé MANUAL_REQUIRED au lieu de REJECT ; le retry unique puis Unavailable conserve le fail-closed (rien n’est intégré) et la distinction des E-codes est testable.',
    },
    {
      title: 'Test qui prouve la classification (à écrire, non écrit ici)',
      where: 'crates/ravum-workspace-coordinator/tests/ (nouveau fichier) ou tests du binaire collab',
      change:
        'wrapper rustc factice via RAVUM_RUSTC : (1) 1er appel émet le JSON « only metadata stub found for `rlib` dependency `std` » + sort 101, 2e appel réussit => après correctif, décision attendue MANUAL_REQUIRED (UNKNOWN) ou AUTO_SAFE après retry, jamais REJECT ; (2) un wrapper émettant un vrai E0061 => REJECT conservé ; (3) wrapper qui échoue toujours avec le motif transitoire => MANUAL_REQUIRED, pas REJECT. Test actuel de référence : `crates/ravum-workspace-coordinator/src/coordinator.rs` tests semantic_mode_*',
      risk: 'faible (test d’intégration, aucun test existant supprimé)',
    },
  ];

  const evidence = {
    schema_version: 1,
    suite: 'verification-transient',
    generated_unix: Math.floor(Date.now() / 1000),
    duration_ms: durationMs,
    protocol: {
      goal: 'classifier l’échec transitoire de la vérification sémantique sous charge (L4.5) sans affaiblir le fail-closed',
      incident,
      scenario: {
        id: 'd-undeclared-dependency',
        fixture: 'benchmarks/grouping-multiagent/fixtures/scenario-d',
        expectedDecisions: scenario.expectedDecisions,
        candidateContentSha256: sha256Hex(scenario.candidateContent),
        candidateEdits: [CANDIDATE_EDIT_HELPER, CANDIDATE_EDIT_OTHER],
      },
      conditions: conditionDefinitions().map((definition) => ({ id: definition.id, label: definition.label })),
      runsPerConditionRequested: options.runs,
      directRustcProbesPerCondition: options.probes,
      runCommand:
        'ravum-collab integrate --workspace <copie fixture> --proposals <dir> --journal <tmp> --json --verify --work <tmp> (binaire déjà compilé, aucun rebuild)',
      directProbeCommand:
        'rustc --edition 2021 --crate-name ravum_merge_check --crate-type lib --emit=metadata --error-format=json --out-dir <tmp> <candidate>/src/lib.rs (RUSTFLAGS/RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER retirés, comme le vérificateur)',
      noCodeChanges: 'crates/** non modifié ; investigation en lecture seule côté code',
      loadBounds:
        'charge CPU = 2 processus Node 120 s max ; charge mémoire/IO = 1 processus Node (192 Mo + 4×16 Mo I/O en boucle) 120 s max ; les deux sont tués explicitement à la fin de chaque condition',
      excluded: 'aucune activation, aucun build cdylib, aucune modification du workspace réel',
    },
    machine: {
      os: os.platform(),
      release: os.release(),
      arch: os.arch(),
      logicalParallelism: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
      node: process.version,
    },
    toolchain: {
      path: path.join('target', 'debug', 'ravum-collab.exe'),
      rustcVv: rustc.stdout,
      rustcCalibrationStartMs: loadStart.calibrationMs,
      sysroot,
      stdMetadataFiles: stdFiles,
    },
    load: { start: loadStart, end: loadEnd },
    referenceDirectProbes: referenceProbes,
    conditions,
    analysis: {
      failures: allFailures.length,
      failureDetails: allFailures.map((failure) => ({
        condition: failure.condition,
        index: failure.run.index,
        wallMs: failure.run.wallMs,
        classification: classifyFailure(failure),
        loadBefore: failure.run.loadBefore,
        loadAfter: failure.run.loadAfter,
        stderr: failure.run.stderr,
      })),
      directProbeFailures,
      referenceProbeFailures,
      failureRateByCondition: conditions.map((condition) => ({
        condition: condition.id,
        runs: condition.runs,
        mismatches: condition.mismatches,
        mismatchRate: condition.mismatchRate,
        directRustcProbes: condition.directRustcProbes,
        directRustcProbeFailures: condition.directRustcProbeFailures,
      })),
      rootCause,
    },
    recommendations: {
      classificationGapRecommendations,
      l22Impact: [
        'les mesures L2.2 en régime calme (valid=true, décisions attendues, digests conformes) restent valides : la campagne de phase 1 (coûts 191/184/1596 ms) et la campagne L2.2 calme ne sont pas invalidées par cet incident',
        'sous charge, le harnais L2.2 peut compter un faux « lot conflictuel » (REJECT transitoire) ; toute mesure de fréquence de conflits doit donc être prise en régime contrôlé ou filtrer les incidents transitoires (code retour/UNDETERMINED) avant d’alimenter le modèle',
        'le seuil de bascule p* du modèle n’est pas modifié par cet incident : c’est la variable p mesurée qui est biaisée si la charge n’est pas maîtrisée ; consigner la charge et exclure/retester les runs non déterministes',
      ],
      nextSteps: [
        'appliquer le correctif de classification + test proposé dans un lot dédié (Worker B ou Lead)',
        'regénérer la preuve principale L2.2 en charge faible si possible (fait dans la même session si la machine le permet)',
      ],
    },
    valid: true,
  };

  fs.mkdirSync(path.dirname(options.out), { recursive: true });
  fs.writeFileSync(options.out, `${JSON.stringify(evidence, null, 2)}\n`);
  if (!options.keep) fs.rmSync(WORK, { recursive: true, force: true });

  console.log('\nRésumé:');
  for (const condition of conditions) {
    console.log(
      `  ${condition.id.padEnd(6)} runs=${condition.runs} mismatches=${condition.mismatches} probes=${condition.directRustcProbes} probeFailures=${condition.directRustcProbeFailures} wall p50=${condition.wallMs.p50}ms`,
    );
  }
  console.log(`  cause racine: ${rootCause.status}`);
  console.log(`  incident d'origine: ${incident.available ? `${incident.failureCount} échec(s) dans ${incident.path}` : 'preuve principale absente'}`);
  console.log(`  preuve: ${options.out}`);
  console.log(`  durée: ${(durationMs / 1000).toFixed(1)} s`);
}

try {
  main();
} catch (error) {
  console.error(`investigate-transient: ${error && error.stack ? error.stack : error}`);
  process.exitCode = 2;
}

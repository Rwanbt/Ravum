#!/usr/bin/env node
//! Harnais S8 MVP-0c — protocole réel + pilote scripté.
//!
//! Modes :
//!   --mode pilot   : exécute A/B/C avec des agents SCRIPTÉS DÉTERMINISTES
//!                    (pas des LLM), mesure tout, écrit une preuve JSON.
//!   --mode live    : prépare les arborescences et les prompts exacts pour la
//!                    campagne avec de VRAIS subagents (--phase prepare), puis
//!                    vérifie/intègre et attribue les tokens (--phase collect),
//!                    enfin agrège plusieurs campagnes (--phase aggregate).
//!
//! Aucune commande Git n'est exécutée : la stratégie A utilise des copies de
//! répertoire et une fusion 3-way en mémoire.
//!
//! Usage pilote :
//!   node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3
//!   node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3 --keep
//!
//! Usage campagne réelle (détails dans docs/mvp/MVP-0c-s8.md) :
//!   node benchmarks/s8-mvp0c/run.mjs --mode live --phase prepare --scenario A --rep 1 --campaign <dir>
//!   … Lead spawne les 3 subagents avec les prompts générés …
//!   node benchmarks/s8-mvp0c/run.mjs --mode live --phase collect --campaign <dir> --out <evidence.json>
//!   node benchmarks/s8-mvp0c/run.mjs --mode live --phase aggregate --campaigns <d1,d2,...> --out <summary.json>

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { CommandLog, parseCargoTest, run } from './lib/exec.mjs';
import { findPowerShell, machineInfo, probeLoad } from './lib/env.mjs';
import { copyTree, dirBytes, ensureDir, listFiles, manifestOf, readContents, rmrf, round, sanitizer, sha256File, sha256Hex, writeJsonAtomic } from './lib/fsutil.mjs';
import { AGENTS, PROTECTED_FILES, TASK_ORDER, TASKS, corpusManifest, tasksOf, taskById, referenceContents } from './lib/tasks.mjs';
import { runScenario } from './lib/scenario.mjs';
import { attributeUsage } from './lib/usage-attribution.mjs';
import { detectClobberedWrites, protectedCheck, referenceCheck, markerCheck } from './lib/verify.mjs';
import { aggregateScenario, buildSchedule, markdownTable, rawTable, writeEvidence, writeMarkdownReport } from './lib/report.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const FIXTURE = path.join(HERE, 'fixture');
const SAMPLER = path.join(HERE, 'lib', 'sampler.ps1');
const DOC_PATH = path.join(ROOT, 'docs', 'mvp', 'MVP-0c-s8.md');
const RESULTS_MD = path.join(HERE, 'results', 'latest.md');
const AGENT_SCRIPT = path.join(HERE, 'lib', 'agent-scripted.mjs');
const WRITE_RECEIPT = path.join(HERE, 'lib', 'write-receipt.mjs');
const EVIDENCE_DEFAULT = path.join(ROOT, 'evidence', 'mvp0c-s8-pilot-2026-11-09.json');

function parseArgs(argv) {
  const options = {
    mode: 'pilot',
    phase: null,
    reps: 3,
    rep: 1,
    scenario: null,
    campaign: null,
    campaigns: null,
    out: null,
    work: null,
    keep: false,
    jobs: 2,
    doc: DOC_PATH,
    agentCommand: 'scripted',
    doneJson: null,
    usage: 'auto',
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = () => argv[++i];
    if (arg === '--mode') options.mode = value();
    else if (arg === '--phase') options.phase = value();
    else if (arg === '--reps') options.reps = Number(value());
    else if (arg === '--scenario') options.scenario = String(value()).toUpperCase();
    else if (arg === '--rep') options.rep = Number(value());
    else if (arg === '--campaign') options.campaign = path.resolve(value());
    else if (arg === '--campaigns') options.campaigns = value().split(',').map((item) => path.resolve(item.trim())).filter(Boolean);
    else if (arg === '--out') options.out = path.resolve(value());
    else if (arg === '--work') options.work = path.resolve(value());
    else if (arg === '--keep') options.keep = true;
    else if (arg === '--jobs') options.jobs = Number(value());
    else if (arg === '--doc') options.doc = path.resolve(value());
    else if (arg === '--agent-command') options.agentCommand = value();
    else if (arg === '--done-json') options.doneJson = path.resolve(value());
    else if (arg === '--usage') options.usage = value();
    else if (arg === '--help' || arg === '-h') {
      console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(0, 30).join('\n'));
      process.exit(0);
    } else throw new Error(`argument inconnu: ${arg}`);
  }
  if (!['pilot', 'live'].includes(options.mode)) throw new Error(`mode inconnu: ${options.mode}`);
  if (!Number.isInteger(options.reps) || options.reps < 1) throw new Error('--reps doit être un entier >= 1');
  if (options.mode === 'live' && !['prepare', 'collect', 'aggregate'].includes(options.phase)) {
    throw new Error('--mode live exige --phase prepare|collect|aggregate');
  }
  return options;
}

function cargoEnv(options) {
  return {
    ...process.env,
    CARGO_TERM_COLOR: 'never',
    CARGO_NET_OFFLINE: 'true',
    CARGO_BUILD_JOBS: String(options.jobs),
    RUST_BACKTRACE: '0',
  };
}

async function cargoTestAt({ dir, env, log = null }) {
  const started = performance.now();
  const result = await run('cargo', ['test', '--quiet'], { cwd: dir, env, timeoutMs: 900000, maxOut: 1024 * 1024, log, label: 'cargo-test' });
  const parsed = parseCargoTest(result.stdout, result.stderr, result.code);
  return { ...parsed, ms: round(performance.now() - started), timed_out: result.timedOut, stdout_tail: result.stdout.slice(-4000), stderr_tail: result.stderr.slice(-4000) };
}

/** Sonde de contrôle : deux écrivains sur le même fichier, un écrasement doit être détecté. */
async function positiveControlConflictDetection({ workRoot, fixtureDir }) {
  const dir = path.join(workRoot, 'control');
  copyTree(fixtureDir, dir);
  const base = readContents(dir, ['src/stats.rs']);
  const writer1 = { agent: 'writer-1', path: 'src/stats.rs', final_sha256: sha256Hex(applyOne(base['src/stats.rs'], 't1')) };
  const writer2 = { agent: 'writer-2', path: 'src/stats.rs', final_sha256: sha256Hex(applyOne(base['src/stats.rs'], 't3')) };
  const started = performance.now();
  fs.writeFileSync(path.join(dir, 'src/stats.rs'), applyOne(base['src/stats.rs'], 't1'), 'utf8');
  fs.writeFileSync(path.join(dir, 'src/stats.rs'), applyOne(base['src/stats.rs'], 't3'), 'utf8');
  const finalContents = readContents(dir, ['src/stats.rs']);
  const detection = detectClobberedWrites({ intents: [writer1, writer2], finalContents });
  const detected = detection.clobbered_writers.includes('writer-1') && detection.files[0]?.final_equals_last_writer === true;
  return {
    ok: detected,
    ms: round(performance.now() - started),
    description:
      'writer-1 écrit stats.rs (moyenne), puis writer-2 écrit stats.rs calculé depuis la base (médiane) : l’édition de writer-1 est écrasée. Le détecteur doit la signaler.',
    intents: [writer1, writer2],
    final_sha256: detection.files[0]?.final_sha256 ?? null,
    clobbered_writers: detection.clobbered_writers,
    final_equals_last_writer: detection.files[0]?.final_equals_last_writer ?? false,
  };
}

function applyOne(text, taskId) {
  const task = taskById(taskId);
  if (!text.includes(task.oldSnippet)) throw new Error(`sonde: extrait absent pour ${taskId}`);
  return text.replace(task.oldSnippet, task.newSnippet);
}

// --- mode pilote -----------------------------------------------------------

async function runPilot(options) {
  const startedAt = new Date();
  const stamp = startedAt.toISOString().replace(/[-:]/g, '').replace(/\..+/, '');
  const workRoot = options.work ?? path.join(os.tmpdir(), `s8-mvp0c-pilot-${stamp}`);
  ensureDir(workRoot);
  const outFile = options.out ?? EVIDENCE_DEFAULT;
  const log = new CommandLog();
  const ps = findPowerShell();
  const env = cargoEnv(options);
  const sanitizePaths = sanitizer(workRoot);

  console.log('=== S8 MVP-0c — pilote scripté (A/B/C) ===');
  console.log('Agents : éditeurs SCRIPTÉS DÉTERMINISTES (pas des LLM) ; tokens IA et supervision humaine NON mesurés.');
  console.log(`Répertoire de travail : ${workRoot}`);
  console.log(`Preuve : ${outFile}`);

  const machine = await machineInfo({ log });
  const versions = machine.versions ?? {};
  const docDigest = fs.existsSync(options.doc) ? sha256File(options.doc) : null;
  const fixtureFiles = listFiles(FIXTURE);
  const fixtureManifest = manifestOf(FIXTURE);
  const editableFiles = [...new Set(TASKS.map((task) => task.path))];
  const schedule = buildSchedule(options.reps);
  const loadProbes = [];
  const probe = async (label) => {
    const sample = await probeLoad();
    loadProbes.push({ label, ...sample });
    console.log(`[charge] ${label}: RAM libre ${Math.round(sample.free_ram_bytes / (1024 * 1024))} Mo, CPU ${sample.load_pct ?? '?'}% sur ${sample.logical_cpus} cœurs logiques`);
  };

  const reps = [];
  const problems = [];
  let baseRed = null;
  let positiveControl = null;
  try {
    await probe('début de campagne');

    // 1. Vérification de la fixture : les tests doivent ÉCHOUER avant éditions.
    const baseCheckDir = path.join(workRoot, 'base-check');
    copyTree(FIXTURE, baseCheckDir);
    const baseTest = await cargoTestAt({ dir: baseCheckDir, env, log });
    baseRed = {
      ok: !baseTest.green && baseTest.failed > 0,
      ms: baseTest.ms,
      passed: baseTest.passed,
      failed: baseTest.failed,
      failed_names: baseTest.failed_names,
      note: 'les tests doivent être rouges avant les éditions ; le harnais échoue sinon',
    };
    if (!baseRed.ok) throw new Error(`fixture invalide : les tests de base ne sont pas rouges (passed=${baseTest.passed}, failed=${baseTest.failed})`);
    console.log(`[fixture] tests de base rouges comme attendu : ${baseTest.failed} échec(s), ${baseTest.passed} succès`);

    // 2. Contrôle positif de la détection de conflit.
    positiveControl = await positiveControlConflictDetection({ workRoot, fixtureDir: FIXTURE });
    if (!positiveControl.ok) throw new Error('contrôle positif de détection de conflit ÉCHOUÉ');
    console.log(`[contrôle] écrasement détecté=${positiveControl.ok} (${positiveControl.clobbered_writers.join(',')}) en ${positiveControl.ms} ms`);

    // 3. Répétitions alternées A/B/C.
    const ctx = {
      repoRoot: ROOT,
      fixtureDir: FIXTURE,
      workRoot,
      fixtureFiles,
      editableFiles,
      ps,
      samplerScript: SAMPLER,
      cargoEnv: env,
      sanitize: sanitizePaths,
      keep: options.keep,
      docPath: options.doc,
      pilot: { collisionDelayMs: 150, collisionStaggerMs: 25 },
      agentCommand: { kind: options.agentCommand === 'scripted' ? 'scripted' : 'custom', template: options.agentCommand },
    };
    for (let index = 0; index < schedule.length; index += 1) {
      const item = schedule[index];
      process.stdout.write(`[${index + 1}/${schedule.length}] ${item.scenario}${item.rep} … `);
      const result = await runScenario({ scenario: item.scenario, rep: item.rep, scheduleIndex: index + 1, ctx });
      reps.push(result);
      console.log(
        `${result.functional_success ? 'succès' : 'ÉCHEC'} | total ${result.timings.t_total_ms} ms | edit ${result.timings.edit_phase_ms} ms | ` +
          `intégration ${result.timings.integration_phase_ms} ms | test ${result.timings.test_ms} ms | conflits ${result.conflicts.count} | ` +
          `attente ${result.agent_wait_total_ms} ms | RAM ${result.resources.sampler.peak_ws_mb} Mo | CPU ${result.resources.sampler.cpu_seconds} s`,
      );
      if (index === Math.floor(schedule.length / 2) - 1) await probe('mi-campagne');
    }
    await probe('fin de campagne');
  } catch (error) {
    problems.push(`exception harnais: ${error.stack ?? error.message}`);
    throw error;
  } finally {
    if (!options.keep) {
      try { rmrf(workRoot); } catch { /* nettoyage best-effort */ }
    } else {
      console.log(`Répertoires conservés : ${workRoot}`);
    }
  }

  const scenarioSummaries = {};
  for (const scenario of ['A', 'B', 'C']) {
    scenarioSummaries[scenario] = aggregateScenario(reps.filter((rep) => rep.scenario === scenario));
  }
  for (const rep of reps) {
    if (!rep.tests.green) problems.push(`${rep.rep_id}: cargo test non vert (${rep.tests.passed} passés, ${rep.tests.failed} échoués)`);
    if (!rep.verification.content_ok) problems.push(`${rep.rep_id}: contenu final != référence (${rep.lost.content_mismatches.length} fichier(s))`);
    if (rep.lost.tasks_missing_markers.length > 0) problems.push(`${rep.rep_id}: changements perdus ${rep.lost.tasks_missing_markers.join(',')}`);
    if (rep.lost.protected_files_changed.length > 0) problems.push(`${rep.rep_id}: fichiers protégés modifiés ${rep.lost.protected_files_changed.join(',')}`);
    if (rep.lost.missing_receipts.length > 0) problems.push(`${rep.rep_id}: reçus manquants ${rep.lost.missing_receipts.join(',')}`);
    if (rep.scenario === 'B' && rep.conflicts.count < 1) problems.push(`${rep.rep_id}: collision injectée non détectée (conflits=0)`);
  }
  const validation = { ok: problems.length === 0, problems };

  const payload = {
    schema_version: 1,
    campaign: 'S8 MVP-0c — pilote scripté',
    mode: 'pilot',
    generated_at: new Date().toISOString(),
    protocol: {
      pre_registered_doc: path.relative(ROOT, options.doc).replaceAll('\\', '/'),
      doc_sha256_at_run: docDigest,
      note: 'protocole et critères rédigés avant le pilote ; le sha256 est celui de la doc au démarrage du run',
      strategies: {
        A: 'copie de la fixture par agent (3 copies), puis intégration par fusion 3-way en mémoire (aucune commande Git)',
        B: 'un seul répertoire partagé, agents concurrents, garde de version optimiste et collision injectée déterministe',
        C: 'un seul répertoire, agents strictement séquentiels',
      },
      agents: AGENTS.length,
      tasks: TASK_ORDER.length,
      reps: options.reps,
      order: 'alterné : A,B,C puis C,B,A, puis A,B,C…',
      success_criteria: [
        'cargo test vert après éditions (12/12)',
        'contenu final exactement égal à la référence (mode pilote)',
        'aucun changement perdu, aucun fichier protégé modifié, aucun reçu manquant',
        'fixture rouge avant éditions (vérifiée)',
        'détection de conflit exercée en B (collision injectée) et contrôle positif du détecteur',
      ],
    },
    environment: {
      generated_at: startedAt.toISOString(),
      machine,
      versions,
      disk: machine.disk,
      load_probes: loadProbes,
      allow_network: false,
      cargo_build_jobs: options.jobs,
    },
    fixture: {
      path: 'benchmarks/s8-mvp0c/fixture',
      files: fixtureFiles,
      manifest: fixtureManifest,
      base_red_check: baseRed,
      corpus: corpusManifest(),
    },
    positive_control: positiveControl,
    schedule,
    agents: AGENTS,
    tokens: {
      measured: false,
      reason:
        'pilote exécuté avec des éditeurs SCRIPTÉS DÉTERMINISTES : aucun LLM appelé, aucune session DSH produite. Les tokens réels ne sont pas mesurés ici et ne seront mesurés que par la campagne live via tools/agent-usage/usage.mjs.',
      per_agent: null,
      tool: 'tools/agent-usage/usage.mjs (campagne live uniquement)',
    },
    human_supervision: { measured: false, reason: 'pilote scripté ; le temps de supervision humaine de la campagne réelle sera mesuré par le Lead' },
    reps,
    scenario_summaries: scenarioSummaries,
    validation,
    limitations: [
      'agents scriptés déterministes, PAS des LLM : aucun token, aucune qualité de code généré, aucun temps de supervision humaine mesurés',
      'stratégie A simulée par copies + fusion 3-way en mémoire, pas par des worktrees Git (contrainte « aucune commande git mutante »)',
      'collision de la stratégie B injectée par le pilote (fenêtre déterministe) ; la campagne réelle ne l’injecte pas',
      'machine partagée avec d’autres agents : charge mesurée et documentée, pas contrôlée',
      'échantillon restreint (3 répétitions) et fixture locale petite',
      'le cache target/ est froid pour chaque répétition (même coût pour A/B/C)',
    ],
  };

  const summary = { scenarios: scenarioSummaries };
  const markdown = markdownTable(summary);
  const rawMarkdown = rawTable(reps);
  writeEvidence({ file: outFile, payload });
  const mdFile = writeMarkdownReport({ file: RESULTS_MD, payload, markdown, rawMarkdown });

  console.log('\n=== Tableau comparatif (médiane [min–max] (IQR)) ===');
  console.log(markdown);
  console.log(`\nPreuves : ${outFile}`);
  console.log(`Rapport  : ${mdFile}`);
  if (problems.length > 0) {
    console.error('\n=== PROBLÈMES ===');
    for (const problem of problems) console.error(`  - ${problem}`);
  }
  console.log(`\nRésultat pilote : ${validation.ok ? 'OK' : 'ÉCHEC'}`);
  return validation.ok ? 0 : 1;
}

// --- mode live : préparation / collecte / agrégation ------------------------

function promptForAgent({ agent, scenario, rep, workspaceDir, marker, basePath, receiptPath }) {
  const tasks = tasksOf(agent).map((task) => `- ${task.id} : \`${task.title}\` dans \`${task.path}\``).join('\n');
  const scenarioNote =
    scenario === 'A'
      ? 'Tu travailles dans une COPIE PRIVÉE de la fixture : les autres agents ont leurs propres copies. Personne d’autre n’écrit dans ton répertoire.'
      : scenario === 'B'
        ? 'Tu travailles dans un RÉPERTOIRE PARTAGÉ : les autres agents écrivent en même temps que toi. Avant chaque écriture, relis le fichier et vérifie qu’il n’a pas changé ; si c’est le cas, réapplique ton édition sur la version à jour (garde de version).'
        : 'Tu interviens SEUL dans le répertoire partagé (campagne séquentielle) : aucun autre agent n’écrit en même temps que toi.';
  return `Tu es ${marker} (agent ${agent}) de la campagne S8 MVP-0c (scénario ${scenario}, répétition ${rep}).

${scenarioNote}

Fixture Rust réelle : ${workspaceDir}
Les tests du crate échouent avant les éditions et doivent passer après.

Tâches assignées (et SEULEMENT celles-là) :
${tasks}

Contraintes :
- Ne modifie JAMAIS : ${PROTECTED_FILES.join(', ')}.
- N’utilise pas git ; pas de réseau.
- Ne lance PAS \`cargo test\` / \`cargo build\` : le harnais compile et vérifie après ta réponse (borne le coût).
- Ne touche pas aux fichiers des autres tâches ; écris du Rust idiomatique (edition 2021).

À la fin, écris ton reçu en exécutant exactement :
  node ${WRITE_RECEIPT} --agent ${agent} --workspace ${workspaceDir} --base ${basePath} --receipt ${receiptPath}
puis réponds uniquement : DONE ${marker}`;
}

async function livePrepare(options) {
  const scenario = options.scenario;
  if (!scenario || !['A', 'B', 'C'].includes(scenario)) throw new Error('--phase prepare exige --scenario A|B|C');
  const rep = options.rep;
  const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..+/, '');
  const campaignDir = options.campaign ?? path.join(os.tmpdir(), `s8-mvp0c-live-${scenario}${rep}-${stamp}`);
  ensureDir(campaignDir);
  const baseDir = path.join(campaignDir, 'base');
  copyTree(FIXTURE, baseDir);
  const baseContents = readContents(baseDir, listFiles(FIXTURE));
  const receiptsDir = path.join(campaignDir, 'receipts');
  ensureDir(receiptsDir);
  const promptsDir = path.join(campaignDir, 'prompts');
  ensureDir(promptsDir);
  const markers = AGENTS.map((agent) => ({ agent: agent.id, marker: `S8MVP0C-AGENT-${agent.id.split('-')[1]}` }));
  const agentDirs = {};
  const basePaths = {};
  const receiptPaths = {};
  for (const agent of AGENTS) {
    const workspaceDir = scenario === 'A' ? path.join(campaignDir, 'agents', agent.id) : path.join(campaignDir, 'shared');
    if (scenario === 'A') copyTree(baseDir, workspaceDir);
    const basePath = path.join(receiptsDir, `${agent.id}.base.json`);
    const hashes = {};
    for (const task of tasksOf(agent.id)) hashes[task.path] = sha256Hex(baseContents[task.path]);
    writeJsonAtomic(basePath, hashes);
    basePaths[agent.id] = basePath;
    receiptPaths[agent.id] = path.join(receiptsDir, `${agent.id}.json`);
    agentDirs[agent.id] = workspaceDir;
  }
  if (scenario !== 'A') {
    const sharedDir = path.join(campaignDir, 'shared');
    if (!fs.existsSync(sharedDir)) copyTree(baseDir, sharedDir);
  }
  const prompts = {};
  for (const agent of AGENTS) {
    const marker = markers.find((entry) => entry.agent === agent.id).marker;
    const prompt = promptForAgent({
      agent: agent.id,
      scenario,
      rep,
      workspaceDir: agentDirs[agent.id],
      marker,
      basePath: basePaths[agent.id],
      receiptPath: receiptPaths[agent.id],
    });
    const promptPath = path.join(promptsDir, `${agent.id}.md`);
    fs.writeFileSync(promptPath, `${prompt}\n`, 'utf8');
    prompts[agent.id] = promptPath;
  }
  const env = cargoEnv(options);
  const baseTest = await cargoTestAt({ dir: baseDir, env });
  const plan = {
    schema_version: 1,
    mode: 'live',
    phase: 'prepared',
    scenario,
    rep,
    created_at: new Date().toISOString(),
    started_at_epoch_ms: Date.now(),
    repo_root: ROOT,
    campaign_dir: campaignDir,
    fixture_dir: FIXTURE,
    fixture_files: listFiles(FIXTURE),
    editable_files: [...new Set(TASKS.map((task) => task.path))],
    protected_files: PROTECTED_FILES,
    markers,
    agents: AGENTS,
    agent_dirs: agentDirs,
    base_paths: basePaths,
    receipt_paths: receiptPaths,
    prompt_paths: prompts,
    base_test: baseTest,
    cargo_env: { CARGO_BUILD_JOBS: String(options.jobs), CARGO_NET_OFFLINE: 'true' },
    instructions:
      'Spawner 3 subagents (description contenant le marqueur), leur donner le prompt exact, attendre les réponses DONE, puis lancer --phase collect.',
  };
  writeJsonAtomic(path.join(campaignDir, 'plan.json'), plan);
  console.log(`Campagne préparée : ${campaignDir}`);
  console.log(`Tests de base (rouges attendus) : ${baseTest.failed} échec(s), ${baseTest.passed} succès`);
  for (const agent of AGENTS) console.log(`  ${agent.id}: prompt ${prompts[agent.id]}`);
  console.log('\nProchaines étapes :');
  console.log('  1. spawner un subagent par agent avec le prompt exact du fichier correspondant (description = marqueur)');
  console.log('  2. attendre DONE pour les 3 agents (ne pas lancer cargo)');
  console.log('  3. collecter : node benchmarks/s8-mvp0c/run.mjs --mode live --phase collect --campaign ' + campaignDir + ' --out <evidence.json>');
  return 0;
}

function readDoneJson(file) {
  if (!file || !fs.existsSync(file)) return null;
  const entries = JSON.parse(fs.readFileSync(file, 'utf8'));
  const byAgent = {};
  for (const entry of entries) byAgent[entry.agent] = entry;
  return byAgent;
}

async function liveCollect(options) {
  const campaignDir = options.campaign;
  if (!campaignDir) throw new Error('--phase collect exige --campaign <dir>');
  const plan = JSON.parse(fs.readFileSync(path.join(campaignDir, 'plan.json'), 'utf8'));
  const scenario = plan.scenario;
  const baseDir = path.join(campaignDir, 'base');
  const baseContents = readContents(baseDir, plan.fixture_files);
  const reference = referenceContents(baseContents);
  const startedEpoch = plan.started_at_epoch_ms;
  const done = readDoneJson(options.doneJson);
  const receipts = {};
  for (const agent of AGENTS) {
    const receiptPath = plan.receipt_paths[agent.id];
    receipts[agent.id] = fs.existsSync(receiptPath) ? JSON.parse(fs.readFileSync(receiptPath, 'utf8')) : null;
  }
  let integration = { conflicts: [], merge_ms: 0, merges: 0 };
  let finalDir;
  if (scenario === 'A') {
    const finals = AGENTS.map((agent) => {
      const dir = plan.agent_dirs[agent.id];
      return { agent: agent.id, finalContents: readContents(dir, plan.editable_files) };
    });
    const { integrateAgentResults } = await import('./lib/verify.mjs');
    const started = performance.now();
    const integrated = integrateAgentResults({ baseContents, agentResults: finals });
    integration = { conflicts: integrated.conflicts, merge_ms: round(integrated.merge_ms), merges: integrated.merges, ms: round(performance.now() - started) };
    finalDir = path.join(campaignDir, 'integration');
    copyTree(baseDir, finalDir);
    for (const [rel, text] of Object.entries(integrated.contents)) fs.writeFileSync(path.join(finalDir, rel), text, 'utf8');
  } else {
    finalDir = path.join(campaignDir, 'shared');
  }
  const finalContents = readContents(finalDir, plan.fixture_files);
  // En live, les agents réels écrivent leur PROPRE code : le contenu exact de la
  // référence scriptée n'est PAS le critère. Critères réels : cargo test vert,
  // reçus cohérents, fichiers protégés intacts, écritures non perdues.
  const contentReference = referenceCheck(finalContents, reference); // informatif seulement
  const markers = markerCheck(finalContents); // informatif seulement en live
  const protectedChanged = protectedCheck(finalContents, baseContents);
  const missingReceipts = AGENTS.filter((agent) => !receipts[agent.id]).map((agent) => agent.id);
  const badReceipts = AGENTS.filter((agent) => receipts[agent.id] && receipts[agent.id].status !== 'done').map((agent) => agent.id);
  const pathWriters = new Map();
  for (const agent of AGENTS) {
    for (const entry of receipts[agent.id]?.changed ?? []) {
      if (!entry.changed_vs_base) continue;
      if (!pathWriters.has(entry.path)) pathWriters.set(entry.path, []);
      pathWriters.get(entry.path).push(agent.id);
    }
  }
  const agentChangeChecks = AGENTS.map((agent) => {
    const checks = (receipts[agent.id]?.changed ?? []).map((entry) => {
      const actual = finalContents[entry.path];
      const actualSha = actual === undefined ? null : sha256Hex(actual);
      const writers = pathWriters.get(entry.path) ?? [];
      const singleWriter = writers.length === 1;
      return {
        path: entry.path,
        writers,
        present: actual !== undefined,
        changed_vs_base: actual !== undefined ? actualSha !== entry.base_sha256 : false,
        single_writer: singleWriter,
        recorded_final_sha256: entry.final_sha256,
        actual_sha256: actualSha,
        sha_match: singleWriter ? actualSha === entry.final_sha256 : null,
      };
    });
    return { agent: agent.id, checks, all_present: checks.every((check) => check.present) };
  });
  const singleWriterShaOk = agentChangeChecks.every((agent) => agent.checks.every((check) => !check.single_writer || check.sha_match === true));
  const lostWrites = agentChangeChecks.flatMap((agent) => agent.checks.filter((check) => !check.present).map((check) => ({ agent: agent.agent, path: check.path })));
  const multiWriterPaths = [...pathWriters.entries()].filter(([, writers]) => writers.length > 1).map(([p, writers]) => ({ path: p, writers }));
  const env = { ...process.env, ...plan.cargo_env, CARGO_TERM_COLOR: 'never' };
  const testStart = performance.now();
  const tests = await cargoTestAt({ dir: finalDir, env });
  const testMs = round(performance.now() - testStart);
  const totalMsFromStart = Date.now() - startedEpoch;
  const agentMetrics = AGENTS.map((agent) => {
    const receipt = receipts[agent.id];
    const doneEntry = done?.[agent.id] ?? null;
    const editMs = receipt?.edit_ms ?? null;
    const startedAt = doneEntry?.started_epoch_ms ?? startedEpoch;
    const doneAt = doneEntry?.done_epoch_ms ?? (receipt ? Date.parse(receipt.created_at) : null);
    return {
      agent: agent.id,
      marker: plan.markers.find((entry) => entry.agent === agent.id)?.marker ?? null,
      tasks: agent.tasks,
      receipt_status: receipt?.status ?? 'missing',
      changed: receipt?.changed ?? [],
      edit_ms: editMs,
      wait_ms: doneAt ? round(doneAt - startedAt) : null,
      conflicts: receipt?.conflicts ?? null,
      attempts: receipt?.attempts ?? null,
    };
  });
  // Attribution des tokens DSH (jamais inventés : null si non attribuables).
  let usage = { measured: false, reason: 'attribution désactivée (--usage none)' };
  if (options.usage !== 'none') {
    usage = await attributeUsage({
      repoRoot: ROOT,
      workspaceRoot: ROOT,
      markers: plan.markers,
      windowStartMs: startedEpoch,
      windowEndMs: Date.now(),
    });
  }
  const problems = [];
  if (!tests.green) problems.push(`cargo test non vert (${tests.passed} passés, ${tests.failed} échoués)`);
  if (!singleWriterShaOk) problems.push('une écriture d’agent à écrivain unique n’est plus celle annoncée par son reçu (perdue ou modifiée)');
  if (lostWrites.length > 0) problems.push(`fichiers modifiés absents du résultat final: ${lostWrites.map((item) => `${item.agent}:${item.path}`).join(',')}`);
  if (protectedChanged.length > 0) problems.push(`fichiers protégés modifiés: ${protectedChanged.join(',')}`);
  if (missingReceipts.length > 0) problems.push(`reçus manquants: ${missingReceipts.join(',')}`);
  if (badReceipts.length > 0) problems.push(`reçus incomplets: ${badReceipts.join(',')}`);
  if (options.usage !== 'none' && usage.measured && !usage.all_agents_matched) problems.push('tokens: toutes les sessions d’agents ne sont pas attribuées');
  const payload = {
    schema_version: 1,
    campaign: 'S8 MVP-0c — campagne réelle',
    mode: 'live-collect',
    generated_at: new Date().toISOString(),
    scenario: plan.scenario,
    rep: plan.rep,
    campaign_dir: campaignDir,
    protocol: {
      note: 'contenu exact de référence attendu uniquement si les agents scriptés sont utilisés ; en live, critère = cargo test vert + reçus cohérents + changements non perdus + fichiers protégés intacts',
    },
    plan,
    timings: {
      total_ms_from_start: totalMsFromStart,
      test_ms: testMs,
      integration_ms: scenario === 'A' ? integration.ms : 0,
    },
    agents: agentMetrics,
    receipts,
    integration: scenario === 'A' ? integration : null,
    tests,
    tokens: usage,
    verification: {
      reference_content_comparison: {
        note: 'informatif en live : les agents réels écrivent leur propre code, différent de la référence scriptée',
        ok: contentReference.ok,
        mismatches: contentReference.mismatches,
      },
      scripted_markers: { note: 'informatif en live : marqueurs de l’implémentation scriptée uniquement', markers },
      protected_ok: protectedChanged.length === 0,
      single_writer_sha_ok: singleWriterShaOk,
      lost_writes: lostWrites,
      multi_writer_paths: multiWriterPaths,
      agent_change_checks: agentChangeChecks,
    },
    validation: { ok: problems.length === 0, problems },
    limitations: [
      'les tokens proviennent uniquement des sessions DSH attribuées par marqueur ; aucune estimation',
      'si --usage auto et qu’une session de sous-agent n’est pas attribuée, la campagne est marquée incomplète',
      'le temps total est mesuré du démarrage de la préparation à la fin de la collecte (inclut la supervision humaine)',
      'CPU/RAM ne sont pas échantillonnés en mode live (les subagents sont pilotés par le Lead)',
    ],
  };
  const outFile = options.out ?? path.join(campaignDir, 'collect.json');
  writeEvidence({ file: outFile, payload });
  console.log(`Collecte écrite : ${outFile}`);
  console.log(`scénario ${plan.scenario}${plan.rep} : test=${tests.green ? 'vert' : 'ROUGE'} (${tests.passed}/${tests.passed + tests.failed}) total=${totalMsFromStart} ms`);
  if (usage.measured) console.log(`tokens : ${usage.agents_matched}/${plan.markers.length} agents attribués, ${usage.unmatched_count} session(s) non attribuée(s)`);
  if (problems.length > 0) for (const problem of problems) console.error(`  - ${problem}`);
  return problems.length === 0 ? 0 : 1;
}

async function liveAggregate(options) {
  if (!options.campaigns || options.campaigns.length === 0) throw new Error('--phase aggregate exige --campaigns <d1,d2,...>');
  const collected = options.campaigns.map((dir) => {
    const file = path.join(dir, 'collect.json');
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  });
  const groups = {};
  for (const item of collected) {
    const key = item.scenario;
    groups[key] = groups[key] ?? [];
    groups[key].push(item);
  }
  const summary = {};
  for (const [scenario, items] of Object.entries(groups)) {
    const totals = items.map((item) => item.timings.total_ms_from_start);
    const tokenRows = items.flatMap((item) => item.agents.map((agent) => ({ agent: agent.agent, tokens: item.tokens?.agents?.[agent.agent]?.totals ?? null })));
    summary[scenario] = {
      campaigns: items.length,
      total_ms: computeStats(totals),
      all_tests_green: items.every((item) => item.tests.green),
      all_protected_ok: items.every((item) => item.verification.protected_ok),
      zero_lost: items.every((item) => item.verification.lost_writes.length === 0 && item.verification.single_writer_sha_ok),
      tokens: tokenRows,
    };
  }
  const outFile = options.out ?? path.join(ROOT, 'evidence', 'mvp0c-s8-live-summary-2026-11-09.json');
  writeEvidence({ file: outFile, payload: { schema_version: 1, campaign: 'S8 MVP-0c — agrégat campagne réelle', generated_at: new Date().toISOString(), summary, campaigns: options.campaigns } });
  console.log(JSON.stringify(summary, null, 2));
  console.log(`Agrégat écrit : ${outFile}`);
  return 0;
}

function computeStats(values) {
  const xs = values.filter((value) => Number.isFinite(value)).slice().sort((a, b) => a - b);
  if (xs.length === 0) return null;
  const mid = Math.floor(xs.length / 2);
  return { n: xs.length, median: xs.length % 2 ? xs[mid] : (xs[mid - 1] + xs[mid]) / 2, min: xs[0], max: xs[xs.length - 1], samples: xs };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.mode === 'pilot') return runPilot(options);
  if (options.phase === 'prepare') return livePrepare(options);
  if (options.phase === 'collect') return liveCollect(options);
  if (options.phase === 'aggregate') return liveAggregate(options);
  throw new Error('phase inconnue');
}

main()
  .then((code) => process.exit(code))
  .catch((error) => {
    console.error(`\nERREUR FATALE S8 MVP-0c : ${error.stack ?? error.message}`);
    process.exit(1);
  });

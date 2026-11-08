#!/usr/bin/env node
//! Harnais S8 — benchmark comparatif worktrees Git / workspace partagé Ravum /
//! agents séquentiels (contrôle), sur une même fixture Rust et un même corpus de
//! 6 demandes d'édition.
//!
//! Les « agents » sont des ÉDITEURS SCRIPTÉS DÉTERMINISTES, pas des LLM : aucun
//! token IA n'est consommé ni mesuré, aucun temps de supervision humaine n'est
//! mesuré. Le harnais mesure uniquement des coûts mécaniques reproductibles.
//!
//! Usage :
//!   node benchmarks/s8/run-s8.mjs [--reps 5] [--scenarios A,B,C] [--keep]
//!                                 [--skip-build] [--out-dir <dir>]
//!                                 [--replica-root <dir>]
//!
//! Code de sortie : 0 si toutes les campagnes sont exploitables et conformes au
//! protocole ; 1 si une campagne casse le protocole (tests rouges, édition
//! indépendante perdue, publication non transactionnelle...).

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { CommandLog, run, runOk } from './lib/exec.mjs';
import { findPowerShell, machineInfo, probeSystemLoad } from './lib/env.mjs';
import { runRep } from './lib/campaign.mjs';
import { AGENTS, TASKS, corpusManifest } from './lib/tasks.mjs';
import { aggregateScenario, buildSchedule, consoleTable, markdownTable, rawTable, writeEvidence, writeMarkdownReport } from './lib/report.mjs';
import { ensureDir, listFiles, manifestOf, rmrf, round, sanitizer, sha256File } from './lib/util.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const FIXTURE_DIR = path.join(HERE, 'fixture');
const SAMPLER_SCRIPT = path.join(HERE, 'lib', 'sampler.ps1');

function parseArgs(argv) {
  const options = {
    reps: 5,
    scenarios: ['A', 'B', 'C'],
    keep: false,
    skipBuild: false,
    coordinatorRef: '7026f21',
    outDir: path.join(ROOT, 'evidence'),
    replicaRoot: null,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--reps') options.reps = Number(argv[++i]);
    else if (arg === '--scenarios') {
      options.scenarios = String(argv[++i])
        .split(',')
        .map((value) => value.trim().toUpperCase())
        .filter(Boolean);
    } else if (arg === '--keep') options.keep = true;
    else if (arg === '--skip-build') options.skipBuild = true;
    else if (arg === '--coordinator-ref') options.coordinatorRef = String(argv[++i]);
    else if (arg === '--out-dir') options.outDir = path.resolve(String(argv[++i]));
    else if (arg === '--replica-root') options.replicaRoot = path.resolve(String(argv[++i]));
    else if (arg === '--help' || arg === '-h') {
      console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(0, 22).join('\n'));
      process.exit(0);
    } else throw new Error(`argument inconnu: ${arg}`);
  }
  if (!Number.isInteger(options.reps) || options.reps < 1) throw new Error('--reps doit être un entier >= 1');
  for (const scenario of options.scenarios) {
    if (!['A', 'B', 'C'].includes(scenario)) throw new Error(`scénario inconnu: ${scenario}`);
  }
  return options;
}

/**
 * Construit le binaire `ravum-collab` depuis une RÉFÉRENCE GIT ÉPINGLÉE, dans un
 * dossier temporaire avec son propre `--target-dir`.
 *
 * Pourquoi : le dépôt est partagé et d'autres agents modifient/compilent le
 * crate `ravum-workspace-coordinator` pendant la campagne. Un binaire pris dans
 * `target/debug` de la racine ne serait ni reproductible ni comparable d'une
 * répétition à l'autre. Le benchmark épingle donc la référence donnée
 * (par défaut `7026f21`, HEAD de la mission S8), l'exporte par `git archive`
 * (sans toucher au worktree ni aux branches) et le compile à part.
 */
async function ensurePinnedCoordinator({ ref, skipBuild, commandLog, cacheRoot, sanitizePaths }) {
  const info = {
    pinned_ref: ref,
    archive_command: `git archive --format=tar -o <tmp>/src.tar ${ref}`,
    build_command_template: 'cargo build -p ravum-workspace-coordinator --bins --target-dir <tmp>/target',
    reason:
      'référence épinglée : le crate est modifié/compilé en parallèle par d’autres agents sur la machine partagée',
  };
  const rev = await run('git', ['rev-parse', `${ref}^{commit}`], {
    cwd: ROOT,
    log: commandLog,
    label: 'git-rev-parse-coordinator',
    timeoutMs: 120000,
  });
  if (rev.code !== 0) throw new Error(`référence coordinateur introuvable : ${ref}\n${rev.stderr}`);
  const commit = rev.stdout.trim();
  info.commit = commit;
  const baseDir = path.join(cacheRoot, commit.slice(0, 12));
  const srcDir = path.join(baseDir, 'src');
  const targetDir = path.join(baseDir, 'target');
  const exe = path.join(targetDir, 'debug', process.platform === 'win32' ? 'ravum-collab.exe' : 'ravum-collab');
  ensureDir(baseDir);

  if (!fs.existsSync(path.join(srcDir, 'Cargo.toml'))) {
    const tarFile = path.join(baseDir, 'src.tar');
    await runOk('git', ['archive', '--format=tar', '-o', tarFile, commit], {
      cwd: ROOT,
      log: commandLog,
      label: 'git-archive-coordinator',
      timeoutMs: 180000,
    });
    ensureDir(srcDir);
    await runOk('tar', ['-xf', tarFile, '-C', srcDir], {
      log: commandLog,
      label: 'tar-extract-coordinator',
      timeoutMs: 180000,
    });
    info.source_exported = true;
  }

  if (!skipBuild || !fs.existsSync(exe)) {
    const result = await run('cargo', ['build', '-p', 'ravum-workspace-coordinator', '--bins', '--target-dir', targetDir], {
      cwd: srcDir,
      env: { ...process.env, CARGO_TERM_COLOR: 'never', CARGO_NET_OFFLINE: 'true' },
      log: commandLog,
      label: 'cargo-build-coordinator',
      timeoutMs: 1800000,
      maxOut: 512 * 1024,
    });
    info.build = {
      ms: round(result.ms),
      exit_code: result.code,
      stdout_tail: result.stdout.slice(-1500),
      stderr_tail: result.stderr.slice(-1500),
      offline: true,
    };
    if (result.code !== 0) {
      throw new Error(`compilation du coordinateur épinglé (${commit.slice(0, 12)}) échouée :\n${result.stderr}`);
    }
  } else {
    info.build = { skipped: true, reason: '--skip-build et binaire épinglé déjà présent' };
  }
  if (!fs.existsSync(exe)) throw new Error(`binaire coordinateur épinglé introuvable : ${exe}`);
  info.exe = exe;
  info.exe_display = sanitizePaths(exe);
  info.sha256 = sha256File(exe);
  info.size_bytes = fs.statSync(exe).size;
  return info;
}

function validate(results) {
  const problems = [];
  for (const rep of results) {
    const where = `${rep.rep_id}`;
    if (!rep.tests.green) problems.push(`${where}: cargo test non vert (code ${rep.tests.exit_code}, ${rep.tests.passed ?? '?'} passés)`);
    if (!rep.verification.content_ok) problems.push(`${where}: contenu final différent de la référence (${rep.verification.mismatches.length} fichier(s))`);
    if (!rep.verification.status_clean_after_tests) problems.push(`${where}: arbre Git non propre après tests`);
    if (rep.lost.independent_missing.length > 0) problems.push(`${where}: éditions indépendantes perdues: ${rep.lost.independent_missing.join(',')}`);
    if (rep.scenario === 'A' || rep.scenario === 'B') {
      if (rep.lost.accepted_missing.length > 0) problems.push(`${where}: changements acceptés perdus: ${rep.lost.accepted_missing.join(',')}`);
      if (!rep.lost.conflict_winner_present) problems.push(`${where}: édition gagnante du conflit absente`);
      if (!rep.lost.rejected_edit_absent) problems.push(`${where}: édition concurrente perdante présente (le conflit n’a pas été arbitré)`);
      if (rep.conflicts.count !== 1) problems.push(`${where}: conflit contrôlé attendu=1, observé=${rep.conflicts.count}`);
    }
    if (rep.scenario === 'A') {
      if (rep.writes.worktrees !== AGENTS.length) problems.push(`${where}: ${rep.writes.worktrees} worktrees au lieu de ${AGENTS.length}`);
      if (rep.writes.merge_commits !== AGENTS.length) problems.push(`${where}: ${rep.writes.merge_commits} merges au lieu de ${AGENTS.length}`);
    }
    if (rep.scenario === 'B') {
      if (rep.publication.commit_count_since_base !== 1) problems.push(`${where}: ${rep.publication.commit_count_since_base} commits de publication au lieu de 1`);
      if (rep.publication.trailers_present.length !== 2) problems.push(`${where}: trailers GitWriter incomplets (${rep.publication.trailers_present.join(',')})`);
      if (!rep.publication.workspace_untouched_before_publish) problems.push(`${where}: le workspace a été modifié avant la publication (les agents ont écrit directement)`);
      if (!rep.publication.status_clean_after_publish) problems.push(`${where}: arbre non propre juste après publication`);
      const prefix = 'chore(collab): integrate';
      if (!String(rep.publication.subject || '').startsWith(prefix)) problems.push(`${where}: sujet de commit de publication inattendu: ${rep.publication.subject}`);
      if (rep.publication.paths.length !== 5) problems.push(`${where}: ${rep.publication.paths.length} chemins publiés au lieu de 5`);
    }
    if (rep.scenario === 'C') {
      if (rep.lost.silent_overwrites !== 1) problems.push(`${where}: écrasement silencieux attendu=1, observé=${rep.lost.silent_overwrites}`);
    }
  }
  return { ok: problems.length === 0, problems };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const startedAt = new Date();
  const dateStamp = startedAt.toISOString().slice(0, 10);
  const commandLog = new CommandLog(200);

  console.log('=== S8 — benchmark worktrees Git / workspace partagé Ravum / séquentiel ===');
  console.log('Agents : éditeurs SCRIPTÉS DÉTERMINISTES (pas des LLM) ; tokens IA et supervision humaine non mesurés.');
  console.log(`Scénarios : ${options.scenarios.join(', ')} — répétitions : ${options.reps}`);
  console.log(`Racine dépôt : ${ROOT}`);

  const ps = findPowerShell();
  console.log(`PowerShell : ${ps.path} (${ps.version})`);

  const coordinatorCacheRoot = path.join(os.tmpdir(), 'ravum-s8-coordinator-cache');
  const coordinatorInfo = await ensurePinnedCoordinator({
    ref: options.coordinatorRef,
    skipBuild: options.skipBuild,
    commandLog,
    cacheRoot: coordinatorCacheRoot,
    sanitizePaths: (value) => String(value).split(coordinatorCacheRoot).join('<coord-cache>'),
  });
  console.log(
    `Coordinateur épinglé : ${coordinatorInfo.commit.slice(0, 12)} (sha256 ${coordinatorInfo.sha256?.slice(0, 12)}…, build ${coordinatorInfo.build.ms} ms)` +
      (coordinatorInfo.build.skipped ? ' [cache]' : ''),
  );

  const rootRepoHead = await run('git', ['rev-parse', 'HEAD'], { cwd: ROOT, log: commandLog, label: 'git-root-head' });
  const rootRepoBranch = await run('git', ['rev-parse', '--abbrev-ref', 'HEAD'], { cwd: ROOT, log: commandLog, label: 'git-root-branch' });
  const rootRepoStatus = await run('git', ['status', '--porcelain'], { cwd: ROOT, log: commandLog, label: 'git-root-status', maxOut: 64 * 1024 });
  const rootRepo = {
    head: rootRepoHead.stdout.trim(),
    branch: rootRepoBranch.stdout.trim(),
    dirty_entries: rootRepoStatus.stdout.split(/\r?\n/).filter((line) => line.trim()).length,
    note: 'état au démarrage du run ; d’autres agents travaillent en parallèle sur cette machine et ce dépôt',
  };

  const machine = await machineInfo(ps);
  const fixtureFiles = listFiles(FIXTURE_DIR).filter((rel) => !rel.startsWith('target/'));
  const fixtureManifest = manifestOf(FIXTURE_DIR);
  const fixtureInfo = { path: 'benchmarks/s8/fixture', files: fixtureFiles, manifest: fixtureManifest };

  const stamp = `${startedAt.toISOString().replace(/[-:]/g, '').replace(/\..+/, '')}`;
  const replicaRoot = options.replicaRoot ?? path.join(os.tmpdir(), `ravum-s8-${stamp}`);
  ensureDir(replicaRoot);
  const sanitizePaths = sanitizer(replicaRoot);

  const ctx = {
    root: ROOT,
    fixtureDir: FIXTURE_DIR,
    fixtureFiles,
    coordinatorExe: coordinatorInfo.exe,
    ps,
    samplerScript: SAMPLER_SCRIPT,
    replicaRoot,
    keep: options.keep,
    sanitize: sanitizePaths,
  };

  const schedule = buildSchedule(options.reps, options.scenarios);
  const environment = {
    generated_at: startedAt.toISOString(),
    host: machine,
    versions: machine.versions,
    node: { version: process.version, platform: process.platform, arch: process.arch },
    toolchain: machine.versions,
    root_repo: rootRepo,
    coordinator_source: {
      pinned_ref: coordinatorInfo.pinned_ref,
      commit: coordinatorInfo.commit,
      binary_sha256: coordinatorInfo.sha256,
      binary_size_bytes: coordinatorInfo.size_bytes,
      exe_display: coordinatorInfo.exe_display,
      build: coordinatorInfo.build,
      archive_command: coordinatorInfo.archive_command,
      build_command_template: coordinatorInfo.build_command_template,
      reason: coordinatorInfo.reason,
    },
    fixture: fixtureInfo,
    corpus: corpusManifest(),
    schedule,
    protocol: {
      scenarios: ['A', 'B', 'C'],
      agents: AGENTS.length,
      tasks: TASKS.length,
      reps: options.reps,
      order: 'alterné : bloc A,B,C puis bloc C,B,A, etc.',
      success_criteria: [
        'cargo test de la fixture vert après intégration (A, B, C)',
        '0 édition indépendante perdue (A, B, C)',
        '0 changement accepté perdu (A, B) ; C exposé au contraire comme mesure',
        'conflit t4/t6 détecté et arbitré (A par git merge, B par décision CONFLICT)',
        'publication B réellement effectuée par le GitWriter transactionnel en 1 commit',
        'A utilise de vrais worktrees Git (3) et de vrais merges (3)',
      ],
      synthetic_limits: [
        'agents = éditeurs scriptés déterministes, PAS des LLM',
        'tokens IA non mesurés (aucun LLM impliqué)',
        'temps de supervision humaine non mesuré',
        'échantillon restreint (répétitions demandées : ' + options.reps + ')',
        'fixture locale et petite ; pas de réseau, pas de dépendances externes',
        'machine partagée avec d’autres agents : charge documentée, pas contrôlée',
      ],
    },
  };

  const loadProbes = [];
  const probe = async (label) => {
    const sample = await probeSystemLoad(ps, { log: commandLog });
    loadProbes.push({ label, ...sample });
    console.log(`[charge] ${label}: RAM libre ${round(sample.free_ram_bytes / (1024 * 1024), 0)} Mo, CPU ${sample.load_pct}%, ${sample.process_count} processus`);
  };

  const reps = [];
  const problems = [];
  try {
    await probe('début de campagne');
    for (let index = 0; index < schedule.length; index += 1) {
      const item = schedule[index];
      const label = `${item.scenario}${item.rep}`;
      process.stdout.write(`[${index + 1}/${schedule.length}] ${label} … `);
      try {
        const result = await runRep({ scenario: item.scenario, rep: item.rep, scheduleIndex: index + 1, ctx });
        reps.push(result);
        const status = result.functional_success ? 'succès' : result.scenario === 'C' ? 'succès contrôle (écrasement mesuré)' : 'ÉCHEC FONCTIONNEL';
        console.log(
          `${status} | total ${result.timings.t_total_ms} ms | edit ${result.timings.edit_phase_ms} ms | ` +
            `intégration ${result.timings.integration_phase_ms} ms | test ${result.timings.test_ms} ms | ` +
            `conflits ${result.conflicts.count} | RAM pic ${result.resources.sampler.peak_ws_mb} Mo | CPU ${result.resources.sampler.cpu_seconds} s`,
        );
      } catch (error) {
        console.log(`ERREUR HARNAIS: ${error.message.split('\n')[0]}`);
        problems.push(`${label}: exception harnais: ${error.stack ?? error.message}`);
        throw error;
      }
      if (index === Math.floor(schedule.length / 2) - 1) await probe('mi-campagne');
    }
    await probe('fin de campagne');
  } finally {
    if (!options.keep) {
      try {
        rmrf(replicaRoot);
      } catch (error) {
        console.warn(`[nettoyage] impossible de supprimer ${replicaRoot}: ${error.message}`);
      }
    } else {
      console.log(`Répliques conservées dans : ${replicaRoot}`);
    }
  }

  const scenarioSummaries = {};
  for (const scenario of options.scenarios) {
    const scenarioReps = reps.filter((rep) => rep.scenario === scenario);
    scenarioSummaries[scenario] = aggregateScenario(scenarioReps);
  }
  const validation = validate(reps);
  const summary = { scenarios: scenarioSummaries };
  const markdown = markdownTable(summary);
  const rawMarkdown = rawTable(reps);

  const written = writeEvidence({
    evidenceDir: options.outDir,
    dateStamp,
    environment,
    schedule,
    reps,
    scenarioSummaries,
    validation: { ...validation, protocol_ok: validation.ok && problems.length === 0 },
    loadProbes,
    coordinatorInfo,
    markdown,
    rawMarkdown,
  });
  const reportFile = path.join(HERE, 'results', 'latest.md');
  writeMarkdownReport({
    file: reportFile,
    summary,
    markdown,
    rawMarkdown,
    environment,
    validation: { ...validation, protocol_ok: validation.ok && problems.length === 0 },
    loadProbes,
  });

  console.log('\n=== Tableau comparatif (médiane [min–max] (IQR)) ===');
  console.log(consoleTable(summary));
  console.log('\nPreuves :');
  for (const file of Object.values(written.files)) console.log(`  - ${file}`);
  console.log(`  - ${written.summaryFile}`);
  console.log(`  - ${reportFile}`);

  if (problems.length > 0) {
    console.error('\n=== PROBLÈMES DE PROTOCOLE ===');
    for (const problem of problems) console.error(`  - ${problem}`);
  }
  if (!validation.ok) {
    console.error('\n=== VALIDATION ÉCHOUÉE ===');
    for (const problem of validation.problems) console.error(`  - ${problem}`);
  }
  const ok = validation.ok && problems.length === 0;
  console.log(`\nRésultat global : ${ok ? 'OK (campagnes exploitables et conformes)' : 'ÉCHEC DU PROTOCOLE'}`);
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(`\nERREUR FATALE S8 : ${error.stack ?? error.message}`);
  process.exit(1);
});

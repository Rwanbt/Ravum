#!/usr/bin/env node
//! Agent-command du PILOTE : éditeur scripté déterministe (PAS un LLM).
//!
//! Contrat d'agent-command (identique pour les subagents réels, à ceci près
//! qu'ils écrivent le reçu eux-mêmes avec `lib/write-receipt.mjs`) :
//!   node lib/agent-scripted.mjs --agent <id> --workspace <dir> \
//!        --tasks t1,t2 --strategy A|B|C --base <hashes.json> --receipt <out.json> \
//!        [--collision-task t3] [--collision-delay-ms 150]
//!
//! Effets : applique les éditions déterministes des tâches assignées dans
//! `<workspace>` (jamais les fichiers protégés), puis écrit un reçu JSON.
//! En stratégie B (répertoire partagé), un petit délai optionnel sur une tâche
//! dont le fichier est partagé provoque une collision d'écriture RÉELLE, pour
//! exercer la détection de conflits ; l'éditeur utilise une garde de version
//! optimiste (relecture + réessai) pour ne perdre aucune édition.

import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { sha256Hex, sleep } from './fsutil.mjs';
import { buildReceipt, writeReceipt } from './receipt.mjs';
import { taskById } from './tasks.mjs';

function flag(args, name) {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : null;
}

const args = process.argv.slice(2);
const agent = flag(args, '--agent');
const workspace = flag(args, '--workspace');
const tasksCsv = flag(args, '--tasks');
const strategy = flag(args, '--strategy') ?? 'A';
const basePath = flag(args, '--base');
const receiptPath = flag(args, '--receipt');
const collisionTask = flag(args, '--collision-task') ?? '';
const collisionDelayMs = Number(flag(args, '--collision-delay-ms') ?? '0');

if (!agent || !workspace || !tasksCsv || !basePath || !receiptPath) {
  console.error('usage: agent-scripted.mjs --agent <id> --workspace <dir> --tasks <csv> --base <hashes.json> --receipt <out.json> [--strategy] [--collision-task] [--collision-delay-ms]');
  process.exit(2);
}
const taskIds = tasksCsv.split(',').map((value) => value.trim()).filter(Boolean);
const baseHashes = JSON.parse(fs.readFileSync(basePath, 'utf8'));

const taskResults = [];
const started = performance.now();
let failed = false;

for (const taskId of taskIds) {
  const task = taskById(taskId);
  const file = path.join(workspace, task.path);
  const taskStart = performance.now();
  let attempts = 0;
  let conflicts = 0;
  let blockedMs = 0;
  let applied = false;
  let lastError = null;

  while (attempts < 6 && !applied) {
    attempts += 1;
    try {
      const before = fs.readFileSync(file, 'utf8');
      const occurrences = before.split(task.oldSnippet).length - 1;
      if (occurrences !== 1) throw new Error(`${task.path}: ${occurrences} occurrence(s) de l'extrait de ${taskId}`);
      const next = before.replace(task.oldSnippet, task.newSnippet);

      if (strategy === 'B' && task.id === collisionTask && collisionDelayMs > 0) {
        // Fenêtre de collision déterministe : on relit APRÈS le délai ; si un
        // autre agent a écrit entre-temps, c'est un conflit détecté, pas une
        // perte silencieuse.
        const waitStart = performance.now();
        await sleep(collisionDelayMs);
        blockedMs += performance.now() - waitStart;
        const afterWait = fs.readFileSync(file, 'utf8');
        if (afterWait !== before) {
          conflicts += 1;
          continue;
        }
        fs.writeFileSync(file, next, 'utf8');
      } else {
        fs.writeFileSync(file, next, 'utf8');
      }

      const after = fs.readFileSync(file, 'utf8');
      if (after === next) {
        applied = true;
      } else {
        // Un autre écrivain a écrasé notre édition : conflit détecté, réessai.
        conflicts += 1;
      }
    } catch (error) {
      lastError = String(error);
      break;
    }
  }

  const markersPresent = task.markers.every((marker) => fs.readFileSync(file, 'utf8').includes(marker));
  if (!applied || !markersPresent) failed = true;
  taskResults.push({
    id: taskId,
    path: task.path,
    attempts,
    conflicts,
    blocked_ms: Math.round(blockedMs * 1000) / 1000,
    ms: Math.round((performance.now() - taskStart) * 1000) / 1000,
    applied,
    markers_present: markersPresent,
    ...(lastError ? { error: lastError } : {}),
  });
}

const receipt = buildReceipt({
  agent,
  workspace,
  baseHashes,
  taskIds,
  extra: {
    written_by: 'agent-scripted.mjs',
    strategy,
    edit_ms: Math.round((performance.now() - started) * 1000) / 1000,
    tasks: taskResults,
    conflicts: taskResults.reduce((acc, task) => acc + task.conflicts, 0),
    attempts: taskResults.reduce((acc, task) => acc + task.attempts, 0),
    blocked_ms: Math.round(taskResults.reduce((acc, task) => acc + task.blocked_ms, 0) * 1000) / 1000,
    deterministic_scripted_agent: true,
    agent_self_rss_bytes: process.memoryUsage().rss,
    agent_self_heap_used_bytes: process.memoryUsage().heapUsed,
  },
});
writeReceipt(receiptPath, receipt);
console.log(JSON.stringify({ agent, status: receipt.status, conflicts: receipt.conflicts, edit_ms: receipt.edit_ms }));
process.exit(failed ? 1 : 0);

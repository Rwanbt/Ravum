//! Attribution des tokens DSH par agent — campagne S8 MVP-0c réelle.
//!
//! Deux sources, jamais mélangées :
//! 1. `tools/agent-usage/usage.mjs` agrège les compteurs `data.usage` enregistrés
//!    par le harnais DSH pour chaque message assistant (source de vérité des
//!    tokens : inputTokens, outputTokens, totalTokens, cacheReadTokens).
//! 2. Les descripteurs de session DSH (`subagent/descriptor.data.label`) et le
//!    premier message utilisateur contiennent le marqueur unique placé dans le
//!    prompt (`S8MVP0C-AGENT-<n>`), ce qui relie une session à un agent.
//!
//! Aucun contenu de message n'est conservé ni imprimé : seuls le libellé du
//! descripteur, le marqueur trouvé (via `matched_via`) et les compteurs le sont.
//!
//! Si une session ne peut pas être attribuée, elle n'est PAS répartie au hasard :
//! elle apparaît dans `unmatched_sessions` (compteurs inclus) et la campagne est
//! marquée incomplète.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { run } from './exec.mjs';
import { round } from './fsutil.mjs';

const TOKEN_KEYS = ['inputTokens', 'outputTokens', 'totalTokens', 'cacheReadTokens', 'processedTokens'];

export function dshHome() {
  return process.env.DSH_HOME || path.join(process.env.USERPROFILE || process.env.HOME || '.', '.dsh');
}

/** Même convention de slug que tools/agent-usage/usage.mjs. */
export function slugifyWorkspace(p) {
  const normalized = path.resolve(p).replace(/[\\/]+$/, '');
  const body = normalized.replace(/:/g, '').replace(/[\\/]+/g, '-');
  return `--${body}--`;
}

export function sessionsRoot() {
  return path.join(dshHome(), 'sessions');
}

export function sessionFileFor(session) {
  return path.join(sessionsRoot(), slugifyWorkspace(process.cwd()), session, 'session.v4.jsonl.zstd');
}

/** Exécute `tools/agent-usage/usage.mjs --json` (read-only) et renvoie le rapport. */
export async function loadUsageReport({ repoRoot, workspaceRoot, log = null }) {
  const tool = path.join(repoRoot, 'tools', 'agent-usage', 'usage.mjs');
  const result = await run(process.execPath, [tool, '--json', '--workspace', workspaceRoot], {
    cwd: repoRoot,
    timeoutMs: 300000,
    maxOut: 4 * 1024 * 1024,
    log,
    label: 'usage-report',
  });
  if (result.code !== 0) return { ok: false, error: `usage.mjs code ${result.code}: ${result.stderr.slice(-500)}` };
  try {
    return { ok: true, command: `node ${path.relative(repoRoot, tool).replaceAll('\\', '/')} --json --workspace ${workspaceRoot}`, report: JSON.parse(result.stdout) };
  } catch (error) {
    return { ok: false, error: `JSON usage invalide: ${String(error)}` };
  }
}

/**
 * Lit l'en-tête d'une session DSH sans conserver le contenu.
 * @returns {{sessionEvent:object|null, descriptorLabel:string|null, firstUserText:string|null, error?:string}}
 */
export function readSessionHeader(compressedFile) {
  try {
    const raw = zlib.zstdDecompressSync(fs.readFileSync(compressedFile)).toString('utf8');
    let sessionEvent = null;
    let descriptorLabel = null;
    let firstUserText = null;
    for (const line of raw.split('\n')) {
      if (!line.trim()) continue;
      let event;
      try { event = JSON.parse(line); } catch { continue; }
      if (event.type === 'session') sessionEvent = event;
      if (event.type === 'subagent/descriptor' && descriptorLabel === null) {
        descriptorLabel = event.data?.label ?? null;
      }
      if (event.type === 'user/message' && firstUserText === null) {
        const content = event.data?.content;
        if (Array.isArray(content)) firstUserText = content.filter((part) => part?.type === 'text').map((part) => part.text).join('\n');
      }
      if (sessionEvent && descriptorLabel !== null && firstUserText !== null) break;
    }
    return { sessionEvent, descriptorLabel, firstUserText, error: null };
  } catch (error) {
    return { sessionEvent: null, descriptorLabel: null, firstUserText: null, error: String(error) };
  }
}

function emptyTotals() {
  const totals = { sessions: 0, assistantMessages: 0, wallMs: 0 };
  for (const key of TOKEN_KEYS) totals[key] = 0;
  return totals;
}

function addSessionToTotals(totals, session) {
  totals.sessions += 1;
  totals.assistantMessages += Number(session.assistantMessages) || 0;
  totals.wallMs += Number(session.wallMs) || 0;
  for (const key of TOKEN_KEYS) totals[key] += Number(session[key]) || 0;
}

/**
 * Attribue les sessions DSH aux agents via marqueurs.
 *
 * @param {{repoRoot:string, workspaceRoot:string, markers:Array<{agent:string, marker:string}>,
 *   windowStartMs?:number|null, windowEndMs?:number|null, toleranceMs?:number,
 *   usageReport?:object|null, log?:object|null}} input
 */
export async function attributeUsage({
  repoRoot,
  workspaceRoot,
  markers,
  windowStartMs = null,
  windowEndMs = null,
  toleranceMs = 120000,
  usageReport = null,
  log = null,
}) {
  const loaded = usageReport ? { ok: true, report: usageReport, command: 'fourni par l’appelant' } : await loadUsageReport({ repoRoot, workspaceRoot, log });
  if (!loaded.ok) {
    return { measured: false, reason: loaded.error, agents: {}, unmatched_sessions: [], limitations: [] };
  }
  const report = loaded.report;
  const slug = slugifyWorkspace(workspaceRoot);
  const root = path.join(sessionsRoot(), slug);
  const candidates = [];
  const unmatched = [];
  const agents = {};
  for (const marker of markers) {
    agents[marker.agent] = { marker: marker.marker, sessions: [], totals: emptyTotals() };
  }

  for (const session of report.sessions ?? []) {
    const createdAt = session.createdAt ?? null;
    const inWindow =
      (windowStartMs === null || (createdAt !== null && createdAt >= windowStartMs - toleranceMs)) &&
      (windowEndMs === null || createdAt === null || createdAt <= windowEndMs + toleranceMs);
    if (!inWindow) continue;
    const header = readSessionHeader(path.join(root, session.session, 'session.v4.jsonl.zstd'));
    const label = header.descriptorLabel ?? '';
    const prompt = header.firstUserText ?? '';
    let matched = null;
    for (const marker of markers) {
      if (label.includes(marker.marker)) { matched = { agent: marker.agent, via: 'subagent/descriptor.label' }; break; }
      if (prompt.includes(marker.marker)) { matched = { agent: marker.agent, via: 'premier message utilisateur' }; break; }
    }
    const baseRow = {
      session: session.session,
      delegationDepth: session.delegationDepth ?? header.sessionEvent?.delegationDepth ?? null,
      origin: header.sessionEvent?.origin ?? null,
      parentSession: header.sessionEvent?.parentSession ?? null,
      createdAt,
      label: label || null,
      assistantMessages: session.assistantMessages ?? 0,
      wallMs: session.wallMs ?? null,
      inputTokens: session.inputTokens ?? 0,
      outputTokens: session.outputTokens ?? 0,
      totalTokens: session.totalTokens ?? 0,
      cacheReadTokens: session.cacheReadTokens ?? 0,
      processedTokens: session.processedTokens ?? 0,
      models: session.models ?? [],
    };
    if (matched) {
      agents[matched.agent].sessions.push({ ...baseRow, matched_via: matched.via });
      addSessionToTotals(agents[matched.agent].totals, session);
    } else if (session.delegationDepth !== null && session.delegationDepth !== undefined && session.delegationDepth > 0) {
      // Seules les sessions de sous-agents non attribuées sont listées ; les
      // sessions racine (Lead) sont hors périmètre par construction.
      unmatched.push(baseRow);
    }
  }

  const matchedAgents = Object.values(agents).filter((entry) => entry.sessions.length > 0).length;
  const allAgentsMatched = markers.every((marker) => agents[marker.agent].sessions.length > 0);
  return {
    measured: true,
    source: 'tools/agent-usage/usage.mjs (+ descripteurs de session DSH pour l’attribution)',
    command: loaded.command,
    dsh_home: dshHome(),
    workspace_slug: slug,
    usage_generated_at: report.generatedAt ?? null,
    window: { start_ms: windowStartMs, end_ms: windowEndMs, tolerance_ms: toleranceMs },
    markers,
    agents,
    agents_matched: matchedAgents,
    all_agents_matched: allAgentsMatched,
    unmatched_sessions: unmatched,
    unmatched_count: unmatched.length,
    limitations: [
      'tokens = compteurs enregistrés par le harnais DSH par message assistant ; aucune estimation',
      'cacheReadTokens est déclaré par l’outil comme sous-ensemble d’inputTokens ; processedTokens = input+output+cacheRead (formule de usage.mjs)',
      'wallMs d’une session couvre le premier au dernier événement, temps d’inactivité inclus',
      'aucun coût monétaire n’est exposé par le harnais',
      'une session non attribuée n’est jamais répartie au hasard : elle est listée à part',
      'le marqueur doit être présent dans la description de spawn (ou le premier message utilisateur)',
    ],
  };
}

export { TOKEN_KEYS, round };

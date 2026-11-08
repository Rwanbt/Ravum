#!/usr/bin/env node
// Agent usage reporter — reads the DeepSeek Harness (DSH) local session store and
// prints the per-session token usage it already records. No external dependency,
// no API key, no network access.
//
// Why it exists: the S8 multi-agent campaign must report tokens and wall-clock
// time for real agents. The DSH harness records, for every assistant message, a
// `data.usage` object `{inputTokens, outputTokens, totalTokens, cacheReadTokens}`
// inside `%DSH_HOME%/sessions/<workspace-slug>/<session>/session.v4.jsonl.zstd`
// (zstd-compressed JSON lines). This tool decompresses those files with Node's
// built-in zstd support and aggregates the usage counters. It never prints model
// messages, tool arguments or credentials.
//
// Usage:
//   node tools/agent-usage/usage.mjs [--json] [--workspace <path|slug>]
//        [--session <id>] [--all-workspaces]
//
// Exit codes: 0 = report produced, 2 = usage error, 3 = session store missing.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const args = process.argv.slice(2);
const opts = { json: false, workspace: process.cwd(), session: null, allWorkspaces: false };

for (let i = 0; i < args.length; i += 1) {
  const arg = args[i];
  if (arg === '--json') opts.json = true;
  else if (arg === '--all-workspaces') opts.allWorkspaces = true;
  else if (arg === '--workspace') opts.workspace = args[++i];
  else if (arg === '--session') opts.session = args[++i];
  else {
    console.error(`usage: node tools/agent-usage/usage.mjs [--json] [--workspace <path|slug>] [--session <id>] [--all-workspaces]`);
    process.exit(2);
  }
}

const dshHome = process.env.DSH_HOME || path.join(process.env.USERPROFILE || process.env.HOME || '.', '.dsh');
const sessionsRoot = path.join(dshHome, 'sessions');
if (!fs.existsSync(sessionsRoot)) {
  console.error(`session store not found: ${sessionsRoot}`);
  process.exit(3);
}

/** DSH stores one directory per workspace: `--<drive>-<segments>--` (separators
 * become `-`, the drive colon is dropped). Fall back to a suffix match so a
 * path normalization difference cannot silently produce an empty report. */
function slugifyWorkspace(p) {
  const normalized = path.resolve(p).replace(/[\\/]+$/, '');
  const body = normalized.replace(/:/g, '').replace(/[\\/]+/g, '-');
  return `--${body}--`;
}

function resolveWorkspaceDir(slug) {
  const direct = path.join(sessionsRoot, slug);
  if (fs.existsSync(direct)) return slug;
  const wanted = slug.replace(/^-+|-+$/g, '').toLowerCase();
  const match = fs
    .readdirSync(sessionsRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .find((name) => name.replace(/^-+|-+$/g, '').toLowerCase() === wanted);
  return match || slug;
}

function listSessionFiles() {
  const files = [];
  const workspaces = opts.allWorkspaces
    ? fs.readdirSync(sessionsRoot, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name)
    : [resolveWorkspaceDir(slugifyWorkspace(opts.workspace))];
  for (const workspace of workspaces) {
    const dir = path.join(sessionsRoot, workspace);
    if (!fs.existsSync(dir)) continue;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const sessionDir = path.join(dir, entry.name);
      const compressed = path.join(sessionDir, 'session.v4.jsonl.zstd');
      if (!fs.existsSync(compressed)) continue;
      if (opts.session && !entry.name.includes(opts.session)) continue;
      files.push({ workspace, session: entry.name, compressed });
    }
  }
  return files.sort((a, b) => a.session.localeCompare(b.session));
}

/** Aggregate one session file. Returns null when the file cannot be decoded. */
function aggregate(file) {
  let text;
  try {
    text = zlib.zstdDecompressSync(fs.readFileSync(file.compressed)).toString('utf8');
  } catch (error) {
    return { ...file, error: String(error) };
  }
  const lines = text.split('\n').filter((line) => line.trim().length > 0);
  let meta = null;
  const totals = { inputTokens: 0, outputTokens: 0, totalTokens: 0, cacheReadTokens: 0 };
  let assistantMessages = 0;
  let firstTime = null;
  let lastTime = null;
  const perModel = new Map();
  for (const line of lines) {
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (!meta && event.type && event.cwd !== undefined) meta = event;
    if (typeof event.time === 'number') {
      if (firstTime === null || event.time < firstTime) firstTime = event.time;
      if (lastTime === null || event.time > lastTime) lastTime = event.time;
    }
    if (event.type !== 'assistant/message') continue;
    const usage = event.data && event.data.usage;
    if (!usage) continue;
    // `data.stream.*.chunk.usage` repeats the same counters; only `data.usage`
    // is aggregated, so each assistant message is counted exactly once.
    assistantMessages += 1;
    for (const key of Object.keys(totals)) {
      if (typeof usage[key] === 'number') totals[key] += usage[key];
    }
    const model = event.data?.message?.source?.model || event.data?.source?.model || 'unknown';
    const row = perModel.get(model) || { model, messages: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };
    row.messages += 1;
    row.inputTokens += usage.inputTokens || 0;
    row.outputTokens += usage.outputTokens || 0;
    row.cacheReadTokens += usage.cacheReadTokens || 0;
    perModel.set(model, row);
  }
  const wallMs = firstTime !== null && lastTime !== null ? lastTime - firstTime : null;
  return {
    workspace: file.workspace,
    session: file.session,
    delegationDepth: meta ? meta.delegationDepth ?? null : null,
    agentPreset: meta ? meta.agentPreset ?? null : null,
    createdAt: meta ? meta.createdAt ?? null : null,
    assistantMessages,
    wallMs,
    ...totals,
    processedTokens: totals.inputTokens + totals.outputTokens + totals.cacheReadTokens,
    models: [...perModel.values()].sort((a, b) => b.messages - a.messages),
  };
}

const rows = listSessionFiles().map(aggregate);
const sessions = rows.filter((row) => !row.error);
const failed = rows.filter((row) => row.error);

const report = {
  dshHome,
  generatedAt: new Date().toISOString(),
  scope: opts.allWorkspaces ? 'all-workspaces' : slugifyWorkspace(opts.workspace),
  sessions: sessions
    .slice()
    .sort((a, b) => (b.totalTokens || 0) - (a.totalTokens || 0)),
  totals: sessions.reduce(
    (acc, row) => {
      acc.sessions += 1;
      acc.assistantMessages += row.assistantMessages;
      acc.inputTokens += row.inputTokens;
      acc.outputTokens += row.outputTokens;
      acc.totalTokens += row.totalTokens;
      acc.cacheReadTokens += row.cacheReadTokens;
      acc.processedTokens += row.processedTokens;
      return acc;
    },
    { sessions: 0, assistantMessages: 0, inputTokens: 0, outputTokens: 0, totalTokens: 0, cacheReadTokens: 0, processedTokens: 0 },
  ),
  failures: failed.map((row) => ({ session: row.session, error: row.error })),
  limitations: [
    'usage is the harness-recorded per-message counter; cacheReadTokens is a subset of inputTokens',
    'wallMs is the span between first and last recorded event, including idle time',
    'no monetary cost is exposed by the harness',
    'a session file may span several human turns; per-campaign accounting must filter sessions or timestamps',
  ],
};

if (opts.json) {
  process.stdout.write(JSON.stringify(report, null, 2) + '\n');
} else {
  console.log(`DSH usage — ${report.scope} — ${report.generatedAt}`);
  console.log(
    `sessions=${report.totals.sessions} assistant_messages=${report.totals.assistantMessages} ` +
      `input=${report.totals.inputTokens} output=${report.totals.outputTokens} ` +
      `cache_read=${report.totals.cacheReadTokens} processed=${report.totals.processedTokens}`,
  );
  for (const row of report.sessions) {
    console.log(
      `- ${row.session}  depth=${row.delegationDepth}  msgs=${row.assistantMessages}  ` +
        `in=${row.inputTokens} out=${row.outputTokens} cache_read=${row.cacheReadTokens} ` +
        `processed=${row.processedTokens}  wall_ms=${row.wallMs ?? 'null'}`,
    );
  }
  if (failed.length > 0) console.log(`unreadable sessions: ${failed.length}`);
}

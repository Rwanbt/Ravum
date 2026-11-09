#!/usr/bin/env node
//! Écrit le reçu d'un agent (utilisé par les SUBAGENTS RÉELS de la campagne).
//!
//! Le harnais de préparation place `receipt-base.json` (hashes de base des
//! fichiers autorisés) et indique le chemin du reçu dans le prompt de l'agent.
//! L'agent n'a qu'à exécuter :
//!
//!   node benchmarks/s8-mvp0c/lib/write-receipt.mjs \
//!     --agent agent-1 --workspace <dir> --base <receipt-base.json> --receipt <reçu.json>
//!
//! Le reçu ne contient que des chemins et des sha256 (aucun contenu de message).

import fs from 'node:fs';
import { buildReceipt, writeReceipt } from './receipt.mjs';
import { agentById } from './tasks.mjs';

function flag(args, name) {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : null;
}

const args = process.argv.slice(2);
const agent = flag(args, '--agent');
const workspace = flag(args, '--workspace');
const basePath = flag(args, '--base');
const receiptPath = flag(args, '--receipt');
if (!agent || !workspace || !basePath || !receiptPath) {
  console.error('usage: write-receipt.mjs --agent <id> --workspace <dir> --base <receipt-base.json> --receipt <out.json>');
  process.exit(2);
}
if (!fs.existsSync(workspace)) {
  console.error(`workspace absent: ${workspace}`);
  process.exit(2);
}
const baseHashes = JSON.parse(fs.readFileSync(basePath, 'utf8'));
const taskIds = agentById(agent).tasks;
const receipt = buildReceipt({
  agent,
  workspace,
  baseHashes,
  taskIds,
  extra: { written_by: 'write-receipt.mjs' },
});
writeReceipt(receiptPath, receipt);
console.log(JSON.stringify({ agent, receipt: receiptPath, status: receipt.status }));
process.exit(receipt.status === 'done' ? 0 : 1);

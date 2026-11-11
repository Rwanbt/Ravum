//! Reçus d'agent : contrat commun entre l'éditeur scripté (pilote) et les
//! subagents réels (campagne MVP-0c).
//!
//! Un reçu décrit ce qu'un agent a modifié, avec les hashes sha256 avant/après :
//! c'est ce qui permet de détecter un changement perdu sans jamais lire le
//! contenu des messages d'un agent.

import fs from 'node:fs';
import path from 'node:path';
import { sha256Hex, writeJsonAtomic } from './fsutil.mjs';
import { taskById, tasksOf } from './tasks.mjs';

export const RECEIPT_SCHEMA_VERSION = 1;

/**
 * Construit un reçu à partir des fichiers actuellement sur disque.
 * @param {{agent:string, workspace:string, baseHashes:Object<string,string>,
 *   taskIds:string[], extra?:object}} input
 */
export function buildReceipt({ agent, workspace, baseHashes, taskIds, extra = {} }) {
  const paths = [...new Set(taskIds.map((id) => taskById(id).path))];
  const changed = paths.map((rel) => {
    const abs = path.join(workspace, rel);
    const finalText = fs.existsSync(abs) ? fs.readFileSync(abs, 'utf8') : null;
    const finalHash = finalText === null ? null : sha256Hex(finalText);
    const baseHash = baseHashes[rel] ?? null;
    return {
      path: rel,
      base_sha256: baseHash,
      final_sha256: finalHash,
      changed_vs_base: finalHash !== baseHash,
      present: finalText !== null,
    };
  });
  return {
    schema_version: RECEIPT_SCHEMA_VERSION,
    kind: 's8-mvp0c-agent-receipt',
    agent,
    task_ids: taskIds,
    changed,
    status: changed.every((entry) => entry.changed_vs_base) ? 'done' : 'incomplete',
    created_at: new Date().toISOString(),
    ...extra,
  };
}

export function writeReceipt(receiptPath, receipt) {
  writeJsonAtomic(receiptPath, receipt);
}

/** Lit un reçu et vérifie sa forme minimale. */
export function readReceipt(receiptPath) {
  if (!fs.existsSync(receiptPath)) return null;
  const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
  if (receipt?.kind !== 's8-mvp0c-agent-receipt') throw new Error(`${receiptPath}: reçu de type inattendu`);
  if (!Array.isArray(receipt.changed)) throw new Error(`${receiptPath}: champ changed manquant`);
  return receipt;
}

/** Base de hashes que les agents doivent recevoir (fichiers autorisés). */
export function baseHashesForAgent(baseContents, agentId) {
  const hashes = {};
  for (const task of tasksOf(agentId)) hashes[task.path] = sha256Hex(baseContents[task.path]);
  return hashes;
}

//! Vérifications objectives S8 MVP-0c : contenu exact, marqueurs, fichiers
//! protégés, changements perdus, détection d'écrasement.

import path from 'node:path';
import { sha256Hex } from './fsutil.mjs';
import { merge3 } from './merge.mjs';
import { TASKS, PROTECTED_FILES, taskById } from './tasks.mjs';

/** Compare les contenus finaux à la référence exacte (mode pilote). */
export function referenceCheck(finalContents, reference) {
  const mismatches = [];
  for (const [rel, expected] of Object.entries(reference)) {
    const actual = finalContents[rel];
    if (actual !== expected) {
      mismatches.push({
        path: rel,
        expected_sha256: sha256Hex(expected),
        actual_sha256: actual === undefined ? null : sha256Hex(actual),
      });
    }
  }
  return { ok: mismatches.length === 0, mismatches };
}

/** Vérifie que chaque marqueur de tâche est présent dans les fichiers finaux. */
export function markerCheck(finalContents) {
  const perTask = {};
  for (const task of TASKS) {
    const text = finalContents[task.path];
    const missing = task.markers.filter((marker) => typeof text !== 'string' || !text.includes(marker));
    perTask[task.id] = { present: missing.length === 0, missing };
  }
  return perTask;
}

/** Fichiers protégés modifiés par rapport à la base (doit être vide). */
export function protectedCheck(finalContents, baseContents) {
  const changed = [];
  for (const rel of PROTECTED_FILES) {
    if (finalContents[rel] !== baseContents[rel]) changed.push(rel);
  }
  return changed;
}

/**
 * Intégration A : fusionne les versions finales des agents dans la base, en
 * mémoire, avec la fusion 3-way ligne à ligne (aucune commande Git).
 *
 * @returns {{contents:Object<string,string>, conflicts:Array, merge_ms:number, merges:number}}
 */
export function integrateAgentResults({ baseContents, agentResults }) {
  const contents = { ...baseContents };
  const conflicts = [];
  let mergeMs = 0;
  let merges = 0;
  for (const agent of agentResults) {
    for (const [rel, finalText] of Object.entries(agent.finalContents)) {
      if (finalText === baseContents[rel]) continue; // rien modifié
      merges += 1;
      if (contents[rel] === baseContents[rel]) {
        contents[rel] = finalText;
        continue;
      }
      const started = performance.now();
      const merged = merge3(baseContents[rel], contents[rel], finalText);
      mergeMs += performance.now() - started;
      contents[rel] = merged.content;
      for (const conflict of merged.conflicts) {
        conflicts.push({
          path: rel,
          integrated_side: 'ours',
          lost_side_agent: agent.agent,
          start_line: conflict.start + 1,
          end_line: conflict.end,
          resolution: 'keep-integrated-side',
        });
      }
    }
  }
  return { contents, conflicts, merge_ms: mergeMs, merges };
}

/**
 * Détecte les écritures écrasées dans un répertoire partagé : pour chaque
 * fichier, compare le contenu final au `final_sha256` de chaque reçu.
 * Une écriture est « écrasée » si son hash final n'est pas celui du fichier.
 *
 * Limite : une fusion textuelle réussie produit un hash différent des deux
 * reçus ; ce détecteur ne sert donc qu'à la sonde de contrôle et aux fichiers
 * à écrivain unique. Les tâches réelles sont vérifiées par la référence exacte
 * et par `cargo test`.
 */
export function detectClobberedWrites({ intents, finalContents }) {
  const byPath = new Map();
  for (const intent of intents) {
    if (!byPath.has(intent.path)) byPath.set(intent.path, []);
    byPath.get(intent.path).push(intent);
  }
  const results = [];
  for (const [rel, writers] of byPath.entries()) {
    const finalText = finalContents[rel];
    const finalSha = finalText === undefined ? null : sha256Hex(finalText);
    results.push({
      path: rel,
      writers: writers.map((writer) => writer.agent),
      final_sha256: finalSha,
      clobbered: writers.filter((writer) => writer.final_sha256 !== finalSha).map((writer) => writer.agent),
      final_equals_last_writer: writers.length > 0 && writers[writers.length - 1].final_sha256 === finalSha,
    });
  }
  return {
    files: results,
    any_clobbered: results.some((result) => result.clobbered.length > 0),
    clobbered_writers: results.flatMap((result) => result.clobbered),
  };
}

/** Nombre de changements attendus présents (pourcentage de réussite). */
export function changePresence(finalContents, reference) {
  let present = 0;
  for (const [rel, expected] of Object.entries(reference)) {
    if (finalContents[rel] === expected) present += 1;
  }
  return { present, total: Object.keys(reference).length };
}

/** Chemin absolu sécurisé sous une racine. */
export function inside(root, rel) {
  const abs = path.resolve(root, rel);
  const resolvedRoot = path.resolve(root);
  if (abs !== resolvedRoot && !abs.startsWith(`${resolvedRoot}${path.sep}`)) {
    throw new Error(`chemin hors racine: ${rel}`);
  }
  return abs;
}

export { taskById };

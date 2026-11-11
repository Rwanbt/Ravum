//! Corpus S8 MVP-0c : 4 tâches indépendantes sur une fixture Rust réelle.
//!
//! Définitions figées AVANT le pilote (pré-enregistrement) :
//! - t1 : `stats::mean`    — agent-1
//! - t2 : `text::slugify`  — agent-1
//! - t3 : `stats::median`  — agent-2  (même fichier que t1, fonction différente)
//! - t4 : `dates::parse_iso_date` — agent-3
//!
//! Chaque tâche remplace un extrait exact par une implémentation exacte. Les
//! agents du pilote sont des éditeurs SCRIPTÉS DÉTERMINISTES (pas des LLM) :
//! aucun token IA n'est mesuré au pilote.

import { sha256Hex } from './fsutil.mjs';

export const AGENTS = [
  { id: 'agent-1', tasks: ['t1', 't2'] },
  { id: 'agent-2', tasks: ['t3'] },
  { id: 'agent-3', tasks: ['t4'] },
];

export const TASK_ORDER = ['t1', 't2', 't3', 't4'];

/** Fichiers protégés : aucun agent n'a le droit de les modifier. */
export const PROTECTED_FILES = ['Cargo.toml', '.gitignore', 'src/lib.rs', 'tests/behavior.rs'];

export const TASKS = [
  {
    id: 't1',
    agent: 'agent-1',
    title: 'stats::mean',
    path: 'src/stats.rs',
    oldSnippet: `pub fn mean(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    // TODO(s8-mvp0c): implémenter la moyenne entière.
    Some(0)
}`,
    newSnippet: `pub fn mean(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<i64>() / values.len() as i64)
}`,
    markers: ['Some(values.iter().sum::<i64>() / values.len() as i64)'],
    tests_expected: ['mean_of_simple_values', 'mean_of_negative_values'],
  },
  {
    id: 't2',
    agent: 'agent-1',
    title: 'text::slugify',
    path: 'src/text.rs',
    oldSnippet: `pub fn slugify(title: &str) -> String {
    // TODO(s8-mvp0c): implémenter la transformation en slug.
    title.to_string()
}`,
    newSnippet: `pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_sep = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            pending_sep = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_sep = true;
        }
    }
    out
}`,
    markers: ['let mut pending_sep = false;', 'out.push(ch.to_ascii_lowercase());'],
    tests_expected: ['slugify_basic_title', 'slugify_collapses_separators_and_trims', 'slugify_empty_is_empty'],
  },
  {
    id: 't3',
    agent: 'agent-2',
    title: 'stats::median',
    path: 'src/stats.rs',
    oldSnippet: `pub fn median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    // TODO(s8-mvp0c): implémenter la médiane entière.
    Some(0)
}`,
    newSnippet: `pub fn median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[middle])
    } else {
        Some((sorted[middle - 1] + sorted[middle]) / 2)
    }
}`,
    markers: ['sorted.sort_unstable();', 'Some((sorted[middle - 1] + sorted[middle]) / 2)'],
    tests_expected: ['median_of_odd_list', 'median_of_even_list'],
  },
  {
    id: 't4',
    agent: 'agent-3',
    title: 'dates::parse_iso_date',
    path: 'src/dates.rs',
    oldSnippet: `pub fn parse_iso_date(text: &str) -> Option<(u16, u8, u8)> {
    // TODO(s8-mvp0c): implémenter l'analyse de date ISO.
    let _ = text;
    None
}`,
    newSnippet: `pub fn parse_iso_date(text: &str) -> Option<(u16, u8, u8)> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let is_digit = |slice: &[u8]| slice.iter().all(|byte| byte.is_ascii_digit());
    if !is_digit(&bytes[0..4]) || !is_digit(&bytes[5..7]) || !is_digit(&bytes[8..10]) {
        return None;
    }
    let year = (bytes[0] - b'0') as u16 * 1000
        + (bytes[1] - b'0') as u16 * 100
        + (bytes[2] - b'0') as u16 * 10
        + (bytes[3] - b'0') as u16;
    let month = (bytes[5] - b'0') * 10 + (bytes[6] - b'0');
    let day = (bytes[8] - b'0') * 10 + (bytes[9] - b'0');
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}`,
    markers: ["if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {", 'Some((year, month, day))'],
    tests_expected: ['parse_iso_date_accepts_valid_date', 'parse_iso_date_rejects_invalid_month', 'parse_iso_date_rejects_wrong_shape'],
  },
];

export function taskById(id) {
  const task = TASKS.find((item) => item.id === id);
  if (!task) throw new Error(`tâche inconnue: ${id}`);
  return task;
}

export function tasksOf(agentId) {
  const agent = AGENTS.find((item) => item.id === agentId);
  if (!agent) throw new Error(`agent inconnu: ${agentId}`);
  return agent.tasks.map(taskById);
}

export function agentById(agentId) {
  const agent = AGENTS.find((item) => item.id === agentId);
  if (!agent) throw new Error(`agent inconnu: ${agentId}`);
  return agent;
}

/** Tous les fichiers pouvant être modifiés par les agents. */
export function editableFiles() {
  return [...new Set(TASKS.map((task) => task.path))];
}

/** Applique des tâches (snippets exacts) à une map chemin -> contenu. */
export function applyTasks(contents, taskIds) {
  const working = { ...contents };
  const applied = [];
  for (const id of taskIds) {
    const task = taskById(id);
    const source = working[task.path];
    if (typeof source !== 'string') throw new Error(`fichier absent: ${task.path}`);
    const occurrences = source.split(task.oldSnippet).length - 1;
    if (occurrences !== 1) {
      throw new Error(`tâche ${id}: ${occurrences} occurrence(s) de l'extrait attendu dans ${task.path}`);
    }
    working[task.path] = source.replace(task.oldSnippet, task.newSnippet);
    applied.push({ id, path: task.path, agent: task.agent });
  }
  return { contents: working, applied };
}

/** Contenu de référence exact après application de toutes les tâches. */
export function referenceContents(baseContents) {
  return applyTasks(baseContents, TASK_ORDER).contents;
}

export function markersForTasks(taskIds) {
  return taskIds.flatMap((id) => taskById(id).markers.map((line) => ({ task: id, line })));
}

/** Manifeste sérialisable du corpus (preuves + pré-enregistrement). */
export function corpusManifest() {
  return {
    agents: AGENTS,
    task_order: TASK_ORDER,
    protected_files: PROTECTED_FILES,
    tasks: TASKS.map((task) => ({
      id: task.id,
      agent: task.agent,
      title: task.title,
      path: task.path,
      markers: task.markers,
      tests_expected: task.tests_expected,
      old_snippet_sha256: sha256Hex(task.oldSnippet),
      new_snippet_sha256: sha256Hex(task.newSnippet),
    })),
  };
}

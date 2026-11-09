//! Corpus d'édition S8 : les mêmes 6 demandes d'édition pour les trois scénarios.
//!
//! IMPORTANT — ce que sont ces « agents » : des éditeurs SCRIPTÉS DÉTERMINISTES.
//! Ce ne sont pas des LLM. Chaque tâche est une opération de remplacement de
//! ligne, reproductible, sans aucune décision sémantique. Les tokens IA ne sont
//! donc pas mesurés (ils n'existent pas) et le temps de supervision humaine
//! n'est pas mesuré non plus.
//!
//! Toutes les opérations remplacent exactement une ligne par exactement une
//! ligne : le nombre de lignes de chaque fichier ne change jamais, ce qui rend
//! les hunks sans contexte sûrs à appliquer même quand un autre agent a déjà
//! modifié une autre ligne du même fichier (le coordinateur applique les hunks
//! à des positions exactes, sans recherche de décalage).

export function fnBody(file, fn, newLine) {
  return { kind: 'fn_body', path: file, fn, newLine };
}

export function replaceLine(file, oldLine, newLine) {
  return { kind: 'replace_line', path: file, oldLine, newLine };
}

/**
 * Les 6 tâches, réparties sur 3 agents (2 chacune).
 *
 * - t1/t2 : agent-1, fichiers différents ;
 * - t3    : agent-2, changement de SIGNATURE (beta_pad) + mise à jour de ses
 *           deux sites d'appel de test ;
 * - t4/t6 : agent-2 et agent-3 écrivent la MÊME fonction `delta_count` :
 *           cas de conflit contrôlé (édition concurrente de la même unité) ;
 * - t2/t5 : agent-1 et agent-3 écrivent le MÊME fichier `src/gamma.rs` mais des
 *           fonctions différentes : édition indépendante dans un fichier partagé.
 */
export const TASKS = [
  {
    id: 't1',
    agent: 'agent-1',
    title: 'alpha_add: addition saturante',
    kind: 'implementation',
    independent: true,
    symbols: ['alpha_add'],
    ops: [fnBody('src/alpha.rs', 'alpha_add', '    a.saturating_add(b)')],
  },
  {
    id: 't2',
    agent: 'agent-1',
    title: 'gamma_prefix: borne minimale explicite',
    kind: 'implementation',
    independent: true,
    symbols: ['gamma_prefix'],
    ops: [
      fnBody('src/gamma.rs', 'gamma_prefix', '    s.chars().take(n.min(s.chars().count())).collect()'),
    ],
  },
  {
    id: 't3',
    agent: 'agent-2',
    title: 'beta_pad: changement de signature (+2 sites d’appel)',
    kind: 'signature',
    independent: true,
    symbols: ['beta_pad'],
    ops: [
      replaceLine(
        'src/beta.rs',
        'pub fn beta_pad(text: &str, width: usize) -> String {',
        'pub fn beta_pad(text: &str, width: usize, fill: char) -> String {',
      ),
      fnBody('src/beta.rs', 'beta_pad', '    format!("{text:<width$}").replace(\' \', &fill.to_string())'),
      replaceLine(
        'tests/behavior.rs',
        '    assert_eq!(beta::beta_pad("ab", 5), "ab   ");',
        '    assert_eq!(beta::beta_pad("ab", 5, \' \'), "ab   ");',
      ),
      replaceLine(
        'tests/behavior.rs',
        '    assert_eq!(beta::beta_pad("abcdef", 3), "abcdef");',
        '    assert_eq!(beta::beta_pad("abcdef", 3, \'-\'), "abcdef");',
      ),
    ],
  },
  {
    id: 't4',
    agent: 'agent-2',
    title: 'delta_count: itération explicite',
    kind: 'implementation',
    independent: true,
    symbols: ['delta_count'],
    competing_with: 't6',
    ops: [fnBody('src/delta.rs', 'delta_count', '    items.iter().count()')],
  },
  {
    id: 't5',
    agent: 'agent-3',
    title: 'gamma_join: cas vide explicite',
    kind: 'implementation',
    independent: true,
    symbols: ['gamma_join'],
    ops: [
      fnBody(
        'src/gamma.rs',
        'gamma_join',
        '    if items.is_empty() { String::new() } else { items.join(sep) }',
      ),
    ],
  },
  {
    id: 't6',
    agent: 'agent-3',
    title: 'delta_count: variante concurrente (conflit contrôlé avec t4)',
    kind: 'implementation',
    independent: false,
    competing_with: 't4',
    symbols: ['delta_count'],
    ops: [fnBody('src/delta.rs', 'delta_count', '    items.len().max(0)')],
  },
];

export const AGENTS = ['agent-1', 'agent-2', 'agent-3'];

export function tasksOf(agent) {
  return TASKS.filter((task) => task.agent === agent);
}

export function taskById(id) {
  return TASKS.find((task) => task.id === id);
}

/**
 * Scénarios : quelles tâches doivent survivre dans l'état final.
 *
 * A et B détectent le conflit t4/t6 et retiennent t4 (premier intégré, ordre
 * déterministe des agents) ; t6 est refusé/résolu hors du résultat.
 * C applique les six éditions dans l'ordre : t6 écrase t4 silencieusement.
 */
export const WINNER = {
  A: 't4',
  B: 't4',
  C: 't6',
};

export function expectedTaskOrder(scenario) {
  if (scenario === 'C') return TASKS.map((task) => task.id);
  return TASKS.filter((task) => task.id !== 't6').map((task) => task.id);
}

export function acceptedTaskIds(scenario) {
  if (scenario === 'C') return TASKS.map((task) => task.id);
  return TASKS.filter((task) => task.id !== 't6').map((task) => task.id);
}

export function independentTaskIds() {
  return TASKS.filter((task) => task.independent).map((task) => task.id);
}

// --- application des opérations -------------------------------------------

function splitLines(text) {
  return text.split('\n');
}

function findUniqueLine(lines, expected, what) {
  const matches = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i] === expected) matches.push(i);
  }
  if (matches.length !== 1) {
    throw new Error(`${what}: ${matches.length} occurrences de ${JSON.stringify(expected)} (attendu 1)`);
  }
  return matches[0];
}

function findFunctionBodyLine(lines, fnName, file) {
  const signatureRe = new RegExp(`^\\s*pub fn ${fnName}\\b`);
  const matches = [];
  for (let i = 0; i < lines.length - 1; i += 1) {
    if (signatureRe.test(lines[i])) matches.push(i);
  }
  if (matches.length !== 1) {
    throw new Error(`${file}: ${matches.length} signatures de ${fnName} (attendu 1)`);
  }
  const bodyIndex = matches[0] + 1;
  if (bodyIndex >= lines.length || lines[bodyIndex].trim() === '') {
    throw new Error(`${file}: corps de ${fnName} introuvable à la ligne ${bodyIndex + 1}`);
  }
  return bodyIndex;
}

/**
 * Applique une liste d'opérations à un ensemble de contenus (chemin -> texte).
 * @returns {{contents:Object<string,string>, applied:Array}}
 */
export function applyOps(contents, ops) {
  const working = { ...contents };
  const applied = [];
  for (const op of ops) {
    if (!(op.path in working)) throw new Error(`op ${op.kind} sur fichier inconnu ${op.path}`);
    const lines = splitLines(working[op.path]);
    const before = lines.length;
    let lineIndex;
    if (op.kind === 'fn_body') {
      lineIndex = findFunctionBodyLine(lines, op.fn, op.path);
      applied.push({
        path: op.path,
        kind: op.kind,
        fn: op.fn,
        line_number: lineIndex + 1,
        old_line: lines[lineIndex],
        new_line: op.newLine,
      });
      lines[lineIndex] = op.newLine;
    } else if (op.kind === 'replace_line') {
      lineIndex = findUniqueLine(lines, op.oldLine, `${op.path} replace_line`);
      applied.push({
        path: op.path,
        kind: op.kind,
        line_number: lineIndex + 1,
        old_line: op.oldLine,
        new_line: op.newLine,
      });
      lines[lineIndex] = op.newLine;
    } else {
      throw new Error(`kind d'opération inconnu: ${op.kind}`);
    }
    if (lines.length !== before) {
      throw new Error(`${op.path}: l'opération ${op.kind} a changé le nombre de lignes (${before} -> ${lines.length})`);
    }
    working[op.path] = lines.join('\n');
  }
  return { contents: working, applied };
}

/** Applique les tâches d'une séquence (id -> contenu de référence). */
export function referenceContents(baseContents, taskIds) {
  let contents = { ...baseContents };
  for (const id of taskIds) {
    const task = taskById(id);
    contents = applyOps(contents, task.ops).contents;
  }
  return contents;
}

/** Marqueurs textuels attendus d'une tâche (lignes neuves), calculés sur la base. */
export function markersForTask(task, baseContents) {
  const { applied } = applyOps(baseContents, task.ops);
  return applied.map((entry) => ({ path: entry.path, line: entry.new_line }));
}

/**
 * Construit un diff unifié sans contexte à partir des opérations appliquées à
 * la base. Chaque hunk remplace une ligne par une ligne à position identique.
 */
export function buildUnifiedPatch(baseContents, ops) {
  const { contents, applied } = applyOps(baseContents, ops);
  const byFile = new Map();
  for (const entry of applied) {
    if (!byFile.has(entry.path)) byFile.set(entry.path, []);
    byFile.get(entry.path).push(entry);
  }
  const parts = [];
  const targetPaths = [];
  const changed = [];
  for (const [file, entries] of byFile.entries()) {
    entries.sort((a, b) => a.line_number - b.line_number);
    targetPaths.push(file);
    const hunks = [];
    parts.push(`diff --git a/${file} b/${file}`);
    parts.push(`--- a/${file}`);
    parts.push(`+++ b/${file}`);
    for (const entry of entries) {
      parts.push(`@@ -${entry.line_number},1 +${entry.line_number},1 @@`);
      parts.push(`-${entry.old_line}`);
      parts.push(`+${entry.new_line}`);
      hunks.push({ line_number: entry.line_number, old_line: entry.old_line, new_line: entry.new_line });
    }
    changed.push({ path: file, hunks });
  }
  return { patch: `${parts.join('\n')}\n`, targetPaths, contents, applied, changed };
}

/** Description sérialisable du corpus pour les preuves. */
export function corpusManifest() {
  return TASKS.map((task) => ({
    id: task.id,
    agent: task.agent,
    title: task.title,
    kind: task.kind,
    independent: task.independent,
    competing_with: task.competing_with ?? null,
    symbols: task.symbols,
    ops: task.ops,
  }));
}

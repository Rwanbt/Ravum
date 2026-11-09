//! Fusion 3-way minimale par lignes, sans dépendance et sans commande Git.
//!
//! Utilisée par la stratégie A (une copie par agent → intégration). Le dépôt
//! Ravum ne reçoit AUCUNE commande git mutante : la fusion est faite ici, en
//! mémoire, sur des fichiers texte.
//!
//! Algorithme : LCS ligne à ligne base↔version pour produire des hunks, puis
//! fusion des hunks non chevauchants. Un chevauchement est un CONFLIT : la
//! politique pré-enregistrée est « le côté déjà intégré gagne », le conflit est
//! compté et les changements perdus sont vérifiés séparément.

/** SequenceMatcher minimal : renvoie les opérations pour transformer `a` en `b`. */
function lcsOperations(a, b) {
  const n = a.length;
  const m = b.length;
  // Table LCS en Uint16 (fichiers de test petits, < 65535).
  const width = m + 1;
  const table = new Uint16Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      table[i * width + j] =
        a[i] === b[j] ? table[(i + 1) * width + j + 1] + 1 : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
    }
  }
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      ops.push({ kind: 'equal', line: a[i] });
      i += 1;
      j += 1;
    } else if (table[(i + 1) * width + j] >= table[i * width + j + 1]) {
      ops.push({ kind: 'delete', line: a[i] });
      i += 1;
    } else {
      ops.push({ kind: 'insert', line: b[j] });
      j += 1;
    }
  }
  while (i < n) {
    ops.push({ kind: 'delete', line: a[i] });
    i += 1;
  }
  while (j < m) {
    ops.push({ kind: 'insert', line: b[j] });
    j += 1;
  }
  return ops;
}

/** Découpe les opérations en hunks (start index dans `a`, lignes remplacées, lignes ajoutées). */
function hunksFromOps(a, ops) {
  const hunks = [];
  let index = 0;
  let current = null;
  for (const op of ops) {
    if (op.kind === 'equal') {
      if (current) {
        hunks.push(current);
        current = null;
      }
      index += 1;
    } else {
      if (!current) current = { start: index, end: index, replacement: [] };
      if (op.kind === 'delete') {
        index += 1;
        current.end = index;
      } else {
        current.replacement.push(op.line);
      }
    }
  }
  if (current) hunks.push(current);
  return hunks;
}

function splitLines(text) {
  if (text === '') return [];
  const trailing = text.endsWith('\n');
  const lines = text.split('\n');
  if (trailing) lines.pop();
  return lines;
}

function joinLines(lines, hadTrailingNewline) {
  const text = lines.join('\n');
  return hadTrailingNewline ? `${text}\n` : text;
}

/**
 * Fusionne deux versions (`ours`, `theirs`) d'un fichier par rapport à `base`.
 * @returns {{content:string, conflicts:Array<{start:number,end:number,ours:string[],theirs:string[]}>}}
 */
export function merge3(baseText, oursText, theirsText) {
  const base = splitLines(baseText);
  const trailing = baseText.endsWith('\n');
  const oursHunks = hunksFromOps(base, lcsOperations(base, splitLines(oursText)));
  const theirsHunks = hunksFromOps(base, lcsOperations(base, splitLines(theirsText)));

  const changes = [
    ...oursHunks.map((hunk) => ({ ...hunk, side: 'ours' })),
    ...theirsHunks.map((hunk) => ({ ...hunk, side: 'theirs' })),
  ].sort((a, b) => a.start - b.start || a.end - b.end);

  const out = [];
  const conflicts = [];
  let baseIndex = 0;
  let i = 0;
  while (i < changes.length) {
    // Regroupe les changements qui se chevauchent (fermeture transitive).
    const group = [changes[i]];
    let groupEnd = changes[i].end;
    let j = i + 1;
    while (j < changes.length && changes[j].start < groupEnd) {
      group.push(changes[j]);
      groupEnd = Math.max(groupEnd, changes[j].end);
      j += 1;
    }
    const groupStart = group[0].start;
    out.push(...base.slice(baseIndex, groupStart));
    const ours = group.filter((change) => change.side === 'ours');
    const theirs = group.filter((change) => change.side === 'theirs');
    if (ours.length > 0 && theirs.length > 0) {
      const oursLines = ours.flatMap((change) => change.replacement);
      const theirsLines = theirs.flatMap((change) => change.replacement);
      const same = oursLines.length === theirsLines.length && oursLines.every((line, index) => line === theirsLines[index]);
      if (!same) {
        // Politique pré-enregistrée : le côté déjà intégré (`ours`) gagne.
        conflicts.push({ start: groupStart, end: groupEnd, ours: oursLines, theirs: theirsLines });
        out.push(...oursLines);
      } else {
        out.push(...oursLines);
      }
    } else if (ours.length > 0) {
      out.push(...ours.flatMap((change) => change.replacement));
    } else {
      out.push(...theirs.flatMap((change) => change.replacement));
    }
    baseIndex = groupEnd;
    i = j;
  }
  out.push(...base.slice(baseIndex));
  return { content: joinLines(out, trailing), conflicts };
}

/** Vrai si deux modifications textuelles d'un même fichier se chevauchent. */
export function contentsDiffer(left, right) {
  return left !== right;
}

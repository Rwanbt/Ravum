//! Utilitaires S8 : hachage, fichiers, statistiques, sérialisation JSON.
//!
//! Aucune dépendance npm : uniquement la bibliothèque standard de Node 26.

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export function sha256Hex(input) {
  return createHash('sha256').update(input).digest('hex');
}

export function sha256File(file) {
  return sha256Hex(fs.readFileSync(file));
}

export function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

export function rmrf(dir) {
  fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 150 });
}

/** Écrit un fichier JSON de manière atomique (tmp + rename). */
export function writeJsonAtomic(file, value) {
  ensureDir(path.dirname(file));
  const tmp = `${file}.tmp-${process.pid}`;
  fs.writeFileSync(tmp, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
  fs.renameSync(tmp, file);
}

export function writeJsonlAtomic(file, lines) {
  ensureDir(path.dirname(file));
  const tmp = `${file}.tmp-${process.pid}`;
  fs.writeFileSync(tmp, lines.map((l) => `${l}\n`).join(''), 'utf8');
  fs.renameSync(tmp, file);
}

/** Liste récursive des fichiers d'un répertoire, triée, chemins relatifs en `/`. */
export function listFiles(root, { skipDirs = ['.git', 'target', 'node_modules', '.cache'] } = {}) {
  const out = [];
  const walk = (dir, rel) => {
    const entries = fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      const abs = path.join(dir, entry.name);
      const next = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (skipDirs.includes(entry.name)) continue;
        walk(abs, next);
      } else if (entry.isFile()) {
        out.push(next);
      }
    }
  };
  walk(root, '');
  return out;
}

/** Taille récursive en octets (sans suivre les liens). */
export function dirBytes(root) {
  let total = 0;
  const walk = (dir) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(abs);
      else if (entry.isFile()) {
        try {
          total += fs.statSync(abs).size;
        } catch {
          /* fichier disparu : ignoré */
        }
      }
    }
  };
  walk(root);
  return total;
}

export function copyTree(src, dest) {
  fs.cpSync(src, dest, { recursive: true, force: true, errorOnExist: false });
}

export function readIfExists(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
}

/** Hachage du contenu logique d'une fixture (chemin -> sha256). */
export function manifestOf(root) {
  const manifest = {};
  for (const rel of listFiles(root)) {
    manifest[rel] = sha256File(path.join(root, rel));
  }
  return manifest;
}

// --- statistiques descriptives -------------------------------------------

export function median(values) {
  const xs = values.filter((v) => Number.isFinite(v)).slice().sort((a, b) => a - b);
  if (xs.length === 0) return null;
  const mid = Math.floor(xs.length / 2);
  return xs.length % 2 === 1 ? xs[mid] : (xs[mid - 1] + xs[mid]) / 2;
}

/** Quantile par interpolation linéaire (type 7, celui de R/NumPy). */
export function quantile(values, q) {
  const xs = values.filter((v) => Number.isFinite(v)).slice().sort((a, b) => a - b);
  if (xs.length === 0) return null;
  if (xs.length === 1) return xs[0];
  const pos = (xs.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  if (lo === hi) return xs[lo];
  return xs[lo] + (xs[hi] - xs[lo]) * (pos - lo);
}

export function mad(values) {
  const m = median(values);
  if (m === null) return null;
  return median(values.map((v) => Math.abs(v - m)));
}

export function describe(values) {
  const xs = values.filter((v) => Number.isFinite(v));
  if (xs.length === 0) return { n: 0 };
  return {
    n: xs.length,
    median: round(median(xs)),
    min: round(Math.min(...xs)),
    max: round(Math.max(...xs)),
    p25: round(quantile(xs, 0.25)),
    p75: round(quantile(xs, 0.75)),
    iqr: round(quantile(xs, 0.75) - quantile(xs, 0.25)),
    mad: round(mad(xs)),
    samples: xs.map(round),
  };
}

export function round(value, digits = 3) {
  if (value === null || value === undefined || !Number.isFinite(value)) return value;
  const f = 10 ** digits;
  return Math.round(value * f) / f;
}

export function sum(values) {
  return values.reduce((a, b) => a + (Number.isFinite(b) ? b : 0), 0);
}

/** Remplace les chemins temporaires par `<tmp>` pour garder les preuves lisibles. */
export function sanitizer(tmpRoot) {
  const variants = new Set([
    tmpRoot,
    tmpRoot.replaceAll('\\', '/'),
    tmpRoot.replaceAll('/', '\\'),
  ]);
  return (text) => {
    if (typeof text !== 'string') return text;
    let out = text;
    for (const v of variants) {
      out = out.split(v).join('<tmp>');
    }
    return out;
  };
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

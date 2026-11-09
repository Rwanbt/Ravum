//! Utilitaires S8 MVP-0c : fichiers, hachage, copie, statistiques descriptives.
//!
//! Node 26 uniquement, aucune dépendance npm.

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export function sha256Hex(input) {
  return createHash('sha256').update(input).digest('hex');
}

export function sha256Bytes(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function sha256File(file) {
  return sha256Bytes(fs.readFileSync(file));
}

export function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

export function rmrf(dir) {
  fs.rmSync(dir, { recursive: true, force: true, maxRetries: 15, retryDelay: 200 });
}

export function copyTree(src, dest) {
  fs.cpSync(src, dest, { recursive: true, force: true, errorOnExist: false });
}

/** Liste récursive (chemins relatifs en `/`), répertoires techniques exclus. */
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

/** Contenus texte d'une liste de fichiers relatifs. */
export function readContents(root, relPaths) {
  const contents = {};
  for (const rel of relPaths) contents[rel] = fs.readFileSync(path.join(root, rel), 'utf8');
  return contents;
}

export function writeContents(root, contents) {
  for (const [rel, text] of Object.entries(contents)) {
    const abs = path.join(root, rel);
    ensureDir(path.dirname(abs));
    fs.writeFileSync(abs, text, 'utf8');
  }
}

/** Taille récursive en octets. */
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

/** Manifeste sha256 chemin -> hash. */
export function manifestOf(root) {
  const manifest = {};
  for (const rel of listFiles(root)) manifest[rel] = sha256File(path.join(root, rel));
  return manifest;
}

/** Écriture JSON atomique (tmp + rename). */
export function writeJsonAtomic(file, value) {
  ensureDir(path.dirname(file));
  const tmp = `${file}.tmp-${process.pid}-${Date.now()}`;
  fs.writeFileSync(tmp, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
  fs.renameSync(tmp, file);
}

export function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

// --- statistiques descriptives -------------------------------------------

export function round(value, digits = 3) {
  if (value === null || value === undefined || !Number.isFinite(value)) return value;
  const f = 10 ** digits;
  return Math.round(value * f) / f;
}

export function median(values) {
  const xs = values.filter((v) => Number.isFinite(v)).slice().sort((a, b) => a - b);
  if (xs.length === 0) return null;
  const mid = Math.floor(xs.length / 2);
  return xs.length % 2 === 1 ? xs[mid] : (xs[mid - 1] + xs[mid]) / 2;
}

/** Quantile par interpolation linéaire (type 7). */
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
    samples: xs.map((x) => round(x)),
  };
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Remplace un préfixe de dossier temporaire par `<tmp>` dans une chaîne. */
export function sanitizer(tmpRoot) {
  const variants = new Set([tmpRoot, tmpRoot.replaceAll('\\', '/'), tmpRoot.replaceAll('/', '\\')]);
  return (text) => {
    if (typeof text !== 'string') return text;
    let out = text;
    for (const variant of variants) out = out.split(variant).join('<tmp>');
    return out;
  };
}

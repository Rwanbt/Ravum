#!/usr/bin/env node
/**
 * tools/impact/impact.mjs — graphe d'impact conservateur pour la CI Ravum.
 *
 * Objectif (mandat MVP-0b, L4-E) : à partir des fichiers modifiés par une PR,
 * calculer les tests qui DOIVENT être exécutés, sans jamais en sauter un.
 *
 *   fichiers modifiés -> composants -> paquets Rust -> fermeture des dépendants
 *                    -> commandes de test sérialisées (--test-threads=1)
 *
 * Le graphe est construit en lisant le `Cargo.toml` racine (members,
 * `[workspace.dependencies]`) puis chaque manifeste de membre :
 *   - `[dependencies]`       : arête NORMALE (le code du dépendant change de
 *                              comportement si la dépendance change) ;
 *   - `[build-dependencies]` : arête NORMALE également (le script de build du
 *                              dépendant peut générer du code différent) ;
 *   - `[dev-dependencies]`   : arête de TEST à UN SEUL SAUT (les tests du
 *                              dépendant compilent la dépendance, mais le code
 *                              du dépendant ne change pas : on n'exige pas les
 *                              tests des dépendants de ce dépendant).
 *
 * Règles fail-closed (jamais de vert silencieux) :
 *   - tout `Cargo.toml`/`Cargo.lock`              => FULL ;
 *   - `crates/ravum-protocol/**`                  => FULL (contrat/ABI partagé) ;
 *   - `crates/ravum-patch-host/**`                => FULL (hôte natif/ABI) ;
 *   - `.github/**`                                => FULL (la sélection est en jeu) ;
 *   - `fixtures/**`                               => FULL (données partagées) ;
 *   - `scripts/**`, `tools/**` non mappé,
 *     `tools/impact/**`                           => FULL ;
 *   - tout chemin inconnu                         => FULL + listé dans unknown_paths ;
 *   - rename/copie                                => UNION source + destination ;
 *   - suppression                                 => composant du chemin supprimé ;
 *   - statut Git non résolu (U/X/B), diff vide,
 *     manifeste illisible                         => FULL + mapping INCERTAIN (exit 2).
 *
 * Sortie : JSON structuré (défaut) ou rapport lisible (`--explain`).
 * Zéro dépendance npm. Node >= 20 (testé Node 26).
 *
 * Deux issues possibles pour FULL (champ `decision`) :
 *   - `full-conservative` : FULL par règle conservatrice de contenu = issue
 *     LÉGITIME ; la suite complète EST l'exigence de ce diff, la CI réussit
 *     si les tests passent et échoue selon leur résultat réel ;
 *   - `full-uncertain` : mapping INCERTAIN / erreur interne ; la suite
 *     complète est exécutée par sécurité PUIS la CI échoue explicitement.
 * Aucune variable de dépôt n'est requise pour qu'une PR FULL légitime soit verte.
 *
 * Codes de sortie :
 *   0 = décision rendue (sélective OU FULL) ;
 *   2 = décision FULL + mapping INCERTAIN (la CI doit échouer explicitement) ;
 *   3 = erreur d'usage / interne sans décision ;
 *   4 = FULL constaté et `--fail-on-full` demandé (garde locale / hook).
 */

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export const SCHEMA = 'ravum-impact/1';

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Commande de la suite complète, toujours sérialisée (machine 16 Go). */
export const FULL_TEST_ARGV = Object.freeze(['cargo', 'test', '--workspace', '--', '--test-threads=1']);

/** Vérification d'intégrité documentaire (README, ADR) — pas une qualification produit. */
export const DOCS_TEST_ARGV = Object.freeze(['python', 'tools/validate_repo_docs.py']);

/** Règles dont un seul fichier déclenche la suite complète. */
const FULL_ONLY_COMPONENT = 'full-suite';

const LIMITS = Object.freeze([
  "Pas d'analyse sémantique : les liens créés par `include!`, les macros procédurales, les `cfg`, les scripts de build et les imports dynamiques ne sont pas suivis ; seule la déclaration Cargo est modélisée.",
  'Sous-ensemble TOML maison (tables, tableaux, tables inline, clés pointées, commentaires, chaînes) : un manifeste non compris rend le mapping INCERTAIN (FULL).',
  "Les `dev-dependencies` sont propagées sur un seul saut : les tests du dépendant sont exigés, pas ceux de ses propres dépendants.",
  '`fixtures/**` déclenche la suite complète : données consommées par plusieurs crates de test et par les démos, sans mapping statique sûr.',
  "`scripts/**` déclenche la suite complète mais les scripts de démo/orchestration ne sont pas exécutés par ce workflow (couverts par `mvp.yml`).",
  "UI statique (`apps/*/static/**`) : la suite Rust du paquet est exigée, et les 10 scénarios E2E navigateur sont exécutés quand `browser_e2e_required` est vrai.",
  "Un diff vide est traité comme incertain (FULL) : impossible de distinguer « aucun changement » d'une base Git erronée.",
  "Le graphe ne détecte pas la suppression ou la désactivation d'un test ; ce workflow ne neutralise rien mais ne peut pas prouver l'absence de tests retirés.",
]);

// ---------------------------------------------------------------------------
// Erreurs
// ---------------------------------------------------------------------------

export class ImpactError extends Error {
  constructor(message) {
    super(message);
    this.name = 'ImpactError';
  }
}

class TomlError extends Error {
  constructor(message) {
    super(message);
    this.name = 'TomlError';
  }
}

// ---------------------------------------------------------------------------
// Petit analyseur TOML (sous-ensemble suffisant pour les manifestes Cargo)
// ---------------------------------------------------------------------------

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/**
 * Découpe le texte en lignes logiques : les tableaux `members = [ ... ]` et les
 * tables inline peuvent s'étaler sur plusieurs lignes ; les commentaires `#`
 * hors chaînes sont retirés.
 */
function splitLogicalLines(text) {
  const lines = [];
  let current = '';
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      current += ch;
      if (escaped) escaped = false;
      else if (ch === '\\' && quote === '"') escaped = true;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || (ch === "'" && !isApostropheStart(text, i))) {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === '#') {
      while (i + 1 < text.length && text[i + 1] !== '\n') i += 1;
      continue;
    }
    if (ch === '[' || ch === '{') depth += 1;
    else if (ch === ']' || ch === '}') depth = Math.max(0, depth - 1);
    if ((ch === '\n' || ch === '\r') && depth === 0) {
      if (current.trim()) lines.push(current);
      current = '';
      continue;
    }
    current += ch;
  }
  if (current.trim()) lines.push(current);
  return lines;
}

// TOML n'a pas de chaînes entre apostrophes typographiques : un `'` isole
// toujours une chaîne littérale. Ce garde-fou existe pour rester lisible.
function isApostropheStart() {
  return false;
}

/** Analyse une clé TOML (simple, pointée ou entre guillemets). */
function parseKeyPath(text) {
  const parts = [];
  let current = '';
  let quote = null;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === '\\' && quote === '"' && i + 1 < text.length) {
        i += 1;
        current += text[i];
        continue;
      }
      if (ch === quote) quote = null;
      else current += ch;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === '.') {
      parts.push(current.trim());
      current = '';
      continue;
    }
    current += ch;
  }
  parts.push(current.trim());
  if (parts.some((part) => part === '')) {
    throw new TomlError(`clé TOML invalide: ${JSON.stringify(text)}`);
  }
  return parts;
}

/** Trouve le premier `=` hors chaîne et hors crochets. */
function findTopLevelEquals(text) {
  let quote = null;
  let depth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === '\\' && quote === '"') i += 1;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === '[' || ch === '{') depth += 1;
    else if (ch === ']' || ch === '}') depth -= 1;
    else if (ch === '=' && depth === 0) return i;
  }
  return -1;
}

/** Découpe sur un séparateur au niveau zéro (tableaux et tables inline). */
function splitTopLevel(text, separator) {
  const parts = [];
  let current = '';
  let quote = null;
  let depth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      current += ch;
      if (ch === '\\' && quote === '"' && i + 1 < text.length) {
        i += 1;
        current += text[i];
      } else if (ch === quote) {
        quote = null;
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === '[' || ch === '{') depth += 1;
    else if (ch === ']' || ch === '}') depth -= 1;
    if (ch === separator && depth === 0) {
      parts.push(current);
      current = '';
      continue;
    }
    current += ch;
  }
  parts.push(current);
  return parts;
}

function parseBasicString(text) {
  if (!text.endsWith('"') || text.length < 2) throw new TomlError(`chaîne non terminée: ${text}`);
  const body = text.slice(1, -1);
  let out = '';
  for (let i = 0; i < body.length; i += 1) {
    const ch = body[i];
    if (ch !== '\\') {
      out += ch;
      continue;
    }
    i += 1;
    const esc = body[i];
    if (esc === undefined) throw new TomlError(`échappement incomplet: ${text}`);
    if (esc === 'n') out += '\n';
    else if (esc === 't') out += '\t';
    else if (esc === 'r') out += '\r';
    else if (esc === '"') out += '"';
    else if (esc === '\\') out += '\\';
    else out += `\\${esc}`;
  }
  return out;
}

function parseLiteralString(text) {
  if (!text.endsWith("'") || text.length < 2) throw new TomlError(`chaîne littérale non terminée: ${text}`);
  return text.slice(1, -1);
}

function parseArray(text) {
  if (!text.endsWith(']')) throw new TomlError(`tableau non terminé: ${text}`);
  const body = text.slice(1, -1).trim();
  if (!body) return [];
  const items = splitTopLevel(body, ',')
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
  return items.map((item) => parseValue(item));
}

function parseInlineTable(text) {
  if (!text.endsWith('}')) throw new TomlError(`table inline non terminée: ${text}`);
  const body = text.slice(1, -1).trim();
  const table = {};
  if (!body) return table;
  for (const part of splitTopLevel(body, ',')) {
    const entry = part.trim();
    if (!entry) continue;
    const eq = findTopLevelEquals(entry);
    if (eq < 0) throw new TomlError(`entrée de table inline invalide: ${entry}`);
    const keys = parseKeyPath(entry.slice(0, eq).trim());
    const value = parseValue(entry.slice(eq + 1).trim());
    assignDotted(table, keys, value);
  }
  return table;
}

function parseValue(text) {
  const value = text.trim();
  if (!value) throw new TomlError('valeur TOML vide');
  if (value.startsWith('"')) return parseBasicString(value);
  if (value.startsWith("'")) return parseLiteralString(value);
  if (value.startsWith('[')) return parseArray(value);
  if (value.startsWith('{')) return parseInlineTable(value);
  if (value === 'true') return true;
  if (value === 'false') return false;
  if (/^[+-]?[0-9][0-9_]*(\.[0-9_]+)?([eE][+-]?[0-9_]+)?$/.test(value)) {
    return Number(value.replace(/_/g, ''));
  }
  // Dates, heures, valeurs nues : conservées telles quelles (non utilisées).
  return value;
}

function assignDotted(table, keys, value) {
  let node = table;
  for (let i = 0; i < keys.length - 1; i += 1) {
    const key = keys[i];
    if (node[key] === undefined) node[key] = {};
    else if (!isObject(node[key])) throw new TomlError(`conflit de clé TOML: ${keys.join('.')}`);
    node = node[key];
  }
  const last = keys[keys.length - 1];
  if (Object.prototype.hasOwnProperty.call(node, last)) {
    throw new TomlError(`clé TOML dupliquée: ${keys.join('.')}`);
  }
  node[last] = value;
}

function ensureTable(root, keys, arrayTable) {
  let node = root;
  for (let i = 0; i < keys.length; i += 1) {
    const key = keys[i];
    const last = i === keys.length - 1;
    if (last && arrayTable) {
      if (node[key] === undefined) node[key] = [];
      if (!Array.isArray(node[key])) throw new TomlError(`conflit de table TOML: ${keys.join('.')}`);
      const element = {};
      node[key].push(element);
      return element;
    }
    if (node[key] === undefined) node[key] = {};
    else if (!isObject(node[key])) throw new TomlError(`conflit de clé/table TOML: ${keys.join('.')}`);
    node = node[key];
  }
  return node;
}

/** Analyse un document TOML restreint en objet imbriqué. */
export function parseToml(text) {
  const root = {};
  let current = root;
  const lines = splitLogicalLines(text);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line) continue;
    if (line.startsWith('[')) {
      const arrayTable = line.startsWith('[[');
      const close = arrayTable ? ']]' : ']';
      if (!line.endsWith(close)) {
        throw new TomlError(`en-tête de table invalide (ligne ${i + 1}): ${line}`);
      }
      const body = line.slice(arrayTable ? 2 : 1, line.length - close.length).trim();
      if (!body) throw new TomlError(`en-tête de table vide (ligne ${i + 1})`);
      current = ensureTable(root, parseKeyPath(body), arrayTable);
      continue;
    }
    const eq = findTopLevelEquals(line);
    if (eq < 0) throw new TomlError(`ligne TOML sans '=' (ligne ${i + 1}): ${line}`);
    const keys = parseKeyPath(line.slice(0, eq).trim());
    const value = parseValue(line.slice(eq + 1).trim());
    assignDotted(current, keys, value);
  }
  return root;
}

// ---------------------------------------------------------------------------
// Graphe de dépendances du workspace
// ---------------------------------------------------------------------------

export function toPosix(value) {
  return String(value).replace(/\\/g, '/');
}

export function normalizeRel(value) {
  const posix = toPosix(value).replace(/^\.\//, '');
  const normalized = path.posix.normalize(posix);
  return normalized === '.' ? '' : normalized.replace(/\/+$/, '');
}

/** Vrai si `rel` est `dir` ou un descendant de `dir`. */
function under(rel, dir) {
  return rel === dir || rel.startsWith(`${dir}/`);
}

const DEPENDENCY_KINDS = new Map([
  ['dependencies', 'normal'],
  ['dev-dependencies', 'dev'],
  ['build-dependencies', 'build'],
]);

/**
 * Énumère les tables de dépendances d'un manifeste de membre, y compris les
 * tables ciblées `[target.'cfg(...)'.dependencies]`.
 */
function* dependencyTables(node, pathParts) {
  if (!isObject(node)) return;
  for (const [key, value] of Object.entries(node)) {
    if (!isObject(value)) continue;
    const kind = DEPENDENCY_KINDS.get(key);
    if (kind) {
      yield { kind, table: value, tablePath: [...pathParts, key].join('.') };
      continue;
    }
    yield* dependencyTables(value, [...pathParts, key]);
  }
}

function resolvePathDependency(pathText, base, workspace, fromPackage) {
  const joined = base ? path.posix.join(toPosix(base), toPosix(pathText)) : toPosix(pathText);
  const dir = normalizeRel(joined);
  const internal = workspace.byDirLower.get(dir.toLowerCase());
  if (internal) return { internal };
  workspace.notes.push(
    `dépendance path hors membres: ${fromPackage.name} -> ${dir} (chemin inconnu du graphe: toute modification de ce répertoire déclenchera FULL)`,
  );
  return { external: true, unmappedPath: dir };
}

/**
 * Résout une entrée de dépendance en arête interne, dépendance externe, ou
 * erreur conservatrice.
 */
function resolveDependency(depName, spec, pkg, workspace) {
  let rawPath = null;
  let inherited = false;
  if (isObject(spec)) {
    if (spec.workspace === true) {
      inherited = true;
      const workspaceSpec = workspace.workspaceDeps[depName];
      if (workspaceSpec === undefined) {
        if (workspace.packages.has(depName)) {
          workspace.notes.push(
            `dépendance ${pkg.name} -> ${depName} = { workspace = true } mais absente de [workspace.dependencies]: arête interne conservatrice par nom`,
          );
          return { internal: depName };
        }
        workspace.errors.push(
          `dépendance workspace introuvable: ${pkg.name} -> ${depName} (aucune entrée [workspace.dependencies])`,
        );
        return { unresolved: true };
      }
      if (isObject(workspaceSpec) && typeof workspaceSpec.path === 'string') {
        rawPath = { path: workspaceSpec.path, base: '' };
      }
      // Une entrée workspace sans `path` est une dépendance registre.
    } else if (typeof spec.path === 'string') {
      rawPath = { path: spec.path, base: pkg.dir };
    }
  }
  if (rawPath) {
    return resolvePathDependency(rawPath.path, rawPath.base, workspace, pkg);
  }
  if (workspace.packages.has(depName)) {
    workspace.notes.push(
      `dépendance ${pkg.name} -> ${depName} déclarée sans path (registre ?) mais homonyme d'un paquet interne: arête interne conservatrice`,
    );
    return { internal: depName };
  }
  void inherited;
  return { external: true };
}

/**
 * Construit le graphe interne : paquets, arêtes sortantes typées et carte des
 * dépendants inverses.
 */
export function loadWorkspace(root = REPO_ROOT) {
  const errors = [];
  const notes = [];
  const rootManifestPath = path.join(root, 'Cargo.toml');
  let rootToml;
  try {
    rootToml = parseToml(readFileSync(rootManifestPath, 'utf8'));
  } catch (err) {
    throw new ImpactError(`manifeste racine illisible (${rootManifestPath}): ${err.message}`);
  }
  const rootWorkspace = isObject(rootToml.workspace) ? rootToml.workspace : null;
  if (!rootWorkspace) throw new ImpactError('Cargo.toml racine sans table [workspace]: graphe impossible');
  const workspaceDeps = isObject(rootWorkspace.dependencies) ? rootWorkspace.dependencies : {};
  const memberList = Array.isArray(rootWorkspace.members) ? rootWorkspace.members : [];
  if (memberList.length === 0) errors.push('workspace sans `members`: graphe impossible, toute analyse sera FULL');

  const packages = new Map();
  const byDirLower = new Map();
  const tomlByPackage = new Map();
  for (const rawMember of memberList) {
    if (typeof rawMember !== 'string') {
      errors.push(`member non textuel ignoré: ${JSON.stringify(rawMember)}`);
      continue;
    }
    const dir = normalizeRel(rawMember);
    const manifestPath = path.join(root, dir, 'Cargo.toml');
    let toml;
    try {
      toml = parseToml(readFileSync(manifestPath, 'utf8'));
    } catch (err) {
      errors.push(`manifeste de membre illisible ${dir}/Cargo.toml: ${err.message}`);
      continue;
    }
    const name = isObject(toml.package) ? toml.package.name : undefined;
    if (typeof name !== 'string' || name.length === 0) {
      errors.push(`manifeste sans [package].name: ${dir}/Cargo.toml`);
      continue;
    }
    if (packages.has(name)) {
      errors.push(`nom de paquet dupliqué dans le workspace: ${name}`);
      continue;
    }
    packages.set(name, {
      name,
      dir,
      manifest: `${dir}/Cargo.toml`,
      deps: { normal: [], dev: [], build: [] },
    });
    byDirLower.set(dir.toLowerCase(), name);
    tomlByPackage.set(name, toml);
  }

  const workspace = {
    root,
    workspaceDeps,
    packages,
    byDirLower,
    errors,
    notes,
    /** name -> {normal:[], dev:[], build:[]} (noms internes, triés, dédupliqués) */
    deps: new Map(),
    /** target -> [{name, kind}] (dépendants inverses) */
    dependents: new Map(),
  };

  for (const pkg of packages.values()) {
    const toml = tomlByPackage.get(pkg.name);
    const edges = { normal: new Set(), dev: new Set(), build: new Set() };
    for (const { kind, table, tablePath } of dependencyTables(toml, [])) {
      for (const [depName, spec] of Object.entries(table)) {
        const resolved = resolveDependency(depName, spec, pkg, workspace);
        if (!resolved.internal) continue;
        if (resolved.internal === pkg.name) {
          errors.push(`auto-dépendance interne détectée: ${pkg.name} (${tablePath})`);
          continue;
        }
        if (!packages.has(resolved.internal)) {
          errors.push(`arête interne vers un paquet absent du workspace: ${pkg.name} -> ${resolved.internal}`);
          continue;
        }
        edges[kind].add(resolved.internal);
      }
    }
    for (const kind of ['normal', 'dev', 'build']) {
      pkg.deps[kind] = [...edges[kind]].sort();
      for (const target of pkg.deps[kind]) {
        if (!workspace.dependents.has(target)) workspace.dependents.set(target, []);
        workspace.dependents.get(target).push({ name: pkg.name, kind });
      }
    }
    workspace.deps.set(pkg.name, pkg.deps);
  }
  for (const dependents of workspace.dependents.values()) {
    dependents.sort((a, b) => a.name.localeCompare(b.name) || a.kind.localeCompare(b.kind));
  }
  return workspace;
}

// ---------------------------------------------------------------------------
// Classification chemin -> composant
// ---------------------------------------------------------------------------

function fullResult(rule, reason, extra = {}) {
  return {
    components: [FULL_ONLY_COMPONENT],
    packages: [],
    full: reason,
    rule,
    docs: false,
    unknown: false,
    evidence: false,
    testFile: false,
    notes: [],
    ...extra,
  };
}

function packageComponent(packageName) {
  return `pkg:${packageName}`;
}

/**
 * Mappe un chemin du dépôt vers un ou plusieurs composants.
 * Renvoie `{ components, packages, full, rule, docs, unknown, evidence, testFile, notes }`.
 */
export function classifyPath(relPath, workspace) {
  const rel = normalizeRel(relPath);
  const base = path.posix.basename(rel);

  // 1. Manifestes de construction : le graphe de paquets peut changer.
  if (base === 'Cargo.toml' || base === 'Cargo.lock') {
    return fullResult(
      'manifest',
      `FULL: manifeste de construction modifié (${base}) — le graphe de dépendances peut changer sans que ce graphe le reflète`,
    );
  }

  // 2. CI : modifier la sélection modifie la garantie elle-même.
  if (under(rel, '.github')) {
    const isWorkflow = under(rel, '.github/workflows');
    return fullResult(
      'ci',
      `FULL: fichier de CI modifié (${isWorkflow ? 'workflow' : 'métadonnées GitHub'}) — la sélection de tests est elle-même en jeu`,
    );
  }

  // 3. Crates critiques : contrats transverses non exprimables sélectivement.
  if (under(rel, 'crates/ravum-protocol')) {
    return fullResult(
      'critical-contract',
      'FULL: crates/ravum-protocol modifié — contrat/ABI partagé par presque tous les composants, les démos E2E transverses ne sont pas exprimables sélectivement',
    );
  }
  if (under(rel, 'crates/ravum-patch-host')) {
    return fullResult(
      'critical-native-host',
      'FULL: crates/ravum-patch-host modifié — hôte natif/ABI avec effets process-global, validation transverse requise',
    );
  }

  // 4. Données de test partagées.
  if (under(rel, 'fixtures')) {
    return fullResult(
      'shared-fixtures',
      'FULL: fixtures/ modifié — données consommées par plusieurs crates de test, aucun mapping statique sûr vers une seule suite',
    );
  }

  // 5. L'outil de sélection lui-même : on ne peut pas lui faire confiance sur sa propre PR.
  if (under(rel, 'tools/impact')) {
    return fullResult(
      'self-modification',
      'FULL: tools/impact/ modifié — l’outil de sélection ne peut pas certifier sa propre modification',
    );
  }

  // 6. Le validateur docs est exercé en l'exécutant : docs check uniquement.
  if (rel === 'tools/validate_repo_docs.py') {
    return {
      components: ['docs'],
      packages: [],
      full: null,
      rule: null,
      docs: true,
      unknown: false,
      evidence: false,
      testFile: false,
      notes: ["outil de vérification documentaire modifié: la vérification docs est exigée (elle exécute ce fichier)"],
    };
  }

  // 6bis. Harnais de conformance du langage : paquet du workspace sous tools/,
  //       mappé explicitement à sa propre suite (comme un crate), pas à un FULL
  //       d'outillage non modélisé.
  if (under(rel, 'tools/conformance')) {
    return {
      components: [packageComponent('ravum-conformance')],
      packages: ['ravum-conformance'],
      full: null,
      rule: null,
      docs: false,
      unknown: false,
      evidence: false,
      testFile: /(^|\/)tests\//.test(rel) || /(^|\/)tests?\.rs$/.test(base),
      notes: [
        'harnais de conformance Ravum modifié: sa propre suite est exigée (cargo test -p ravum-conformance)',
      ],
    };
  }

  // 7. Autres outils : sémantique inconnue du graphe.
  if (under(rel, 'tools')) {
    return fullResult(
      'unmapped-tooling',
      'FULL: outil hors du graphe d’impact (tools/) — sémantique non modélisée',
    );
  }

  // 8. Scripts d'orchestration / démonstration.
  if (under(rel, 'scripts')) {
    return fullResult(
      'orchestration-scripts',
      'FULL: scripts/ modifié — orchestration démo/tests sans mapping statique; la suite complète est exigée (les scripts eux-mêmes restent couverts par mvp.yml)',
    );
  }

  // 9. Artefacts de preuve : aucun test automatique ne les consomme.
  if (under(rel, 'evidence')) {
    return {
      components: ['evidence'],
      packages: [],
      full: null,
      rule: null,
      docs: false,
      unknown: false,
      evidence: true,
      testFile: false,
      notes: ['artefact de preuve modifié: aucun test automatisé ne le consomme (seuls les scripts de démo le lisent)'],
    };
  }

  // 10. Documentation.
  const isRootMarkdown = rel.indexOf('/') === -1 && rel.endsWith('.md');
  if (under(rel, 'docs') || isRootMarkdown || rel === 'docs') {
    return {
      components: ['docs'],
      packages: [],
      full: null,
      rule: null,
      docs: true,
      unknown: false,
      evidence: false,
      testFile: false,
      notes: ['documentation modifiée: vérification d’intégrité documents (pas de test Rust sélectif)'],
    };
  }

  // 11. Crates et applications du workspace.
  for (const scope of ['crates', 'apps']) {
    if (!under(rel, scope)) continue;
    const parts = rel.split('/');
    if (parts.length < 2) {
      return fullResult('unknown-component', `FULL: chemin ambigu sous ${scope}/: ${rel}`, { unknown: true });
    }
    const memberDir = `${scope}/${parts[1]}`;
    const packageName = workspace.byDirLower.get(memberDir.toLowerCase());
    if (!packageName) {
      return fullResult(
        'unknown-component',
        `FULL: composant inconnu du workspace (${memberDir}) — paquet absent des members: ${rel}`,
        { unknown: true },
      );
    }
    const isTestFile = /(^|\/)tests\//.test(rel) || /(^|\/)tests?\.rs$/.test(base);
    const notes = [];
    if (isTestFile) {
      notes.push(`fichier de test modifié: la suite de ${packageName} est exigée au minimum (fermeture des dépendants appliquée)`);
    }
    if (/\/static\//.test(rel)) {
      notes.push('ressource statique (UI web): la suite Rust du paquet est exigée; aucune suite navigateur n’est branchée sur ce workflow');
    }
    return {
      components: [packageComponent(packageName)],
      packages: [packageName],
      full: null,
      rule: null,
      docs: false,
      unknown: false,
      evidence: false,
      testFile: isTestFile,
      notes,
    };
  }

  // 12. Tout le reste est inconnu => fail-closed.
  return fullResult(
    'unknown-path',
    `FULL: chemin hors du graphe d’impact connu: ${rel} — aucun composant ne peut être prouvé`,
    { unknown: true },
  );
}

// ---------------------------------------------------------------------------
// Fermeture des dépendants
// ---------------------------------------------------------------------------

function computeAffected(graph, directPackages) {
  const provenance = new Map();
  for (const name of [...directPackages].sort()) {
    provenance.set(name, { type: 'direct', via: null, kind: null });
  }
  const queue = [...directPackages].sort();
  while (queue.length > 0) {
    const target = queue.shift();
    for (const edge of graph.dependents.get(target) ?? []) {
      if (edge.kind === 'dev') continue;
      if (provenance.has(edge.name)) continue;
      provenance.set(edge.name, { type: 'transitive', via: target, kind: edge.kind });
      queue.push(edge.name);
    }
  }
  // Dépendants de test (dev-dependencies) : un seul saut, depuis tout paquet
  // déjà affecté (direct ou transitif). Leur code ne change pas, seuls leurs
  // tests recompilent la dépendance modifiée.
  const oneHop = new Map();
  for (const name of [...provenance.keys()].sort()) {
    for (const edge of graph.dependents.get(name) ?? []) {
      if (edge.kind !== 'dev') continue;
      if (provenance.has(edge.name) || oneHop.has(edge.name)) continue;
      oneHop.set(edge.name, name);
    }
  }
  for (const [name, via] of [...oneHop.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    provenance.set(name, { type: 'dev-dependent', via, kind: 'dev' });
  }
  return provenance;
}

function orderPackages(graph, packageSet) {
  const ordered = [];
  const state = new Map();
  const cycles = [];
  const visit = (name, stack) => {
    const current = state.get(name);
    if (current === 'done') return;
    if (current === 'visiting') {
      cycles.push([...stack, name].join(' -> '));
      return;
    }
    state.set(name, 'visiting');
    stack.push(name);
    const dependencies = [...(graph.deps.get(name)?.normal ?? []), ...(graph.deps.get(name)?.build ?? [])]
      .filter((dep) => packageSet.has(dep))
      .sort();
    for (const dep of dependencies) visit(dep, stack);
    stack.pop();
    state.set(name, 'done');
    ordered.push(name);
  };
  for (const name of [...packageSet].sort()) {
    if (!state.has(name)) visit(name, []);
  }
  return { ordered, cycles };
}

function describeProvenance(provenance) {
  if (provenance.type === 'direct') return 'impact direct';
  if (provenance.type === 'dev-dependent') {
    return `dépendant de test (dev-dependency) de ${provenance.via}`;
  }
  const kindLabel = provenance.kind === 'build' ? 'build-dependency' : 'dépendance';
  return `dépendant transitif via ${provenance.via} (${kindLabel})`;
}

// ---------------------------------------------------------------------------
// Analyse
// ---------------------------------------------------------------------------

function normalizePath(value) {
  return normalizeRel(value);
}

function normalizeChange(change) {
  const status = String(change?.status ?? 'M').trim().toUpperCase();
  if (!/^[ACDMRTUXB]/.test(status)) {
    throw new ImpactError(`statut de diff inconnu: ${JSON.stringify(change?.status)}`);
  }
  const head = status[0];
  if (head === 'R' || head === 'C') {
    const from = change.from ?? change.path;
    const to = change.to;
    if (!from || !to) {
      throw new ImpactError(`changement ${status} sans source/destination: ${JSON.stringify(change)}`);
    }
    return {
      status,
      from: normalizePath(from),
      to: normalizePath(to),
      entries: [
        { role: 'source', path: normalizePath(from) },
        { role: 'destination', path: normalizePath(to) },
      ],
    };
  }
  const single = change.path ?? change.from ?? change.to;
  if (!single) throw new ImpactError(`changement ${status} sans chemin: ${JSON.stringify(change)}`);
  return {
    status,
    path: normalizePath(single),
    entries: [{ role: 'path', path: normalizePath(single) }],
  };
}

function describeChange(change) {
  if (change.from && change.to) return `${change.status}: ${change.from} -> ${change.to}`;
  return `${change.status}: ${change.path}`;
}

function uniqueSorted(values) {
  return [...new Set(values)].sort();
}

function dedupe(values) {
  return [...new Set(values)];
}

/**
 * Calcule la décision d'impact : FULL ou liste exacte des paquets/commandes.
 *
 * @param {ReturnType<typeof loadWorkspace>} workspace
 * @param {Array<{status?:string,path?:string,from?:string,to?:string}>} changes
 * @returns décision structurée
 */
export function analyze(workspace, changes) {
  const reasons = [];
  const notes = [...workspace.notes];
  const unknownPaths = [];
  const fullTriggers = [];
  const components = {};
  const changed = [];
  const directPackages = new Set();
  let docsRequired = false;
  let full = false;
  let uncertain = false;

  const rawChanges = Array.isArray(changes) ? changes : [];
  if (rawChanges.length === 0) {
    full = true;
    uncertain = true;
    reasons.push(
      'full:diff-vide — aucun fichier modifié: impossible de distinguer un diff réellement vide d’une base Git erronée; suite complète exigée et mapping déclaré INCERTAIN',
    );
  }

  for (const rawChange of rawChanges) {
    const change = normalizeChange(rawChange);
    changed.push(change);
    const head = change.status[0];
    if (head === 'U' || head === 'X' || head === 'B') {
      full = true;
      uncertain = true;
      reasons.push(
        `full:statut-non-resolu — statut Git non résolu (${change.status}) pour ${describeChange(change)}: suite complète exigée et mapping INCERTAIN`,
      );
    }
    for (const entry of change.entries) {
      if (entry.path === '') continue;
      const classified = classifyPath(entry.path, workspace);
      if (!components[entry.path]) components[entry.path] = [];
      for (const component of classified.components) {
        if (!components[entry.path].includes(component)) components[entry.path].push(component);
      }
      if (classified.docs) docsRequired = true;
      for (const packageName of classified.packages) directPackages.add(packageName);
      if (classified.full) {
        full = true;
        fullTriggers.push({ path: entry.path, rule: classified.rule, reason: classified.full });
      }
      if (classified.unknown) unknownPaths.push(entry.path);
      for (const note of classified.notes) notes.push(`${entry.path}: ${note}`);
    }
  }

  for (const trigger of fullTriggers) {
    reasons.push(`full:${trigger.rule} — ${trigger.path} :: ${trigger.reason}`);
  }
  for (const pathValue of uniqueSorted(unknownPaths)) {
    reasons.push(`unknown-path — ${pathValue}`);
  }

  if (workspace.errors.length > 0) {
    full = true;
    uncertain = true;
    for (const error of workspace.errors) reasons.push(`uncertain:workspace — ${error}`);
  }
  if (directPackages.size > 0 && !full) {
    reasons.push(`direct — paquet(s) directement modifié(s): ${[...directPackages].sort().join(', ')}`);
  }
  if (docsRequired) {
    reasons.push('docs — documentation modifiée: vérification d’intégrité des documents requise');
  }

  let packages = [];
  let commands = [];
  const provenanceMap = {};

  if (full) {
    const reason = fullTriggers.length > 0
      ? 'FULL: au moins une règle fail-closed est déclenchée (voir reasons)'
      : 'FULL: mapping incertain (voir reasons)';
    commands.push({
      id: 'cargo-test-workspace',
      kind: 'cargo',
      argv: [...FULL_TEST_ARGV],
      reason,
      package: null,
    });
  } else {
    const provenance = computeAffected(workspace, directPackages);
    const ordered = orderPackages(workspace, new Set(provenance.keys()));
    for (const cycle of ordered.cycles) notes.push(`cycle de dépendances détecté (cargo l'autorise via dev-dependencies): ${cycle}`);
    packages = ordered.ordered;
    for (const packageName of packages) {
      const which = provenance.get(packageName);
      provenanceMap[packageName] = {
        type: which.type,
        via: which.via,
        kind: which.kind,
        label: describeProvenance(which),
      };
      if (which.type === 'transitive') {
        reasons.push(
          `transitive — test du dépendant ${packageName} requis car ${which.via} change (${which.kind === 'build' ? 'build-dependency' : 'dépendance'})`,
        );
      } else if (which.type === 'dev-dependent') {
        reasons.push(
          `dev-hop — test du dev-dépendant ${packageName} requis car ${which.via} change (dev-dependency, un seul saut)`,
        );
      }
      commands.push({
        id: `cargo-test-${packageName}`,
        kind: 'cargo',
        argv: ['cargo', 'test', '-p', packageName, '--', '--test-threads=1'],
        reason: `${packageName}: ${which.type === 'direct' ? 'impact direct' : describeProvenance(which)}`,
        package: packageName,
      });
    }
    if (packages.length === 0 && !docsRequired) {
      reasons.push('aucun-test — aucun test Rust automatisé n’est requis pour ce diff (composants hors code Rust)');
    }
  }

  if (docsRequired) {
    commands.push({
      id: 'docs-integrity',
      kind: 'docs',
      argv: [...DOCS_TEST_ARGV],
      reason: 'documentation modifiée: vérification d’intégrité des documents (README/ADR), pas une qualification produit',
      package: null,
    });
  }

  // --- E2E navigateur : requis pour toute surface applicative/UI ou décision
  // non fiable. Conservateur : jamais de faux négatif sur un chemin UI connu ;
  // un chemin inconnu (FULL) exige aussi le navigateur par prudence.
  const changedPaths = changed.flatMap((change) => change.entries.map((entry) => entry.path));
  const browserReasons = [];
  const requireBrowser = (reason) => {
    if (!browserReasons.includes(reason)) browserReasons.push(reason);
  };
  if (uncertain) {
    requireBrowser('mapping incertain (fail-closed) : E2E navigateur exigés par prudence');
  }
  for (const changedPath of changedPaths) {
    if (under(changedPath, 'apps')) requireBrowser(`application/UI modifiée: ${changedPath}`);
    if (under(changedPath, 'crates/ravum-todo-core')) requireBrowser(`contrat applicatif modifié: ${changedPath}`);
    if (under(changedPath, 'crates/ravum-store')) requireBrowser(`stockage applicatif modifié: ${changedPath}`);
    if (changedPath === 'scripts/demo-m4-browser.ps1') requireBrowser('script E2E navigateur modifié');
  }
  if (!uncertain && unknownPaths.length > 0) {
    requireBrowser(`chemin inconnu (${uniqueSorted(unknownPaths)[0]}) : E2E navigateur exigés par prudence`);
  }
  if (full) {
    const manifestChanged = changedPaths.some((changedPath) => {
      const base = path.posix.basename(changedPath);
      return base === 'Cargo.toml' || base === 'Cargo.lock';
    });
    const uiChanged = changedPaths.some(
      (changedPath) =>
        under(changedPath, 'apps') ||
        under(changedPath, 'crates/ravum-todo-core') ||
        under(changedPath, 'crates/ravum-store'),
    );
    if (manifestChanged) requireBrowser('décision FULL + manifeste de construction modifié');
    if (uiChanged) requireBrowser('décision FULL + surface applicative/UI modifiée');
  }
  const browserE2eRequired = browserReasons.length > 0;
  if (browserE2eRequired) {
    const extra = browserReasons.length > 1 ? ` (+${browserReasons.length - 1} autre(s) raison(s))` : '';
    reasons.push(`browser-e2e — requis: ${browserReasons[0]}${extra}`);
  } else {
    reasons.push('browser-e2e — non requis: aucun chemin apps/todo-core/store/script E2E, pas d’incertitude, pas de FULL+manifeste/UI');
  }

  return {
    schema: SCHEMA,
    /**
     * Issue de la décision :
     *   - 'selective'        : tests ciblés, exécution sélective ;
     *   - 'full-conservative': FULL par règle conservatrice de contenu — issue
     *     LÉGITIME, la suite complète est l'exigence de ce diff et le job
     *     réussit/échoue selon le résultat réel des tests ;
     *   - 'full-uncertain'   : mapping INCERTAIN / erreur interne — la suite
     *     complète est exécutée par sécurité puis la CI échoue explicitement.
     */
    decision: full ? (uncertain ? 'full-uncertain' : 'full-conservative') : 'selective',
    full,
    uncertain,
    packages: full ? [] : packages,
    packages_scope: full ? 'workspace' : 'impacted',
    direct_packages: [...directPackages].sort(),
    naive_packages: [...directPackages].sort(),
    browser_e2e_required: browserE2eRequired,
    browser_e2e_reason: browserReasons[0] ?? null,
    browser_e2e_reasons: browserReasons,
    commands,
    reasons: dedupe(reasons),
    unknown_paths: uniqueSorted(unknownPaths),
    notes: dedupe(notes),
    limits: [...LIMITS],
    docs_required: docsRequired,
    components,
    changed,
    provenance: provenanceMap,
  };
}

// ---------------------------------------------------------------------------
// Lecture des changements (git ou synthétique)
// ---------------------------------------------------------------------------

function runGit(root, args) {
  const result = spawnSync('git', args, {
    cwd: root,
    encoding: 'buffer',
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  return {
    ok: !result.error && result.status === 0,
    status: result.status ?? -1,
    stdout: result.stdout ? result.stdout.toString('utf8') : '',
    stderr: result.stderr ? result.stderr.toString('utf8') : '',
    error: result.error ?? null,
  };
}

export function refExists(root, ref) {
  if (!ref) return false;
  const result = runGit(root, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]);
  return result.ok && result.stdout.trim().length > 0;
}

/** Parse la sortie NUL-terminée de `git diff --name-status -z`. */
export function parseNameStatusZ(output) {
  const tokens = String(output).split('\u0000');
  const changes = [];
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (!token) continue;
    const status = token.trim();
    const head = status[0];
    if (head === 'R' || head === 'C') {
      const from = tokens[i + 1];
      const to = tokens[i + 2];
      i += 2;
      if (from && to) changes.push({ status, from, to });
    } else {
      const file = tokens[i + 1];
      i += 1;
      if (file && /^[ACDMRTUXB]/.test(status)) changes.push({ status, path: file });
    }
  }
  return changes;
}

/**
 * Résout le diff réel `<base>...<head>`. Base explicite : aucune substitution
 * silencieuse (une base erronée rendrait le mapping incertain). Base par défaut :
 * `origin/main` puis `main` (fallback documenté). `head` vaut `HEAD` par défaut
 * et permet de rejouer un diff historique ou de comparer deux refs.
 */
export function resolveGitChanges(root, explicitBase = null, headRef = 'HEAD') {
  const head = headRef || 'HEAD';
  if (!refExists(root, head)) {
    throw new ImpactError(`référence de tête Git introuvable: ${head}`);
  }
  const candidates = explicitBase ? [explicitBase] : ['origin/main', 'main'];
  const tried = [];
  for (const candidate of candidates) {
    if (!candidate) continue;
    tried.push(candidate);
    if (!refExists(root, candidate)) continue;
    const diff = runGit(root, [
      '-c',
      'core.quotepath=false',
      'diff',
      '--name-status',
      '-z',
      '--diff-filter=ACDMRTUXB',
      '-M',
      `${candidate}...${head}`,
    ]);
    if (!diff.ok) {
      throw new ImpactError(`git diff ${candidate}...${head} a échoué: ${diff.stderr.trim() || diff.error?.message || 'raison inconnue'}`);
    }
    return { base: candidate, head, base_explicit: Boolean(explicitBase), tried, changes: parseNameStatusZ(diff.stdout) };
  }
  throw new ImpactError(
    `aucune base Git résolvable parmi [${candidates.join(', ')}] (essayées: ${tried.join(', ') || 'aucune'})`,
  );
}

/**
 * Parse `--files` : diff synthétique. Entrées séparées par `,` ou `;`.
 * Formes acceptées : `chemin`, `M:chemin`, `A:chemin`, `D:chemin`,
 * `R100:ancien->nouveau`, `C100:ancien->nouveau` (statut 1-4 caractères).
 */
export function parseFilesArg(text) {
  const raw = String(text ?? '').trim();
  if (!raw) return [];
  const entries = raw
    .split(/[;,]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  return entries.map((entry) => {
    const match = /^([ACDMRTUXB][0-9]{0,3}):(.*)$/.exec(entry);
    if (match && match[2] && !match[2].startsWith('\\') && !match[2].startsWith('/')) {
      const status = match[1];
      const rest = match[2];
      if (status[0] === 'R' || status[0] === 'C') {
        const [from, to] = rest.split('->');
        if (!from || !to) {
          throw new ImpactError(`renommage synthétique invalide: ${entry} (format attendu R100:ancien->nouveau)`);
        }
        return { status, from: normalizePath(from), to: normalizePath(to) };
      }
      return { status, path: normalizePath(rest) };
    }
    return { status: 'M', path: normalizePath(entry) };
  });
}

// ---------------------------------------------------------------------------
// Rapport lisible
// ---------------------------------------------------------------------------

export function renderExplain(result, workspace) {
  const lines = [];
  const push = (line = '') => lines.push(line);
  push('Ravum — graphe d’impact conservateur (MVP-0b L4-E)');
  push('===================================================');
  push(`Mode: ${result.mode ?? 'inconnu'}${result.base ? ` — base ${result.base}` : ''}`);
  push(`Schéma: ${result.schema} — Node ${process.version}`);
  push('');
  push(`FICHIERS MODIFIÉS (${result.changed.length})`);
  if (result.changed.length === 0) push('  (aucun)');
  for (const change of result.changed) {
    const describe = change.from && change.to ? `${change.status} ${change.from} -> ${change.to}` : `${change.status} ${change.path}`;
    const paths = change.from && change.to ? [change.from, change.to] : [change.path];
    const components = uniqueSorted(paths.flatMap((filePath) => result.components[filePath] ?? []));
    push(`  ${describe}`);
    push(`      composants: ${components.join(', ') || '(aucun)'}`);
  }
  push('');
  if (result.full) {
    if (result.uncertain) {
      push('DÉCISION: SUITE COMPLÈTE (FULL) — MAPPING INCERTAIN');
      push('  issue : la suite complète est exécutée par sécurité PUIS la CI doit échouer explicitement.');
    } else {
      push('DÉCISION: SUITE COMPLÈTE (FULL) — RÈGLE CONSERVATRICE (issue légitime)');
      push('  issue : la suite complète EST l’exigence de ce diff; le job réussit si les tests passent, échoue sinon.');
    }
    push(`  mapping incertain: ${result.uncertain ? 'OUI' : 'non'}`);
    push('  raisons:');
    for (const reason of result.reasons) push(`    - ${reason}`);
    for (const unknown of result.unknown_paths) push(`  chemin inconnu: ${unknown}`);
    push('');
    push('COMMANDES REQUISES (sérialisées)');
    for (const command of result.commands) {
      push(`  - ${command.kind}: ${command.argv.join(' ')}`);
      push(`      raison: ${command.reason}`);
    }
  } else {
    push('DÉCISION: SÉLECTIVE');
    push('');
    push(`PAQUETS DIRECTEMENT MODIFIÉS: ${result.direct_packages.join(', ') || '(aucun)'}`);
    push('');
    push('FERMETURE DES DÉPENDANTS (tests requis)');
    if (result.packages.length === 0) push('  (aucun paquet Rust)');
    for (const packageName of result.packages) {
      const info = result.provenance[packageName];
      push(`  - ${packageName}  [${info?.label ?? 'inconnu'}]`);
    }
    const skipped = result.packages.filter((packageName) => !result.naive_packages.includes(packageName));
    push('');
    push('PREUVE CONTRE UNE CLASSIFICATION NAÏVE');
    push(`  classification naïve (crates directement modifiés seulement): ${result.naive_packages.join(', ') || '(aucun)'}`);
    push(`  tests/paquets qu’une classification naïve aurait SAUTÉS: ${skipped.length > 0 ? skipped.join(', ') : '(aucun sur ce diff)'}`);
    push('');
    push('COMMANDES REQUISES (sérialisées, --test-threads=1)');
    if (result.commands.length === 0) push('  (aucune — aucun test automatisé requis pour ce diff)');
    for (const command of result.commands) {
      push(`  - ${command.kind}: ${command.argv.join(' ')}`);
      push(`      raison: ${command.reason}`);
    }
    if (result.unknown_paths.length > 0) {
      push('');
      push(`CHEMINS INCONNUS: ${result.unknown_paths.join(', ')}`);
    }
  }
  push('');
  if (result.browser_e2e_required) {
    push(`E2E NAVIGATEUR: REQUIS — ${result.browser_e2e_reason}`);
    for (const extra of (result.browser_e2e_reasons ?? []).slice(1)) push(`  autre raison: ${extra}`);
  } else {
    push('E2E NAVIGATEUR: non requis');
  }
  push('');
  push('NOTES');
  if (result.notes.length === 0) push('  (aucune)');
  for (const note of result.notes) push(`  - ${note}`);
  push('');
  push('LIMITES ASSUMÉES');
  for (const limit of result.limits) push(`  - ${limit}`);
  push('');
  push('RAPPEL: ce graphe sélectionne des tests, il ne qualifie ni compilateur, ni preuve, ni ABI, ni temps réel.');
  void workspace;
  return `${lines.join('\n')}\n`;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function fallbackDecision(message) {
  return {
    schema: SCHEMA,
    decision: 'full-uncertain',
    full: true,
    uncertain: true,
    browser_e2e_required: true,
    browser_e2e_reason: 'décision de secours impossible : E2E navigateur exigés par prudence (fail-closed)',
    browser_e2e_reasons: ['décision de secours impossible : E2E navigateur exigés par prudence (fail-closed)'],
    packages: [],
    packages_scope: 'workspace',
    direct_packages: [],
    naive_packages: [],
    commands: [
      {
        id: 'cargo-test-workspace',
        kind: 'cargo',
        argv: [...FULL_TEST_ARGV],
        reason: `FULL: décision de secours — ${message}`,
        package: null,
      },
    ],
    reasons: [`full:fallback — ${message}`, 'uncertain:mapping — décision FULL de secours, la CI doit échouer explicitement'],
    unknown_paths: [],
    notes: [],
    limits: [...LIMITS],
    docs_required: false,
    components: {},
    changed: [],
    provenance: {},
    mode: 'fallback',
    base: null,
    git: null,
  };
}

function emitFallbackForExplain(result) {
  return renderExplain(result, null);
}

function printUsage(stream = process.stdout) {
  stream.write(
    [
      'Usage: node tools/impact/impact.mjs [options]',
      '',
      'Options:',
      '  --base <ref>        Base Git du diff (défaut: origin/main puis main).',
      '                      Une base explicite introuvable rend le mapping FULL+INCERTAIN.',
      '  --head <ref>        Tête Git du diff (défaut: HEAD). Sert à rejouer un diff historique.',
      '  --files <liste>     Diff synthétique (séparateur , ou ;).',
      '                      Formes: chemin | M:chemin | D:chemin | R100:ancien->nouveau.',
      '  --json              Sortie JSON (défaut sans --explain).',
      '  --explain           Rapport lisible (stdout; avec --json, envoyé sur stderr).',
      '  --fail-on-full      Code de sortie 4 si la décision est FULL (garde locale).',
      '  --dump-graph        Affiche le graphe de dépendances interne et quitte.',
      '  --repo <dir>        Racine du dépôt (défaut: racine du dépôt courant).',
      '  --help              Cette aide.',
      '',
      'Codes de sortie: 0 décision rendue | 2 FULL+INCERTAIN | 3 usage | 4 FULL demandé via --fail-on-full',
      '',
    ].join('\n'),
  );
}

function buildResultMeta(result, extra) {
  return { ...result, ...extra };
}

function main(argv) {
  let parsed;
  try {
    parsed = parseArgs({
      args: argv,
      allowPositionals: false,
      strict: true,
      options: {
        base: { type: 'string' },
        head: { type: 'string' },
        files: { type: 'string' },
        json: { type: 'boolean', default: false },
        explain: { type: 'boolean', default: false },
        'fail-on-full': { type: 'boolean', default: false },
        'dump-graph': { type: 'boolean', default: false },
        repo: { type: 'string' },
        help: { type: 'boolean', default: false },
      },
    });
  } catch (err) {
    process.stderr.write(`impact: argument invalide: ${err.message}\n`);
    printUsage(process.stderr);
    return 3;
  }
  const options = parsed.values;
  if (options.help) {
    printUsage(process.stdout);
    return 0;
  }
  const root = options.repo ? path.resolve(options.repo) : REPO_ROOT;
  const wantJson = options.json || !options.explain;
  const wantExplain = options.explain;
  const emit = (result, exitCode) => {
    if (wantJson) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (wantExplain) {
      const text = result.mode === 'fallback' ? emitFallbackForExplain(result) : renderExplain(result, null);
      if (wantJson) process.stderr.write(text);
      else process.stdout.write(text);
    }
    return exitCode;
  };

  let workspace;
  try {
    workspace = loadWorkspace(root);
  } catch (err) {
    const result = fallbackDecision(`graphe de dépendances impossible: ${err.message}`);
    result.mode = 'fallback';
    return emit(result, 2);
  }

  if (options['dump-graph']) {
    const graph = {
      schema: SCHEMA,
      root,
      packages: [...workspace.packages.values()]
        .map((pkg) => ({ name: pkg.name, dir: pkg.dir, manifest: pkg.manifest, deps: pkg.deps }))
        .sort((a, b) => a.name.localeCompare(b.name)),
      errors: workspace.errors,
      notes: workspace.notes,
    };
    process.stdout.write(`${JSON.stringify(graph, null, 2)}\n`);
    return workspace.errors.length > 0 ? 2 : 0;
  }

  const mode = typeof options.files === 'string' ? 'files' : 'git';
  let changes;
  let gitInfo = null;
  try {
    if (mode === 'files') {
      changes = parseFilesArg(options.files);
    } else {
      gitInfo = resolveGitChanges(root, options.base ?? null, options.head ?? 'HEAD');
      changes = gitInfo.changes;
    }
  } catch (err) {
    const result = fallbackDecision(err.message);
    result.mode = mode;
    return emit(result, 2);
  }

  let result;
  try {
    result = analyze(workspace, changes);
  } catch (err) {
    const fallback = fallbackDecision(`analyse impossible: ${err.message}`);
    fallback.mode = mode;
    return emit(fallback, 2);
  }

  result = buildResultMeta(result, {
    mode,
    base: gitInfo ? gitInfo.base : null,
    head: gitInfo ? gitInfo.head : null,
    base_explicit: gitInfo ? gitInfo.base_explicit : null,
    base_candidates_tried: gitInfo ? gitInfo.tried : null,
    git: gitInfo,
  });

  let exitCode = 0;
  if (result.uncertain) exitCode = 2;
  if (result.full && options['fail-on-full']) exitCode = 4;
  if (result.full && !options['fail-on-full'] && result.uncertain) exitCode = 2;
  return emit(result, exitCode);
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  process.exitCode = main(process.argv.slice(2));
}

export { main };

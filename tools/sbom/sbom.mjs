#!/usr/bin/env node
// ravum-sbom — generateur SPDX 2.3 minimal et deterministe pour le workspace Ravum.
//
// Contraintes :
//   - Node standard uniquement (fs, path, crypto, process) : aucune dependance npm,
//     aucun acces reseau.
//   - Entree : la sortie de `cargo metadata --format-version 1 --locked`
//     (fichier via --metadata-file, sinon stdin). Aucun appel a cargo ici.
//   - Sortie : SPDX-2.3 JSON, ordre stable (packages et relations tries),
//     horodatage neutralisable (--timestamp ou SOURCE_DATE_EPOCH).
//
// Ce n'est PAS une attestation : aucune signature, aucun fichier analyse,
// aucune conclusion de licence (licenseConcluded = NOASSERTION). Voir
// docs/release/SBOM.md et docs/release/PROVENANCE.md.
//
// Sorties : 0 succes, 1 erreur (entree/manifeste), 2 usage invalide.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname } from 'node:path';

const EXIT_OK = 0;
const EXIT_FAIL = 1;
const EXIT_USAGE = 2;
const TOOL = 'ravum-sbom-1.0';

function fail(message, code = EXIT_FAIL) {
  console.error(`sbom: error: ${message}`);
  process.exit(code);
}

function usage() {
  console.error(
    'Usage: node tools/sbom/sbom.mjs [--metadata-file FILE] [--out FILE]\n' +
    '         [--name NAME] [--commit COMMIT] [--timestamp ISO8601]\n' +
    '  Lit `cargo metadata --format-version 1 --locked` sur stdin si --metadata-file est absent.\n' +
    '  SOURCE_DATE_EPOCH (secondes) fixe `created` si --timestamp est absent.'
  );
}

function parseArgs(argv) {
  const opts = { metadataFile: null, out: null, name: 'ravum-workspace', commit: 'nogit', timestamp: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const take = () => {
      if (i + 1 >= argv.length) fail(`valeur manquante pour ${arg}`, EXIT_USAGE);
      return argv[++i];
    };
    switch (arg) {
      case '--metadata-file': opts.metadataFile = take(); break;
      case '--out': opts.out = take(); break;
      case '--name': opts.name = take(); break;
      case '--commit': opts.commit = take(); break;
      case '--timestamp': opts.timestamp = take(); break;
      case '-h': case '--help': usage(); process.exit(EXIT_OK); break;
      default: fail(`argument inconnu: ${arg}`, EXIT_USAGE);
    }
  }
  return opts;
}

function sanitize(value) {
  const cleaned = String(value).replace(/[^A-Za-z0-9.-]+/g, '-').replace(/-+/g, '-').replace(/^-|-$/g, '');
  return cleaned.length > 0 ? cleaned : 'unknown';
}

function shortHash(value) {
  return createHash('sha256').update(String(value)).digest('hex').slice(0, 8);
}

function isoNow(opts) {
  if (opts.timestamp) {
    if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(opts.timestamp)) {
      fail('--timestamp doit etre au format 1970-01-01T00:00:00Z', EXIT_USAGE);
    }
    return opts.timestamp;
  }
  const epoch = process.env.SOURCE_DATE_EPOCH;
  if (epoch !== undefined && epoch !== '') {
    const seconds = Number(epoch);
    if (!Number.isInteger(seconds) || seconds < 0) {
      fail('SOURCE_DATE_EPOCH doit etre un entier de secondes >= 0', EXIT_USAGE);
    }
    return new Date(seconds * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');
  }
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

const opts = parseArgs(process.argv.slice(2));

let rawMetadata;
try {
  rawMetadata = opts.metadataFile ? readFileSync(opts.metadataFile, 'utf8') : readFileSync(0, 'utf8');
} catch (error) {
  fail(`lecture des metadonnees impossible: ${error.message}`);
}
if (!rawMetadata || rawMetadata.trim() === '') {
  fail('metadonnees vides: fournir `cargo metadata --format-version 1 --locked`');
}

let metadata;
try {
  metadata = JSON.parse(rawMetadata);
} catch (error) {
  fail(`JSON de metadonnees invalide: ${error.message}`);
}

const packages = Array.isArray(metadata.packages) ? metadata.packages : null;
const workspaceMembers = Array.isArray(metadata.workspace_members) ? metadata.workspace_members : null;
if (!packages || packages.length === 0) fail('aucun package dans les metadonnees');
if (!workspaceMembers || workspaceMembers.length === 0) fail('workspace_members absent ou vide');

const workspace = new Set(workspaceMembers);
const byId = new Map();
for (const pkg of packages) {
  if (!pkg || typeof pkg.id !== 'string' || typeof pkg.name !== 'string' || typeof pkg.version !== 'string') {
    fail('package sans id/name/version dans les metadonnees');
  }
  byId.set(pkg.id, pkg);
}

// Identifiants SPDX stables et uniques (name+version peut collisionner).
const baseIds = new Map();
for (const pkg of packages) {
  const base = `SPDXRef-Package-${sanitize(pkg.name)}-${sanitize(pkg.version)}`;
  baseIds.set(pkg.id, base);
}
const baseCounts = new Map();
for (const base of baseIds.values()) baseCounts.set(base, (baseCounts.get(base) || 0) + 1);
const spdxId = new Map();
for (const pkg of packages) {
  const base = baseIds.get(pkg.id);
  spdxId.set(pkg.id, baseCounts.get(base) > 1 ? `${base}-${shortHash(pkg.id)}` : base);
}

const sortedPackages = [...packages].sort((a, b) => {
  const ka = `${a.name}\u0000${a.version}\u0000${a.id}`;
  const kb = `${b.name}\u0000${b.version}\u0000${b.id}`;
  return ka < kb ? -1 : ka > kb ? 1 : 0;
});

const spdxPackages = sortedPackages.map((pkg) => {
  const declared = typeof pkg.license === 'string' && pkg.license.trim() !== '' ? pkg.license.trim() : 'NOASSERTION';
  const entry = {
    SPDXID: spdxId.get(pkg.id),
    name: pkg.name,
    versionInfo: pkg.version,
    downloadLocation: 'NOASSERTION',
    filesAnalyzed: false,
    licenseConcluded: 'NOASSERTION',
    licenseDeclared: declared,
    copyrightText: 'NOASSERTION',
    supplier: 'NOASSERTION',
    externalRefs: [
      {
        referenceCategory: 'PACKAGE-MANAGER',
        referenceType: 'purl',
        referenceLocator: `pkg:cargo/${pkg.name}@${pkg.version}`,
      },
    ],
    comment: workspace.has(pkg.id) ? 'Workspace member (path dependency, not published).' : 'Resolved Cargo dependency.',
  };
  return entry;
});

// Relations DEPENDS_ON depuis le graphe resolu.
const nodes = Array.isArray(metadata.resolve?.nodes) ? metadata.resolve.nodes : [];
const nodePackage = new Map();
for (const node of nodes) {
  if (node && typeof node.id === 'string') nodePackage.set(node.id, node);
}

const relationshipKeys = new Set();
const dependsOn = [];
for (const node of nodes) {
  const nodeInfo = nodePackage.get(node.id);
  if (!nodeInfo) continue;
  const from = spdxId.get(node.id);
  if (!from) continue;
  const deps = Array.isArray(node.deps) ? node.deps : [];
  for (const dep of deps) {
    const to = dep && spdxId.get(dep.pkg);
    if (!to || to === from) continue;
    const key = `${from}\u0000${to}`;
    if (relationshipKeys.has(key)) continue;
    relationshipKeys.add(key);
    dependsOn.push({ from, to });
  }
}
dependsOn.sort((a, b) => {
  const ka = `${a.from}\u0000${a.to}`;
  const kb = `${b.from}\u0000${b.to}`;
  return ka < kb ? -1 : ka > kb ? 1 : 0;
});

const roots = [...workspace]
  .filter((id) => byId.has(id) && spdxId.has(id))
  .map((id) => spdxId.get(id))
  .sort();

const relationships = [];
for (const root of roots) {
  relationships.push({ spdxElementId: 'SPDXRef-DOCUMENT', relationshipType: 'DESCRIBES', relatedSpdxElement: root });
}
for (const edge of dependsOn) {
  relationships.push({ spdxElementId: edge.from, relationshipType: 'DEPENDS_ON', relatedSpdxElement: edge.to });
}

const created = isoNow(opts);
const docName = sanitize(opts.name);
const commit = sanitize(opts.commit || 'nogit');
const documentNamespace = `https://spdx.ravum.invalid/${docName}-${commit}`;

const document = {
  spdxVersion: 'SPDX-2.3',
  dataLicense: 'CC0-1.0',
  SPDXID: 'SPDXRef-DOCUMENT',
  name: docName,
  documentNamespace,
  creationInfo: {
    created,
    creators: [`Tool: ${TOOL}`],
    comment:
      'Generated from `cargo metadata --format-version 1 --locked` without network access. ' +
      'License fields are copied from Cargo metadata; absent licenses are NOASSERTION. ' +
      'No file was analyzed (filesAnalyzed=false); this is not a signed attestation.',
  },
  documentDescribes: roots,
  packages: spdxPackages,
  relationships,
};

const serialized = `${JSON.stringify(document, null, 2)}\n`;

if (opts.out) {
  try {
    mkdirSync(dirname(opts.out), { recursive: true });
    writeFileSync(opts.out, serialized, { encoding: 'utf8' });
  } catch (error) {
    fail(`ecriture de ${opts.out} impossible: ${error.message}`);
  }
} else {
  process.stdout.write(serialized);
}

const workspaceCount = sortedPackages.filter((pkg) => workspace.has(pkg.id)).length;
console.error(
  `sbom: packages=${spdxPackages.length} (workspace=${workspaceCount}, dependencies=${spdxPackages.length - workspaceCount}) ` +
  `relationships=${relationships.length} created=${created}`
);
process.exit(EXIT_OK);

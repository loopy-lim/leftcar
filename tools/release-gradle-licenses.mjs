import {readFile, readdir} from 'node:fs/promises';
import {join, relative} from 'node:path';
import {DOMParser} from '@xmldom/xmldom';
import {sha256} from './release-source.mjs';

const validPart = value => typeof value === 'string' && /^[A-Za-z0-9_.+\-]+$/.test(value);
const children = (element, name) => [...element.childNodes].filter(node => node.nodeType === 1 && node.localName === name);
const hasDuplicateFields = element => {
  const names = [...element.childNodes].filter(node => node.nodeType === 1).map(node => node.localName);
  return new Set(names).size !== names.length;
};
const child = (element, name) => children(element, name)[0];
const value = (element, name) => child(element, name)?.textContent.trim() || null;
const evidence = (path, text, coordinate) => ({path, sha256: sha256(text), coordinate});

async function cachedPomLicense(cache, coordinate, seen = new Set()) {
  if (coordinate.some(part => !validPart(part)) || seen.size >= 16) return null;
  const key = coordinate.join(':');
  if (seen.has(key)) return null;
  const nextSeen = new Set([...seen, key]);
  const base = join(cache, 'caches/modules-2/files-2.1', ...coordinate);
  const candidates = [];
  for (const dir of await readdir(base).catch(() => [])) {
    for (const file of await readdir(join(base, dir)).catch(() => [])) {
      if (file.endsWith('.pom')) {
        const path = join(base, dir, file);
        candidates.push({path, text: await readFile(path, 'utf8')});
      }
    }
  }
  if (!candidates.length || new Set(candidates.map(pom => sha256(pom.text))).size !== 1) return null;
  const {path, text} = candidates[0];
  let project;
  try {
    // Cached POMs are data. Never resolve external entities or accept parser recovery.
    if (/<!DOCTYPE|<!ENTITY/i.test(text)) return null;
    project = new DOMParser({onError: () => { throw new Error('Invalid POM XML'); }})
      .parseFromString(text, 'application/xml').documentElement;
  } catch { return null; }
  // XML syntax alone does not enforce Maven's singleton model fields.
  if (project.localName !== 'project' || hasDuplicateFields(project)) return null;
  const parent = child(project, 'parent');
  if (parent && hasDuplicateFields(parent)) return null;
  const inherited = name => value(project, name) ?? (parent ? value(parent, name) : null);
  const identity = [inherited('groupId'), value(project, 'artifactId'), inherited('version')];
  if (identity.some((part, index) => part !== coordinate[index])) return null;
  const source = evidence(relative(cache, path), text, key);
  const licensesElement = child(project, 'licenses');
  const licenses = licensesElement ? children(licensesElement, 'license') : [];
  if (licenses.length) {
    if (licenses.some(hasDuplicateFields)) return null;
    const names = licenses.map(license => value(license, 'name'));
    // A child owns its nonempty license list even if we cannot read all names.
    // Maven inherits a parent's list only when the child's list is empty.
    if (names.some(name => name === null)) return null;
    return {license: [...new Set(names)].sort().join('; '), licenseSource: seen.size ? 'cached-parent-pom' : 'cached-pom', licenseEvidence: [source]};
  }
  if (!parent) return null;
  const result = await cachedPomLicense(cache, ['groupId', 'artifactId', 'version'].map(name => value(parent, name)), nextSeen);
  return result ? {...result, licenseEvidence: [source, ...result.licenseEvidence]} : null;
}

async function localPublications(root) {
  const modules = join(root, 'node_modules');
  const directories = [];
  for (const name of await readdir(modules).catch(() => [])) {
    if (name.startsWith('@')) {
      for (const packageName of await readdir(join(modules, name)).catch(() => [])) directories.push(join(modules, name, packageName));
    } else if (!name.startsWith('.')) directories.push(join(modules, name));
  }
  const publications = new Map();
  for (const directory of directories) {
    try {
      const configPath = join(directory, 'expo-module.config.json');
      const packagePath = join(directory, 'package.json');
      const configText = await readFile(configPath, 'utf8');
      const packageText = await readFile(packagePath, 'utf8');
      const publication = JSON.parse(configText).android?.publication;
      const pkg = JSON.parse(packageText);
      if (!publication || publication.repository !== 'local-maven-repo' || publication.version !== pkg.version || typeof pkg.license !== 'string' || !pkg.license.trim()) continue;
      const coordinate = [publication.groupId, publication.artifactId, publication.version];
      if (coordinate.some(part => !validPart(part))) continue;
      const key = coordinate.join(':');
      // Ambiguous publication ownership stays unknown.
      const result = {license: pkg.license, licenseSource: 'npm-local-publication', licenseEvidence: [
        evidence(relative(root, configPath), configText, key),
        evidence(relative(root, packagePath), packageText, `${pkg.name}@${pkg.version}`),
      ]};
      publications.set(key, publications.has(key) ? null : result);
    } catch { /* Missing or invalid local metadata remains unknown. */ }
  }
  return publications;
}

export async function collectGradleLicenses(root, coordinates, cache) {
  const local = await localPublications(root);
  const licenses = new Map();
  for (const coordinate of coordinates) {
    const key = coordinate.join(':');
    const result = await cachedPomLicense(cache, coordinate) ?? local.get(key);
    if (result) licenses.set(key, result);
  }
  return licenses;
}

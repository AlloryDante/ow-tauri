// The app uid formula of Overwolf's public CLI (`ow client calc-electron-uid`,
// @overwolf/ow-cli): sha1 of "{'author':'<author>','name':'<name>.electron'}",
// each digest byte written as two letters 'a' + low nibble, 'a' + high nibble.
// The harness uses it to test which package.json inputs ow-electron feeds in.

import { createHash } from 'node:crypto';

/**
 * Computes the uid for an author string and an app name.
 * @param {string} author
 * @param {string} name
 * @returns {string} 40 characters in a..p
 */
export function electronUid(author, name) {
  const key = `{'author':'${author}','name':'${name}.electron'}`;
  const digest = createHash('sha1').update(key, 'utf8').digest();
  let out = '';
  for (const byte of digest) {
    out += String.fromCharCode(97 + (byte & 15)) + String.fromCharCode(97 + (byte >> 4));
  }
  return out;
}

/**
 * Parses an npm-style person string "Name <email> (url)" into its parts.
 * @param {string} value
 */
export function parsePerson(value) {
  const match = /^\s*([^<(]*?)\s*(?:<([^>]*)>)?\s*(?:\(([^)]*)\))?\s*$/.exec(value);
  if (!match) return { name: value };
  return { name: match[1], email: match[2], url: match[3] };
}

/**
 * Every plausible (author, name) input pair derived from a package.json, so an
 * observed uid can be explained by exactly one of them.
 * @param {Record<string, any>} pkg
 * @param {string} [appGetName] what Electron's app.getName() returned
 */
export function uidCandidates(pkg, appGetName) {
  const authors = new Map();
  const a = pkg.author;
  if (a && typeof a === 'object') {
    authors.set('author.name', a.name);
    authors.set('JSON(author)', JSON.stringify(a));
  } else if (typeof a === 'string') {
    authors.set('author (verbatim string)', a);
    authors.set('author (npm-parsed name)', parsePerson(a).name);
  }
  authors.set('empty string', '');
  authors.set('"undefined"', 'undefined');
  authors.set('"null"', 'null');

  const names = new Map();
  if (pkg.build && pkg.build.productName) names.set('build.productName', pkg.build.productName);
  if (pkg.productName) names.set('productName', pkg.productName);
  if (pkg.name) names.set('name', pkg.name);
  if (appGetName) names.set('app.getName()', appGetName);
  names.set('empty string', '');
  names.set('"undefined"', 'undefined');

  for (const map of [authors, names]) {
    for (const [source, value] of [...map]) {
      if (typeof value === 'string' && value.trim() !== value)
        map.set(`trim(${source})`, value.trim());
    }
  }

  const out = [];
  for (const [authorSource, author] of authors) {
    if (author === undefined) continue;
    for (const [nameSource, name] of names) {
      out.push({ authorSource, author, nameSource, name, uid: electronUid(author, name) });
    }
  }
  return out;
}

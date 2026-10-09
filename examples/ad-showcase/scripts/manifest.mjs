// The staged manifest: the app's package.json with the local identity
// (identity.local.json, git-ignored) merged in. Used by stage.mjs and by the
// lab runner, so both hosts run with the same uid.

/** Identity keys copied from the identity file. */
export const IDENTITY_KEYS = ['name', 'productName', 'author', 'version'];

/**
 * Merges an identity into a manifest. `uid` becomes `overwolf.uid` (a
 * console-assigned uid, used verbatim by both hosts).
 *
 * @param {Record<string, unknown>} base the app's package.json
 * @param {Record<string, unknown> | null} identity the identity file, or null
 * @returns {Record<string, unknown>} the staged manifest
 */
export function stagedManifest(base, identity) {
  const pkg = { ...base };
  if (!identity) return pkg;
  for (const key of IDENTITY_KEYS) {
    if (identity[key] !== undefined) pkg[key] = identity[key];
  }
  if (identity.uid !== undefined && identity.uid !== null && identity.uid !== '') {
    if (typeof identity.uid !== 'string' || !/^[A-Za-z0-9]{1,64}$/.test(identity.uid.trim())) {
      throw new Error('identity uid must be 1 to 64 ASCII letters or digits');
    }
    pkg.overwolf = { ...(pkg.overwolf ?? {}), uid: identity.uid.trim() };
  }
  return pkg;
}

/**
 * The author name the uid formula uses: a string `author`, or `author.name`
 * (as ow-electron reads `package.json`).
 *
 * @param {unknown} author
 * @returns {string | undefined}
 */
function authorOf(author) {
  if (typeof author === 'string' && author !== '') return author;
  if (author && typeof author === 'object' && typeof author.name === 'string' && author.name)
    return author.name;
  return undefined;
}

/**
 * The `tauri.conf.json` override that gives the Tauri app the staged
 * identity: product name and version, and `plugins.overwolf` author, name
 * and (when the identity has one) uid, so both hosts run with the same uid.
 * Used as `tauri dev --config`, `tauri build --config` and the lab's
 * `TAURI_CONFIG`.
 *
 * @param {Record<string, unknown>} manifest the staged manifest
 * @returns {Record<string, unknown>} the override
 */
export function tauriConfig(manifest) {
  const productName =
    typeof manifest.productName === 'string' && manifest.productName !== ''
      ? manifest.productName
      : manifest.name;
  const overwolf = { name: productName };
  const author = authorOf(manifest.author);
  if (author !== undefined) overwolf.author = author;
  const uid =
    manifest.overwolf && typeof manifest.overwolf === 'object' ? manifest.overwolf.uid : undefined;
  if (typeof uid === 'string' && uid !== '') overwolf.uid = uid;
  return { productName, version: manifest.version, plugins: { overwolf } };
}

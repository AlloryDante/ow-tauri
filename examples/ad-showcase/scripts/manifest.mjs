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

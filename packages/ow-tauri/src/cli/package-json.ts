/**
 * The packaged form of `package.json` that the signing service receives
 * (`docs/CONTRACT.md` G.3, G.4 step a), and the builder's gating flags.
 *
 * It reproduces what Overwolf's published builder sends: `build.extraMetadata`
 * deep-merged into `package.json`, then the build-only keys removed. Unlike
 * the builder, no `electronVersion` is added.
 *
 * @packageDocumentation
 */

/** A JSON object. */
export type JsonObject = Record<string, unknown>;

/** Keys the builder never ships. */
const IGNORED_KEYS = new Set([
  'dist',
  'gitHead',
  'build',
  'jspm',
  'ava',
  'xo',
  'nyc',
  'eslintConfig',
  'contributors',
  'bundleDependencies',
  'tags',
]);

/**
 * Whether `value` is a plain JSON object (not an array, not null).
 *
 * @param value - any value
 * @returns `true` for an object
 */
export function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Merges `source` into `target` the way the builder's `deepAssign` does:
 * objects merge key by key, everything else (arrays included) replaces.
 *
 * @param target - the object to change
 * @param source - the values to merge in
 * @returns `target`
 */
export function deepAssign(target: JsonObject, source: JsonObject): JsonObject {
  for (const [key, value] of Object.entries(source)) {
    const current = target[key];
    if (isObject(value) && isObject(current)) {
      deepAssign(current, value);
    } else if (isObject(value)) {
      target[key] = deepAssign({}, value);
    } else {
      target[key] = value;
    }
  }
  return target;
}

/**
 * The `build` block of `package.json` (electron-builder configuration), or
 * an empty object.
 *
 * @param pkg - the parsed `package.json`
 * @returns the `build` object
 */
export function buildConfig(pkg: JsonObject): JsonObject {
  const build = pkg['build'];
  return isObject(build) ? build : {};
}

/**
 * The `build.overwolf` block, or an empty object.
 *
 * @param pkg - the parsed `package.json`
 * @returns the `build.overwolf` object
 */
export function buildOverwolf(pkg: JsonObject): JsonObject {
  const ow = buildConfig(pkg)['overwolf'];
  return isObject(ow) ? ow : {};
}

/**
 * The packaged form of `package.json` (G.3): `build.extraMetadata` merged
 * in, then `_*` keys, the builder's ignored keys, `scripts` and `keywords`
 * (unless `build.removePackageScripts` / `removePackageKeywords` is
 * `false`), `devDependencies`, and `babel` when no dependency is a Babel
 * package, removed.
 *
 * @param pkg - the parsed `package.json`
 * @returns a new object; `pkg` is not changed
 */
export function packagedForm(pkg: JsonObject): JsonObject {
  const build = buildConfig(pkg);
  const out = deepAssign({}, pkg);
  const extra = build['extraMetadata'];
  if (isObject(extra)) deepAssign(out, extra);
  const removeScripts = build['removePackageScripts'] !== false;
  const removeKeywords = build['removePackageKeywords'] !== false;
  const deps = out['dependencies'];
  const removeBabel = isObject(deps) && !Object.keys(deps).some((name) => name.startsWith('babel'));
  for (const key of Object.keys(out)) {
    if (
      key.startsWith('_') ||
      IGNORED_KEYS.has(key) ||
      (removeScripts && key === 'scripts') ||
      (removeKeywords && key === 'keywords') ||
      key === 'devDependencies' ||
      (removeBabel && key === 'babel')
    ) {
      Reflect.deleteProperty(out, key);
    }
  }
  return out;
}

/**
 * The builder's truthy environment rule: set, not empty, not `0`, not
 * `false` (any case).
 *
 * @param value - the environment value
 * @returns whether it switches the option on
 */
export function envOn(value: string | undefined): boolean {
  return value !== undefined && value !== '' && value !== '0' && value.toLowerCase() !== 'false';
}

/**
 * `isOwSigningRequired`: signing is required unless
 * `build.overwolf.requireSigning` is `false`, or always when
 * `OW_REQUIRE_SIGNING` is on. The builder applies it to Windows builds only.
 *
 * @param flag - `build.overwolf.requireSigning`
 * @param env - the environment
 * @returns whether a failed signing fails the build
 */
export function isSigningRequired(flag: unknown, env: Record<string, string | undefined>): boolean {
  return flag !== false || envOn(env['OW_REQUIRE_SIGNING']);
}

/**
 * `isOwCertSigningEnabled`: `build.overwolf.enableOWCertSigning === true`
 * or `OW_ENABLE_CERT_SIGNING` on.
 *
 * @param flag - `build.overwolf.enableOWCertSigning`
 * @param env - the environment
 * @returns whether Overwolf certificate signing is asked for
 */
export function isCertSigningEnabled(
  flag: unknown,
  env: Record<string, string | undefined>,
): boolean {
  return flag === true || envOn(env['OW_ENABLE_CERT_SIGNING']);
}

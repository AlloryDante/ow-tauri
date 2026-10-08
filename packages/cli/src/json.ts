/// <reference types="node" />
/**
 * JSON helpers: RFC 7396 merge patch (what Tauri uses for config overlays),
 * reading JSON objects, and writing a file in the indentation it already has.
 *
 * @packageDocumentation
 */

import { readFile, rename, writeFile } from 'node:fs/promises';

/** A JSON object. */
export type JsonObject = Record<string, unknown>;

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
 * Applies a JSON Merge Patch (RFC 7396) as `json_patch::merge` does: an
 * object patch merges key by key and `null` removes a key; any other patch
 * replaces the target.
 *
 * @param target - the document (not changed)
 * @param patch - the patch
 * @returns the patched document
 */
export function mergePatch(target: unknown, patch: unknown): unknown {
  if (!isObject(patch)) return patch;
  const out: JsonObject = isObject(target) ? { ...target } : {};
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) Reflect.deleteProperty(out, key);
    else out[key] = mergePatch(out[key], value);
  }
  return out;
}

/**
 * Reads a file that must hold a JSON object.
 *
 * @param path - the file
 * @returns the object
 * @throws an `[OW]` error naming the file when it is missing, not JSON or not an object
 */
export async function readJsonObject(path: string): Promise<JsonObject> {
  let text: string;
  try {
    text = await readFile(path, 'utf8');
  } catch (error) {
    throw new Error(`[OW] cannot read ${path}: ${(error as Error).message}`, { cause: error });
  }
  return parseJsonObject(text, path);
}

/**
 * Parses JSON text that must be an object (a leading BOM is ignored).
 *
 * @param text - the text
 * @param origin - where it came from, for the error message
 * @returns the object
 */
export function parseJsonObject(text: string, origin: string): JsonObject {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text.replace(/^\uFEFF/, ''));
  } catch (error) {
    throw new Error(`[OW] ${origin} is not valid JSON: ${(error as Error).message}`, {
      cause: error,
    });
  }
  if (!isObject(parsed)) throw new Error(`[OW] ${origin} is not a JSON object`);
  return parsed;
}

/**
 * The indentation of JSON text: a tab, or the width of the first indented
 * line; two spaces when the text has none.
 *
 * @param text - JSON text
 * @returns the `JSON.stringify` indent
 */
export function detectIndent(text: string): string | number {
  const match = /^([ \t]+)\S/m.exec(text);
  if (!match?.[1]) return 2;
  return match[1].startsWith('\t') ? '\t' : match[1].length;
}

/**
 * Formats a JSON document with the indentation of `previous` (two spaces
 * for a new file) and a final newline.
 *
 * @param value - the document
 * @param previous - the file's current text, if any
 * @returns the text
 */
export function formatJson(value: unknown, previous?: string): string {
  return `${JSON.stringify(value, null, previous === undefined ? 2 : detectIndent(previous))}\n`;
}

/**
 * Writes a file through a temporary sibling and a rename.
 *
 * @param path - the target
 * @param data - the content
 */
export async function writeAtomic(path: string, data: string | Buffer): Promise<void> {
  const temp = `${path}.ow-tmp`;
  await writeFile(temp, data);
  await rename(temp, path);
}

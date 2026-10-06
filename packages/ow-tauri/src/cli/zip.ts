/// <reference types="node" />
/**
 * A minimal ZIP reader for the signing service's archives: the central
 * directory, stored and deflated entries (the same subset Overwolf's builder
 * reads).
 *
 * @packageDocumentation
 */

import { inflateRawSync } from 'node:zlib';

const EOCD = 0x06054b50;
const CENTRAL = 0x02014b50;
const LOCAL = 0x04034b50;

/** The largest archive entry the reader inflates (512 MiB). */
const MAX_ENTRY_BYTES = 512 * 1024 * 1024;

/**
 * Reads every file of a ZIP archive.
 *
 * @param buf - the archive bytes
 * @returns entry name to its bytes
 * @throws `Error` when the archive is malformed or uses an unsupported
 *   compression method
 */
export function readZipEntries(buf: Buffer): Map<string, Buffer> {
  const entries = new Map<string, Buffer>();
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== EOCD) eocd--;
  if (eocd < 0) throw new Error('[OW] signing response ZIP has no end of central directory');
  const total = buf.readUInt16LE(eocd + 10);
  let cd = buf.readUInt32LE(eocd + 16);
  for (let i = 0; i < total; i++) {
    if (cd + 46 > buf.length || buf.readUInt32LE(cd) !== CENTRAL) {
      throw new Error('[OW] signing response ZIP has a broken central directory');
    }
    const method = buf.readUInt16LE(cd + 10);
    const compressedSize = buf.readUInt32LE(cd + 20);
    const nameLen = buf.readUInt16LE(cd + 28);
    const extraLen = buf.readUInt16LE(cd + 30);
    const commentLen = buf.readUInt16LE(cd + 32);
    const localOffset = buf.readUInt32LE(cd + 42);
    const name = buf.toString('utf8', cd + 46, cd + 46 + nameLen);
    if (localOffset + 30 > buf.length || buf.readUInt32LE(localOffset) !== LOCAL) {
      throw new Error(`[OW] signing response ZIP entry "${name}" has no local header`);
    }
    const localNameLen = buf.readUInt16LE(localOffset + 26);
    const localExtraLen = buf.readUInt16LE(localOffset + 28);
    const start = localOffset + 30 + localNameLen + localExtraLen;
    if (start + compressedSize > buf.length) {
      throw new Error(`[OW] signing response ZIP entry "${name}" is truncated`);
    }
    const data = buf.subarray(start, start + compressedSize);
    if (!name.endsWith('/')) {
      if (method === 0) {
        entries.set(name, Buffer.from(data));
      } else if (method === 8) {
        entries.set(name, inflateRawSync(data, { maxOutputLength: MAX_ENTRY_BYTES }));
      } else {
        throw new Error(
          `[OW] signing response ZIP entry "${name}" uses compression method ${String(method)}`,
        );
      }
    }
    cd += 46 + nameLen + extraLen + commentLen;
  }
  return entries;
}

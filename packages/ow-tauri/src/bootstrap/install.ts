/**
 * Installs the runtime global and lets the npm entry points attach to it
 * (`docs/CONTRACT.md` section B, "One runtime per webview").
 *
 * @packageDocumentation
 */
import { OwTauriError } from '../shared/errors.js';
import {
  CONTRACT_VERSION,
  PACKAGE_VERSION,
  RUNTIME_API_VERSION,
  type HostContext,
} from '../shared/protocol.js';
import { Kernel, RUNTIME_GLOBAL } from './kernel.js';
import { createProcessShim } from './process-shim.js';

/** Registry key of the kernel on the runtime global. */
const KERNEL = Symbol.for('ow-tauri.kernel');

/**
 * The object at `globalThis.__OW_TAURI_RUNTIME__`: non-writable and
 * non-configurable, installed before any page script.
 */
export interface RuntimeGlobal {
  /** Package version of the injected runtime. */
  readonly version: string;
  /** Contract version of the injected runtime. */
  readonly contract: number;
  /** Facade API version of the injected runtime (`FacadeKernel`). */
  readonly api: number;
  /** Where the runtime runs. */
  readonly context: HostContext;
  /**
   * `window_eval` expression form (CONTRACT A.2.3).
   *
   * @param id - evaluation id
   * @param fn - returns the value
   */
  evalBegin(id: number, fn: () => unknown): void;
  /**
   * `window_eval` statement form (CONTRACT A.2.3).
   *
   * @param id - evaluation id
   * @param fn - runs the statements
   */
  evalFallback(id: number, fn: () => unknown): void;
}

function existing(): (RuntimeGlobal & { [KERNEL]?: Kernel }) | undefined {
  const value = (globalThis as Record<string, unknown>)[RUNTIME_GLOBAL];
  return typeof value === 'object' && value !== null
    ? (value as RuntimeGlobal & { [KERNEL]?: Kernel })
    : undefined;
}

/**
 * Installs the runtime global (once per document) and starts the kernel.
 * When a runtime is already installed, returns its kernel instead.
 *
 * @returns the document's kernel
 */
export function installRuntime(): Kernel {
  const found = existing();
  if (found) return attachTo(found);
  const kernel = new Kernel();
  const runtime = Object.freeze({
    version: kernel.version,
    contract: kernel.contract,
    api: kernel.api,
    get context(): HostContext {
      return kernel.context;
    },
    evalBegin: (id: number, fn: () => unknown) => {
      kernel.evalBegin(id, fn);
    },
    evalFallback: (id: number, fn: () => unknown) => {
      kernel.evalFallback(id, fn);
    },
    [KERNEL]: kernel,
  });
  Object.defineProperty(globalThis, RUNTIME_GLOBAL, {
    value: runtime,
    writable: false,
    configurable: false,
    enumerable: false,
  });
  installProcessGlobal(kernel);
  void kernel.start();
  return kernel;
}

/**
 * Returns the document's kernel for an npm entry point: the installed one
 * when its contract and facade API versions match, a fresh installation when
 * none exists, and otherwise a private kernel on which every member throws
 * `OwTauriError('not-ready')` ("ow-tauri runtime a does not match package b").
 *
 * @returns the kernel
 */
export function attachRuntime(): Kernel {
  const found = existing();
  return found ? attachTo(found) : installRuntime();
}

/**
 * Attaches to an installed runtime global (see {@link attachRuntime}).
 *
 * @param found - the runtime global
 * @returns its kernel, or a private kernel that throws `not-ready` on use
 * @internal
 */
export function attachTo(found: RuntimeGlobal & { [KERNEL]?: Kernel }): Kernel {
  const kernel = found[KERNEL];
  if (
    found.contract === CONTRACT_VERSION &&
    found.api === RUNTIME_API_VERSION &&
    kernel instanceof Object
  )
    return kernel;
  const api = typeof found.api === 'number' ? found.api : 'none';
  return new Kernel({
    mismatch: new OwTauriError(
      'not-ready',
      `ow-tauri runtime ${found.version} (contract ${String(found.contract)}, api ${String(api)}) does not match package ${PACKAGE_VERSION} (contract ${String(CONTRACT_VERSION)}, api ${String(RUNTIME_API_VERSION)})`,
      {
        data: {
          runtime: found.version,
          runtimeContract: found.contract,
          runtimeApi: found.api,
          package: PACKAGE_VERSION,
          contract: CONTRACT_VERSION,
          api: RUNTIME_API_VERSION,
        },
      },
    ),
  });
}

/**
 * Defines the global `process` shim (CONTRACT B.2.5) unless the document
 * already has a `process` (Node, or a bundler polyfill).
 *
 * @param kernel - the kernel
 */
export function installProcessGlobal(kernel: Kernel): void {
  if (kernel.context === 'none' || 'process' in globalThis) return;
  Object.defineProperty(globalThis, 'process', {
    value: kernel.singleton('process', () => createProcessShim(kernel)),
    writable: true,
    configurable: true,
    enumerable: false,
  });
}

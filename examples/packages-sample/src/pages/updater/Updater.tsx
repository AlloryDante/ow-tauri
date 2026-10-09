import { useReducer, useRef, type ReactElement } from 'react';
import { check, type Update } from 'tauri-plugin-overwolf-api/updater';

import { useLog } from '../../log/context';
import { describeError, logged } from '../../log/logged';
import { INITIAL, explain, percent, updaterReducer } from './progress';

/**
 * The upstream "check for updates", on the plugin's Overwolf updater:
 * `check()`, then `downloadAndInstall()` with its progress events. The
 * install starts the NSIS setup and exits the app (Windows).
 */
export function Updater(): ReactElement {
  const log = useLog();
  const [state, dispatch] = useReducer(updaterReducer, INITIAL);
  const update = useRef<Update | null>(null);

  const runCheck = async (): Promise<void> => {
    dispatch({ type: 'check' });
    const o = await logged(log, 'check()', () => check(), {
      source: 'updater',
      shown: (u) => (u ? { version: u.version, currentVersion: u.currentVersion } : null),
    });
    if (!o.ok) {
      dispatch({ type: 'failed', code: o.code, message: o.message });
      return;
    }
    await update.current?.close().catch(() => undefined);
    update.current = o.value;
    const found = o.value;
    dispatch({
      type: 'checked',
      update: found && {
        version: found.version,
        currentVersion: found.currentVersion,
        ...(found.date === undefined ? {} : { date: found.date }),
        ...(found.body === undefined ? {} : { body: found.body }),
      },
    });
  };

  const install = async (): Promise<void> => {
    const found = update.current;
    if (!found) return;
    dispatch({ type: 'download' });
    log.push('info', 'updater', `downloadAndInstall() ${found.version}`);
    try {
      await found.downloadAndInstall((event) => {
        dispatch({ type: 'event', event });
        if (event.event !== 'Progress')
          log.push('dim', 'updater', `download: ${event.event}`, event);
      });
    } catch (error) {
      const { code, message } = describeError(error);
      log.push('error', 'updater', `downloadAndInstall() failed: ${code}`, message);
      dispatch({ type: 'failed', code, message });
    }
  };

  const busy =
    state.phase === 'checking' || state.phase === 'downloading' || state.phase === 'installing';
  const pct = percent(state);

  return (
    <section className="page updater">
      <div className="settings-card">
        <header>
          <h3>Overwolf update feed</h3>
          <code className="permission">overwolf:updater</code>
        </header>
        <div className="call">
          <button
            type="button"
            className="btn-primary"
            disabled={busy}
            onClick={() => void runCheck()}
          >
            Check for updates
          </button>
        </div>
        <div className="updater-status" role="status">
          {state.phase === 'idle' && <p>Not checked yet.</p>}
          {state.phase === 'checking' && <p>Checking…</p>}
          {state.phase === 'up-to-date' && <p className="ok">Up to date.</p>}
          {state.phase === 'available' && (
            <>
              <p>
                Version <strong>{state.update.version}</strong> is available (running{' '}
                {state.update.currentVersion}
                {state.update.date ? `, released ${state.update.date}` : ''}).
              </p>
              {state.update.body && <pre className="notes">{state.update.body}</pre>}
              <button type="button" className="btn-primary" onClick={() => void install()}>
                Download and install
              </button>
            </>
          )}
          {(state.phase === 'downloading' || state.phase === 'installing') && (
            <>
              <p>
                {state.phase === 'installing'
                  ? 'Downloaded and verified; starting the installer…'
                  : 'Downloading…'}{' '}
                {String(state.downloaded)} bytes
                {state.phase === 'downloading' && state.total ? ` of ${String(state.total)}` : ''}
              </p>
              <progress
                max={100}
                {...(pct === null ? {} : { value: pct })}
                aria-label="Download progress"
              />
            </>
          )}
          {state.phase === 'error' && (
            <>
              <p className="err">
                <code>{state.code}</code>: {state.message}
              </p>
              {explain(state.code) && <p className="hint">{explain(state.code)}</p>}
            </>
          )}
        </div>
      </div>
    </section>
  );
}

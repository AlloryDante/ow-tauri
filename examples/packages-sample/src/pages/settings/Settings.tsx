import { useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from 'react';
import type { CMPTab, MachineIds, OverwolfInfo } from 'tauri-plugin-overwolf-api';

import { useLog } from '../../log/context';
import type { Outcome } from '../../log/logged';
import {
  PERMISSIONS,
  looksLikeEmail,
  mask,
  settingsActions,
  utmText,
  type SettingsCall,
} from './actions';

/** The consent window's tabs. */
const TABS: readonly CMPTab[] = ['purposes', 'features', 'vendors'];

/** The last outcome of each call, as one line. */
type Results = Partial<Record<SettingsCall, string>>;

/** One line for an outcome. */
function resultLine(outcome: Outcome<unknown>): string {
  return outcome.ok ? 'done' : `${outcome.code}: ${outcome.message}`;
}

/** A card of the page with its title and the permission set it needs. */
function Card({
  title,
  permission,
  children,
}: {
  title: string;
  permission: string;
  children: ReactNode;
}): ReactElement {
  return (
    <section className="settings-card" aria-label={title}>
      <header>
        <h3>{title}</h3>
        <code className="permission">{permission}</code>
      </header>
      {children}
    </section>
  );
}

/** A call button with its last result under it. */
function Call({
  label,
  result,
  onClick,
}: {
  label: string;
  result: string | undefined;
  onClick: () => void;
}): ReactElement {
  return (
    <div className="call">
      <button type="button" className="btn-secondary" onClick={onClick}>
        {label}
      </button>
      {result !== undefined && (
        <output className={result === 'done' ? 'ok' : 'err'}>{result}</output>
      )}
    </div>
  );
}

/** A call with a boolean argument: true and false buttons, one result. */
function Toggle({
  label,
  result,
  onSet,
}: {
  label: string;
  result: string | undefined;
  onSet: (enabled: boolean) => void;
}): ReactElement {
  return (
    <div className="call">
      <span className="mono">{label}(</span>
      <button
        type="button"
        className="btn-secondary"
        onClick={() => {
          onSet(true);
        }}
      >
        true
      </button>
      <button
        type="button"
        className="btn-secondary"
        onClick={() => {
          onSet(false);
        }}
      >
        false
      </button>
      <span className="mono">)</span>
      {result !== undefined && (
        <output className={result === 'done' ? 'ok' : 'err'}>{result}</output>
      )}
    </div>
  );
}

/**
 * The upstream CMP and app settings, Tauri-native: consent, e-mail hashes,
 * identity and machine ids, and the analytics and ads switches. Every call
 * and its result also goes to the Logger.
 */
export function Settings(): ReactElement {
  const log = useLog();
  const actions = useMemo(() => settingsActions(log), [log]);
  const [results, setResults] = useState<Results>({});
  const [info, setInfo] = useState<OverwolfInfo | null>(null);
  const [cmpRequired, setCmpRequired] = useState<boolean | null>(null);
  const [tab, setTab] = useState<CMPTab>('purposes');
  const [email, setEmail] = useState('');
  const [hashes, setHashes] = useState({ sha1: '', md5: '', sha256: '' });
  const [machineIds, setMachineIds] = useState<MachineIds | null>(null);
  const [reveal, setReveal] = useState(false);
  const loaded = useRef(false);

  const note = (call: SettingsCall, outcome: Outcome<unknown>): void => {
    setResults((r) => ({ ...r, [call]: resultLine(outcome) }));
  };

  useEffect(() => {
    // Once per page visit, also under StrictMode's second effect run.
    if (loaded.current) return;
    loaded.current = true;
    void actions.getInfo().then((o) => {
      if (o.ok) setInfo(o.value);
    });
    void actions.isCMPRequired().then((o) => {
      if (o.ok) setCmpRequired(o.value);
    });
  }, [actions]);

  const checkCmp = async (): Promise<void> => {
    const o = await actions.isCMPRequired();
    note('isCMPRequired', o);
    if (o.ok) setCmpRequired(o.value);
  };
  const generate = async (): Promise<void> => {
    if (!looksLikeEmail(email)) {
      setResults((r) => ({ ...r, generateUserEmailHashes: 'enter an e-mail address first' }));
      return;
    }
    const o = await actions.generateUserEmailHashes(email);
    note('generateUserEmailHashes', o);
    if (o.ok)
      setHashes({ sha1: o.value.sha1 ?? '', md5: o.value.md5 ?? '', sha256: o.value.sha256 ?? '' });
  };
  const setOwn = async (): Promise<void> => {
    const own = Object.fromEntries(Object.entries(hashes).filter(([, v]) => v.trim() !== ''));
    note('setUserEmailHashes', await actions.setUserEmailHashes(own));
  };
  const loadMachineIds = async (): Promise<void> => {
    const o = await actions.getMachineIds();
    note('getMachineIds', o);
    if (o.ok) setMachineIds(o.value);
  };
  const shownId = (id: string): string => (reveal ? id : mask(id));

  return (
    <section className="page settings">
      <div className="settings-grid">
        <Card title="Consent (CMP)" permission={PERMISSIONS.isCMPRequired}>
          <p className="status">
            CMP required:{' '}
            <strong data-testid="cmp-required">
              {cmpRequired === null ? '…' : cmpRequired ? 'yes' : 'no'}
            </strong>
          </p>
          <Call
            label="isCMPRequired()"
            result={results.isCMPRequired}
            onClick={() => void checkCmp()}
          />
          <label className="field">
            <span>Tab</span>
            <select
              value={tab}
              onChange={(e) => {
                setTab(e.target.value as CMPTab);
              }}
            >
              {TABS.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </label>
          <Call
            label="openAdPrivacySettingsWindow()"
            result={results.openAdPrivacySettingsWindow}
            onClick={() => {
              void actions.openAdPrivacySettingsWindow(tab).then((o) => {
                note('openAdPrivacySettingsWindow', o);
              });
            }}
          />
          <Call
            label="openCMPWindow() (deprecated name)"
            result={results.openCMPWindow}
            onClick={() => {
              void actions.openCMPWindow().then((o) => {
                note('openCMPWindow', o);
              });
            }}
          />
        </Card>

        <Card title="E-mail hashes" permission={PERMISSIONS.generateUserEmailHashes}>
          <label className="field">
            <span>E-mail</span>
            <input
              type="email"
              autoComplete="off"
              placeholder="player@example.com"
              value={email}
              onChange={(e) => {
                setEmail(e.target.value);
              }}
            />
          </label>
          <Call
            label="generateUserEmailHashes()"
            result={results.generateUserEmailHashes}
            onClick={() => void generate()}
          />
          {(['sha1', 'md5', 'sha256'] as const).map((key) => (
            <label key={key} className="field">
              <span>{key}</span>
              <input
                className="mono"
                spellCheck={false}
                value={hashes[key]}
                onChange={(e) => {
                  setHashes((h) => ({ ...h, [key]: e.target.value }));
                }}
              />
            </label>
          ))}
          <Call
            label="setUserEmailHashes(hashes)"
            result={results.setUserEmailHashes}
            onClick={() => void setOwn()}
          />
          <Call
            label="clearUserEmailHashes()"
            result={results.clearUserEmailHashes}
            onClick={() => {
              void actions.clearUserEmailHashes().then((o) => {
                note('clearUserEmailHashes', o);
              });
            }}
          />
        </Card>

        <Card title="Identity" permission={`${PERMISSIONS.getInfo}, ${PERMISSIONS.getMachineIds}`}>
          {info ? (
            <dl className="facts">
              <dt>uid</dt>
              <dd className="mono">{info.uid}</dd>
              <dt>app cuid</dt>
              <dd className="mono">{info.appCuid}</dd>
              <dt>phase</dt>
              <dd>{info.phasePercent}%</dd>
              <dt>ads</dt>
              <dd>
                {info.adsSupported ? 'supported' : 'not supported here'},{' '}
                {info.testAd ? 'test ads' : 'live ads'}
              </dd>
              <dt>host</dt>
              <dd>
                {info.host.label} {info.host.version}
              </dd>
              <dt>UTM</dt>
              <dd className="mono pre">{utmText(info)}</dd>
            </dl>
          ) : (
            <p className="status">Loading getInfo()…</p>
          )}
          <Call
            label="getMachineIds()"
            result={results.getMachineIds}
            onClick={() => void loadMachineIds()}
          />
          {machineIds && (
            <dl className="facts">
              <dt>muid</dt>
              <dd className="mono">{shownId(machineIds.muid)}</dd>
              <dt>muidV2</dt>
              <dd className="mono">{shownId(machineIds.muidV2)}</dd>
              <dt />
              <dd>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={reveal}
                    onChange={(e) => {
                      setReveal(e.target.checked);
                    }}
                  />
                  Show in full
                </label>
              </dd>
            </dl>
          )}
        </Card>

        <Card
          title="Analytics and ads"
          permission={`${PERMISSIONS.disableAdsFPD}, ${PERMISSIONS.setAnalyticsUserEnabled}`}
        >
          <p className="hint">For this launch only:</p>
          {(['disableAdsFPD', 'disableAdsOptimization', 'disableAnonymousAnalytics'] as const).map(
            (call) => (
              <Call
                key={call}
                label={`${call}()`}
                result={results[call]}
                onClick={() => {
                  void actions[call]().then((o) => {
                    note(call, o);
                  });
                }}
              />
            ),
          )}
          <p className="hint">Stored, from the next launch on:</p>
          <Toggle
            label="setAnonymousAnalyticsPreference"
            result={results.setAnonymousAnalyticsPreference}
            onSet={(enabled) => {
              void actions.setAnonymousAnalyticsPreference(enabled).then((o) => {
                note('setAnonymousAnalyticsPreference', o);
              });
            }}
          />
          <p className="hint">The user switch (plugins.overwolf.analytics.userSwitch):</p>
          <Toggle
            label="setAnalyticsUserEnabled"
            result={results.setAnalyticsUserEnabled}
            onSet={(enabled) => {
              void actions.setAnalyticsUserEnabled(enabled).then((o) => {
                note('setAnalyticsUserEnabled', o);
              });
            }}
          />
        </Card>
      </div>
    </section>
  );
}

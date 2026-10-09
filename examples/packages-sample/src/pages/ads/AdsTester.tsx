import { useCallback, useState, type ReactElement } from 'react';

import { useLog } from '../../log/context';
import { payloadOf } from '../../log/format';
import { LogView } from '../../log/LogView';
import { AdSlot } from './AdSlot';
import { DEFAULT_LAYOUT, LAYOUTS, layoutOf, sizeText, type SlotSize } from './formats';
import { showPerformanceAd } from './performance';

/** The log sources of the event panel. */
const AD_SOURCES = ['ad'] as const;

/** Which slot holds an expanded high impact ad. */
type Expanded = 'ad1' | 'ad2' | null;

/** The slot that gets `adstyle="high-impact-ad;"` in a high impact layout. */
function wantsHighImpact(highImpact: boolean, size: SlotSize): boolean {
  return highImpact && sizeText(size) === '400x600';
}

/**
 * The upstream ads tester: a layout picker, the app's window drawn with two
 * ad slots around its main area, the performance ad, and the ad events.
 */
export function AdsTester(): ReactElement {
  const log = useLog();
  const [layoutId, setLayoutId] = useState(DEFAULT_LAYOUT);
  const [expanded, setExpanded] = useState<Expanded>(null);
  const layout = layoutOf(layoutId);
  const highImpact = layout.highImpact === true;

  const onHighImpact1 = useCallback((on: boolean) => {
    setExpanded((e) => (on ? 'ad1' : e === 'ad1' ? null : e));
  }, []);
  const onHighImpact2 = useCallback((on: boolean) => {
    setExpanded((e) => (on ? 'ad2' : e === 'ad2' ? null : e));
  }, []);

  const performance = (): void => {
    const el = showPerformanceAd((name, event) => {
      log.push('success', 'ad', `performance ad: ${name}`, payloadOf(event));
    });
    log.push(
      el ? 'info' : 'warn',
      'ad',
      el ? 'performance ad: appended to the page' : 'performance ad: one is already showing',
    );
  };

  const slot = (name: 'ad1' | 'ad2', size: SlotSize, onHi: (on: boolean) => void): ReactElement => (
    <div
      className={`slot-cell ${expanded === name ? 'expanded' : ''}`}
      hidden={expanded !== null && expanded !== name}
    >
      <AdSlot
        key={`${layout.id}-${name}`}
        name={name}
        cid={`sample-${name}`}
        size={size}
        highImpact={wantsHighImpact(highImpact, size)}
        onHighImpact={onHi}
      />
    </div>
  );

  return (
    <section className="page ads-tester">
      <div className="layout-actions">
        <label className="select-layout">
          <span>Ad layout</span>
          <select
            value={layout.id}
            onChange={(e) => {
              setExpanded(null);
              setLayoutId(e.target.value);
              log.push('info', 'ad', `layout: ${e.target.value}`);
            }}
          >
            {LAYOUTS.map((l) => (
              <option key={l.id} value={l.id}>
                {l.label}
              </option>
            ))}
          </select>
        </label>
        <button type="button" className="btn-secondary" onClick={performance}>
          Performance ad
        </button>
      </div>

      <div className="app-layout-container">
        <div className="app-layout-header" aria-hidden="true">
          <span className="fake-title">Your game app</span>
          <span className="fake-buttons">— ▢ ✕</span>
        </div>
        {highImpact ? (
          <div className="app-layout-main">
            <div className="main-area" />
            <div className="ad-zone">
              {slot('ad1', layout.ad1, onHighImpact1)}
              {slot('ad2', layout.ad2, onHighImpact2)}
            </div>
          </div>
        ) : (
          <div className="app-layout-main">
            <div className="side-col">{slot('ad1', layout.ad1, onHighImpact1)}</div>
            <div className="main-area" />
            <div className="side-col">{slot('ad2', layout.ad2, onHighImpact2)}</div>
          </div>
        )}
      </div>

      <h3 className="sub-title">Ad events</h3>
      <div className="ad-log">
        <LogView sources={AD_SOURCES} search={false} label="Ad events" />
      </div>
    </section>
  );
}

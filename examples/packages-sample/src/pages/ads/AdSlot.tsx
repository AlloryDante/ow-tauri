import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { OwAdViewElement } from 'tauri-plugin-overwolf-api/adview';
// Types only: declares <owadview> as a JSX element.
import type {} from 'tauri-plugin-overwolf-api/jsx';

import { useLog } from '../../log/context';
import { payloadOf } from '../../log/format';
import { SLOT_EVENTS, isVideoSize, sizeText, type SlotSize } from './formats';

/** The upstream sample's custom tracking value. */
export const CUSTOM_TRACKING = JSON.stringify({ testQAKey: 'testQAValue' });

/** Props of {@link AdSlot}. */
export interface AdSlotProps {
  /** The slot's name in the log and on its buttons (`ad1`, `ad2`). */
  name: string;
  /** The container id (at most 20 characters, unique in the window). */
  cid: string;
  /** The slot size. */
  size: SlotSize;
  /** Mount the ad right away (default `false`: the Start button does). */
  autoStart?: boolean;
  /** Ask for a high impact ad (`adstyle="high-impact-ad;"`). */
  highImpact?: boolean;
  /** Called with `true` on `high-impact-ad-loaded`, `false` on `high-impact-ad-removed` or removal. */
  onHighImpact?: (active: boolean) => void;
}

/**
 * One ad slot of the ads tester: a box of the slot size and the upstream
 * buttons (start, remove), plus recreate and mute. The `<owadview>` events
 * are DOM events with underscore names, which React's `on*` props do not
 * map, so the listeners go on through a `ref`. Recreate gives the element a
 * new React key: React removes the old element (the plugin closes its ad)
 * and mounts a fresh one.
 */
export function AdSlot({
  name,
  cid,
  size,
  autoStart = false,
  highImpact = false,
  onHighImpact,
}: AdSlotProps): ReactElement {
  const log = useLog();
  const ref = useRef<OwAdViewElement>(null);
  const [active, setActive] = useState(autoStart);
  const [instance, setInstance] = useState(0);
  const [muted, setMuted] = useState(false);
  const title = `${name} ${sizeText(size)}${highImpact ? ' (HI)' : ''}${isVideoSize(size) ? ' (video)' : ''}`;

  useEffect(() => {
    const el = ref.current;
    if (!active || !el) return undefined;
    const removers = SLOT_EVENTS.map((event) => {
      const listener = (e: Event): void => {
        log.push(
          event.startsWith('did-fail') ? 'warn' : 'success',
          'ad',
          `${title}: ${event}`,
          payloadOf(e),
        );
        if (event === 'high-impact-ad-loaded') onHighImpact?.(true);
        if (event === 'high-impact-ad-removed') onHighImpact?.(false);
      };
      el.addEventListener(event, listener);
      return () => {
        el.removeEventListener(event, listener);
      };
    });
    return () => {
      removers.forEach((remove) => {
        remove();
      });
    };
  }, [active, instance, log, title, onHighImpact]);

  const start = (): void => {
    if (active) return;
    log.push('info', 'ad', `${title}: start`);
    setMuted(false);
    setActive(true);
  };
  const remove = (): void => {
    if (!active) return;
    log.push('info', 'ad', `${title}: remove`);
    setActive(false);
    onHighImpact?.(false);
  };
  const recreate = (): void => {
    log.push('info', 'ad', `${title}: recreate`);
    setMuted(false);
    setInstance((n) => n + 1);
    setActive(true);
    onHighImpact?.(false);
  };
  const toggleMute = (): void => {
    const el = ref.current;
    if (!el) return;
    const next = !muted;
    try {
      el.setAudioMuted(next);
      log.push('info', 'ad', `${title}: setAudioMuted(${String(next)})`);
      setMuted(next);
    } catch (error) {
      log.push('error', 'ad', `${title}: setAudioMuted failed`, String(error));
    }
  };

  return (
    <div className="ad-wrapper" data-slot={name}>
      <div
        className="ad-container"
        style={{ width: size[0], height: size[1] }}
        aria-label={`${title} ad slot`}
      >
        {active && (
          <owadview
            key={instance}
            ref={ref}
            cid={cid}
            slotsize={sizeText(size)}
            customtracking={CUSTOM_TRACKING}
            {...(highImpact ? { adstyle: 'high-impact-ad;' } : {})}
          />
        )}
      </div>
      <div className="ad-actions">
        <span>{title}:</span>
        <button type="button" className="ad-btn" onClick={start} disabled={active}>
          Start ad
        </button>
        <button type="button" className="ad-btn" onClick={remove} disabled={!active}>
          Remove ad
        </button>
        <button type="button" className="ad-btn" onClick={recreate}>
          Recreate
        </button>
        <button
          type="button"
          className="ad-btn"
          onClick={toggleMute}
          disabled={!active}
          aria-pressed={muted}
        >
          {muted ? 'Unmute' : 'Mute'}
        </button>
      </div>
    </div>
  );
}

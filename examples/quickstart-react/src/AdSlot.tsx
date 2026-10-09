import { useEffect, useRef } from 'react';
// Types only: declares <owadview> as a JSX element.
import type {} from 'tauri-plugin-overwolf-api/jsx';
import type { OwAdViewElement } from 'tauri-plugin-overwolf-api/adview';

/** Props of {@link AdSlot}. */
export interface AdSlotProps {
  /** The container id (at most 20 characters). */
  cid: string;
  /** The slot size, `"WxH"`. */
  width: number;
  /** See `width`. */
  height: number;
  /** Called with each ad event's name (`display_ad_loaded`, `impression`, ...). */
  onAdEvent?: (name: string) => void;
}

/** The ad events this example listens to. */
const EVENTS = ['display_ad_loaded', 'impression', 'video_ad_ready', 'complete'] as const;

/**
 * One `<owadview>` in a box of its slot size. The element's events are DOM
 * events with underscore names, which React's `on*` props do not map, so the
 * listeners go on through a `ref`. Under StrictMode the effect runs, cleans
 * up and runs again; the element itself stays mounted, so the ad loads once.
 */
export function AdSlot({ cid, width, height, onAdEvent }: AdSlotProps) {
  const ref = useRef<OwAdViewElement>(null);

  useEffect(() => {
    const ad = ref.current;
    if (!ad || !onAdEvent) return undefined;
    const listeners = EVENTS.map((name) => {
      const listener = () => onAdEvent(name);
      ad.addEventListener(name, listener);
      return () => ad.removeEventListener(name, listener);
    });
    return () => listeners.forEach((remove) => remove());
  }, [onAdEvent]);

  return (
    <div className="ad-container" style={{ width, height }}>
      <owadview ref={ref} cid={cid} slotsize={`${width}x${height}`} />
    </div>
  );
}

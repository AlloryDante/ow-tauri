// Type-level tests of the public API as an app sees it (`npm run typecheck`).
import 'tauri-plugin-overwolf-api/adview';
import { adview, type OwAdViewElement } from 'tauri-plugin-overwolf-api/adview';
import type {} from 'tauri-plugin-overwolf-api/jsx';
import {
  OverwolfError,
  getInfo,
  isCMPRequired,
  openAdPrivacySettingsWindow,
  setExternalPaymentUserId,
  type OverwolfInfo,
} from 'tauri-plugin-overwolf-api';
import { check, type DownloadEvent } from 'tauri-plugin-overwolf-api/updater';
import { mockOverwolf } from 'tauri-plugin-overwolf-api/testing';

export async function app(): Promise<void> {
  const info: OverwolfInfo = await getInfo();
  const uid: string = info.uid;
  const host: string = info.host.label;
  const required: boolean = await isCMPRequired();
  await openAdPrivacySettingsWindow({ tab: 'vendors', modal: true });
  // @ts-expect-error the tab names are fixed
  await openAdPrivacySettingsWindow({ tab: 'other' });
  await setExternalPaymentUserId({ providerName: 'tebex', userId: 12 });
  // @ts-expect-error userId is required
  await setExternalPaymentUserId({ providerName: 'tebex' });

  const ad: OwAdViewElement = document.createElement('owadview');
  ad.setAudioMuted(true);
  ad.addEventListener('display_ad_loaded', () => undefined);
  const tracked: HTMLElement[] = adview.elements();

  const update = await check({ allowPrerelease: true });
  if (update) {
    await update.downloadAndInstall((event: DownloadEvent) => {
      if (event.event === 'Progress') console.log(event.data.chunkLength);
    });
  }

  try {
    await getInfo();
  } catch (error) {
    if (error instanceof OverwolfError && error.code === 'forbidden') console.log(error.message);
  }
  console.log(uid, host, required, tracked);
  mockOverwolf({ label: 'main', info: { testAd: false } }).restore();
}

export function Banner(): React.JSX.Element {
  return (
    <div style={{ width: 400, height: 300 }}>
      <owadview cid="main-mrec" slotsize="400x300" performance="" />
    </div>
  );
}

export function Wrong(): React.JSX.Element {
  // @ts-expect-error slotsize is a string
  return <owadview cid="x" slotsize={400} />;
}

import 'tauri-plugin-overwolf-api/adview'; // installs the <owadview> runtime in this page
import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

window.addEventListener('DOMContentLoaded', async () => {
  const info = await getInfo();
  console.log(`uid ${info.uid}, test ads ${info.testAd}`);

  const ad = document.querySelector('owadview')!;
  ad.addEventListener('display_ad_loaded', () => console.log('ad loaded'));
  ad.addEventListener('impression', () => console.log('impression'));

  const privacy = document.querySelector<HTMLButtonElement>('#privacy')!;
  privacy.hidden = !(await isCMPRequired());
  privacy.addEventListener('click', () => void openAdPrivacySettingsWindow());
});

import { mockOverwolf, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { button, click, flush, render } from '../../test/render';
import { Settings } from './Settings';

let overwolf: MockOverwolf;
beforeEach(() => {
  overwolf = mockOverwolf({
    cmpRequired: true,
    info: { utmParams: { utm_source: 'site' } },
    machineIds: { muid: 'machine-one', muidV2: 'machine-two' },
  });
});
afterEach(() => {
  overwolf.restore();
});

describe('Settings', () => {
  it('loads the identity and the consent state once, also under StrictMode', async () => {
    const view = await render(<Settings />);
    await flush();
    expect(overwolf.callsOf('get_info')).toHaveLength(1);
    expect(overwolf.callsOf('is_cmp_required')).toHaveLength(1);
    expect(view.container.querySelector('[data-testid="cmp-required"]')?.textContent).toBe('yes');
    expect(view.container.textContent).toContain('utm_source=site');
    await view.unmount();
  });

  it('opens the privacy settings window on the chosen tab', async () => {
    const view = await render(<Settings />);
    const select = view.container.querySelector('select')!;
    select.value = 'vendors';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await flush();
    await click(button(view.container, 'openAdPrivacySettingsWindow()'));
    await flush();
    expect(overwolf.callsOf('open_ad_privacy_settings_window')).toEqual([
      { options: { tab: 'vendors' } },
    ]);
    expect(view.container.textContent).toContain('done');
    await view.unmount();
  });

  it('needs an e-mail address before generating hashes, then fills them in', async () => {
    const view = await render(<Settings />);
    await click(button(view.container, 'generateUserEmailHashes()'));
    await flush();
    expect(view.container.textContent).toContain('enter an e-mail address first');
    expect(overwolf.callsOf('generate_user_email_hashes')).toHaveLength(0);
    await view.unmount();
  });

  it('shows the machine ids masked until asked', async () => {
    const view = await render(<Settings />);
    await click(button(view.container, 'getMachineIds()'));
    await flush();
    expect(view.container.textContent).not.toContain('machine-one');
    expect(view.container.textContent).toContain('mach••••••••');
    const reveal = [...view.container.querySelectorAll('input[type="checkbox"]')].at(
      -1,
    ) as HTMLInputElement;
    await click(reveal);
    expect(view.container.textContent).toContain('machine-one');
    await view.unmount();
  });

  it('shows the result of a switch, including a refusal', async () => {
    overwolf.setCommand('set_analytics_user_enabled', () => {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the plugin rejects with wire objects
      throw { code: 'unsupported', message: 'userSwitch is off' };
    });
    const view = await render(<Settings />);
    await click(button(view.container, 'disableAdsFPD()'));
    const userSwitch = view.container.querySelectorAll('.call');
    const row = [...userSwitch].find((r) => r.textContent.includes('setAnalyticsUserEnabled'))!;
    await click(button(row, 'false'));
    await flush();
    expect(overwolf.callsOf('disable_ads_fpd')).toHaveLength(1);
    expect(overwolf.callsOf('set_analytics_user_enabled')).toEqual([{ enabled: false }]);
    expect(row.textContent).toContain('unsupported: userSwitch is off');
    await view.unmount();
  });
});

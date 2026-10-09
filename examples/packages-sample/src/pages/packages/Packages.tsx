import type { ReactElement } from 'react';

import { PACKAGES } from './package-list';

/**
 * The upstream sample's package pages (GEP, overlay, recorder, utility),
 * each shown as not available on Tauri with the reason.
 */
export function Packages(): ReactElement {
  return (
    <section className="page packages">
      <p className="hint">
        ow-electron loads Overwolf packages into its own patched Electron at run time. Tauri has no
        package runtime, so tauri-plugin-overwolf offers no package API in 1.0: ads, consent,
        analytics, identity and the Windows updater work, the packages below do not.
      </p>
      <ul className="package-list">
        {PACKAGES.map((p) => (
          <li key={p.id} className="settings-card" data-package={p.id}>
            <header>
              <h3>{p.name}</h3>
              <span className="badge">not available on Tauri</span>
            </header>
            <p>{p.does}</p>
            <p className="hint">{p.reason}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}

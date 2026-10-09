import { useEffect, useState, useSyncExternalStore, type ReactElement } from 'react';
import type { OverwolfInfo } from 'tauri-plugin-overwolf-api';

import { LogView } from './log/LogView';
import { PAGES, pageOf, type PageId } from './nav';
import { AdsTester } from './pages/ads/AdsTester';
import { Packages } from './pages/packages/Packages';
import { Settings } from './pages/settings/Settings';
import { Updater } from './pages/updater/Updater';

/** Calls `listener` when the URL hash changes. */
function onHashChange(listener: () => void): () => void {
  window.addEventListener('hashchange', listener);
  return () => {
    window.removeEventListener('hashchange', listener);
  };
}

/** The page of the current URL hash. */
function useHash(): string {
  return useSyncExternalStore(onHashChange, () => window.location.hash);
}

/** The body of a page. */
function PageBody({ id }: { id: PageId }): ReactElement {
  switch (id) {
    case 'logger':
      return (
        <section className="page logger">
          <LogView label="Application log" />
        </section>
      );
    case 'ads':
      return <AdsTester />;
    case 'settings':
      return <Settings />;
    case 'updater':
      return <Updater />;
    case 'packages':
      return <Packages />;
  }
}

/** Props of {@link App}. */
export interface AppProps {
  /** The app info the launch logged (`startup()`). */
  info: Promise<OverwolfInfo | null>;
}

/**
 * The sample's window: header, side navigation and the selected page (the
 * upstream sample's layout).
 */
export function App({ info }: AppProps): ReactElement {
  const page = pageOf(useHash());
  const [host, setHost] = useState('');

  useEffect(() => {
    let current = true;
    void info.then((i) => {
      if (current && i)
        setHost(`${i.host.label} ${i.host.version}${i.testAd ? ' · test ads' : ''}`);
    });
    return () => {
      current = false;
    };
  }, [info]);

  return (
    <>
      <header className="header">
        <h1>
          <span className="logo" aria-hidden="true" />
          ow-tauri Packages Sample
        </h1>
      </header>
      <aside className="side-bar">
        <nav className="main-menu" aria-label="Pages">
          <ul>
            {PAGES.map((p) => (
              <li key={p.id} className={p.id === page.id ? 'is-active' : ''}>
                <a href={`#${p.id}`} aria-current={p.id === page.id ? 'page' : undefined}>
                  {p.title}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <span className="host-version">{host}</span>
      </aside>
      <main className="main">
        <div className="page-title">
          <h2>{page.title}</h2>
          <p>{page.description}</p>
        </div>
        <PageBody id={page.id} />
      </main>
    </>
  );
}

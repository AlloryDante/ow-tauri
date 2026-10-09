import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
// Installs the <owadview> runtime in this page, once, before React renders.
import 'tauri-plugin-overwolf-api/adview';

import { App } from './App';
import { LogContext } from './log/context';
import { createLogStore } from './log/store';
import { startup } from './startup';
import './styles.css';

const log = createLogStore();
const info = startup(log);

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');
createRoot(root).render(
  <StrictMode>
    <LogContext value={log}>
      <App info={info} />
    </LogContext>
  </StrictMode>,
);

// The invisible lab's driver (e2e/README.md): only in the lab build
// (`vite build --mode lab`); every other build drops this branch and the
// driver module with it.
if (import.meta.env.MODE === 'lab') {
  void import('./lab/driver').then(({ startDriver }) => startDriver(log));
}

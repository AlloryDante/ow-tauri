import React from 'react';
import ReactDOM from 'react-dom/client';
// Installs the <owadview> runtime in this page, once, before React renders.
import 'tauri-plugin-overwolf-api/adview';

import App from './App';
import './App.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

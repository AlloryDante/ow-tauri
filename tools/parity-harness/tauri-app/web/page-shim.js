// Tauri edition of the parity harness: forwards the reports of the shared
// harness page (../../app/page.js, which logs them with the prefix
// __PARITY__) to the app, where ow-electron's harness reads them from the
// console. Everything else is logged as usual.
(() => {
  const PREFIX = '__PARITY__';
  const log = console.log.bind(console);
  console.log = (...args) => {
    const [first] = args;
    if (typeof first === 'string' && first.startsWith(PREFIX)) {
      let payload;
      try {
        payload = JSON.parse(first.slice(PREFIX.length));
      } catch {
        payload = { raw: first };
      }
      window.__TAURI_INTERNALS__
        .invoke('harness_page_event', { payload })
        .catch((error) => log('harness_page_event failed', String(error)));
      return;
    }
    log(...args);
  };
})();

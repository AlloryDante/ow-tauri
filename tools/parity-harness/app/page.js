// Parity harness renderer: creates one <owadview> per requested layout,
// exactly as Overwolf's official sample does (cid, slotsize, customTracking),
// and reports every element event to the main process through the console
// (prefix __PARITY__). It never dispatches input events.

(() => {
  // Event names used by the official sample and the ow-electron docs.
  const EVENTS = [
    'impression',
    'display_ad_loaded',
    'player_loaded',
    'play',
    'complete',
    'ad-clicked',
    'house_ad_action',
    'house-ad-action',
    'high-impact-ad-loaded',
    'high-impact-ad-removed',
    'shutdown',
    'performance_ad_no_fill',
    'performance_ad_dismiss',
    'performance_ad_loaded',
    'performance_ad_clicked',
    'performance_ad_video_complete',
    'performance_ad_video_skipped',
    'video_no_ready_ads',
    'video_no_impression_timeout',
    'video_ad_ready',
    'video_ad_skipped',
    'did-attach',
    'dom-ready',
    'did-finish-load',
    'did-fail-load',
    'crashed',
  ];

  const report = (payload) => console.log('__PARITY__' + JSON.stringify(payload));

  const describeEvent = (event) => {
    const own = {};
    for (const key of Object.keys(event)) {
      try {
        own[key] = JSON.parse(JSON.stringify(event[key]));
      } catch {
        own[key] = String(event[key]);
      }
    }
    let detail = null;
    try {
      detail = event.detail === undefined ? null : JSON.parse(JSON.stringify(event.detail));
    } catch {
      detail = String(event.detail);
    }
    return {
      constructor: event.constructor && event.constructor.name,
      bubbles: event.bubbles,
      cancelable: event.cancelable,
      detail,
      own,
    };
  };

  const params = new URLSearchParams(location.search);
  const layouts = (params.get('layouts') || '400x600').split(',').filter(Boolean);

  layouts.forEach((layout, index) => {
    const [width, height] = layout.split('x').map(Number);
    const slot = document.createElement('div');
    slot.className = 'slot';
    slot.style.width = `${width}px`;
    slot.style.height = `${height}px`;
    document.body.appendChild(slot);

    const cid = `parity_${layout}_${index}`.slice(0, 20);
    const ad = document.createElement('owadview');
    ad.setAttribute('id', `ad${index}`);
    ad.setAttribute('cid', cid);
    ad.setAttribute('slotsize', layout);
    ad.setAttribute('customTracking', JSON.stringify({ parityHarness: layout }));
    for (const name of EVENTS) {
      ad.addEventListener(name, (event) =>
        report({ kind: 'owadview-event', event: name, cid, layout, info: describeEvent(event) }),
      );
    }
    slot.appendChild(ad);
    report({
      kind: 'owadview-created',
      cid,
      layout,
      shadowRoot: Boolean(ad.shadowRoot),
      ownKeys: Object.getOwnPropertyNames(Object.getPrototypeOf(ad)),
    });
  });

  // The element's internals as the embedding page sees them, after it attached.
  setTimeout(() => {
    for (const ad of document.querySelectorAll('owadview')) {
      const root = ad.shadowRoot;
      report({
        kind: 'owadview-structure',
        cid: ad.getAttribute('cid'),
        attributes: Array.from(ad.attributes, (a) => [a.name, a.value]),
        shadowChildren: root ? Array.from(root.children, (c) => c.tagName.toLowerCase()) : null,
        rect: ad.getBoundingClientRect().toJSON(),
      });
    }
    report({
      kind: 'page-state',
      visibilityState: document.visibilityState,
      hasFocus: document.hasFocus(),
    });
  }, 5000);
})();

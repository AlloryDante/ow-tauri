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
    'render-process-gone',
    // Names no Overwolf page documents today. Listening costs nothing, and the
    // authoritative list of what the host dispatches is ipc.jsonl
    // (GUEST_VIEW_INTERNAL_DISPATCH_EVENT); these only show the page side.
    'reward',
    'rewarded',
    'reward_granted',
    'reward-granted',
    'performance_ad_reward',
    'performance_ad_rewarded',
    'performance_ad_reward_granted',
    'performance_ad_closed',
    'performance_ad_error',
    'house_ad_loaded',
    'house-ad-loaded',
    'video_ad_complete',
    'no_fill',
    'ad_error',
    'close',
  ];

  const report = (payload) => console.log('__PARITY__' + JSON.stringify(payload));
  // The OS hides the document of an occluded window (both engines); ad
  // pages then reload themselves, so record every change.
  document.addEventListener('visibilitychange', () =>
    report({ kind: 'page-visibility', visibilityState: document.visibilityState }),
  );

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
  const layouts = (params.get('layouts') || '400x600').split(',').filter((l) => l && l !== 'none');
  // Extra attributes: an object for every element, or an array (one per element).
  const attrs = params.get('attrs') ? JSON.parse(params.get('attrs')) : null;
  const attrsFor = (index) => (Array.isArray(attrs) ? attrs[index] || {} : attrs || {});
  const t0 = performance.now();
  const since = () => Math.round(performance.now() - t0);

  // Layout-relevant computed style of an element and its rect (performance
  // ads cover the window; high-impact ads grow into their zone).
  const STYLE_KEYS = [
    'display',
    'position',
    'top',
    'left',
    'right',
    'bottom',
    'width',
    'height',
    'zIndex',
    'pointerEvents',
    'visibility',
    'opacity',
    'background',
    'backdropFilter',
    'inset',
  ];
  const styleOf = (el) => {
    const cs = getComputedStyle(el);
    return Object.fromEntries(STYLE_KEYS.map((k) => [k, cs[k]]));
  };
  // A child of the element (light DOM or shadow root), three levels deep.
  const describeNode = (node, depth) => ({
    tag: node.tagName.toLowerCase(),
    attributes: Array.from(node.attributes, (a) => [a.name, a.value.slice(0, 200)]),
    rect: node.getBoundingClientRect().toJSON(),
    style: styleOf(node),
    children: depth < 3 ? Array.from(node.children, (c) => describeNode(c, depth + 1)) : [],
  });
  const describeElement = (ad) => {
    const root = ad.shadowRoot;
    return {
      cid: ad.getAttribute('cid'),
      connected: ad.isConnected,
      attributes: Array.from(ad.attributes, (a) => [a.name, a.value]),
      rect: ad.getBoundingClientRect().toJSON(),
      style: styleOf(ad),
      inlineStyle: ad.getAttribute('style'),
      parent: ad.parentElement
        ? {
            tag: ad.parentElement.tagName.toLowerCase(),
            id: ad.parentElement.id || null,
            rect: ad.parentElement.getBoundingClientRect().toJSON(),
          }
        : null,
      children: Array.from(ad.children, (c) => describeNode(c, 0)),
      shadowChildren: root ? Array.from(root.children, (c) => describeNode(c, 0)) : null,
    };
  };
  const viewport = () => ({
    inner: [innerWidth, innerHeight],
    body: document.body.getBoundingClientRect().toJSON(),
  });
  const sampleLayout = (reason) => {
    report({
      kind: 'layout-sample',
      reason,
      at: since(),
      viewport: viewport(),
      elements: Array.from(document.querySelectorAll('owadview'), describeElement),
      zone: (() => {
        const z = document.getElementById('ads-parent');
        return z
          ? {
              rect: z.getBoundingClientRect().toJSON(),
              children: Array.from(z.children, (c) => ({
                id: c.id,
                display: getComputedStyle(c).display,
                rect: c.getBoundingClientRect().toJSON(),
              })),
            }
          : null;
      })(),
    });
  };

  let created = 0;
  /**
   * Creates one <owadview> the way the docs and the official sample do.
   * spec: {layout?: 'WxH', parent?: 'slot'|'body'|'#id', attrs?: {}, cid?: string|null}
   * - parent 'slot' (default): a sized div, as for standard sizes;
   * - parent 'body': appended to <body> unsized, as the performance-ad docs do;
   * - parent '#id': appended to that existing element.
   */
  const addAd = (spec = {}) => {
    const index = created++;
    const layout = spec.layout || null;
    let parent;
    if (!spec.parent || spec.parent === 'slot') {
      const [width, height] = (layout || '300x250').split('x').map(Number);
      parent = document.createElement('div');
      parent.className = 'slot';
      parent.style.width = `${width}px`;
      parent.style.height = `${height}px`;
      document.body.appendChild(parent);
    } else if (spec.parent === 'body') {
      parent = document.body;
    } else {
      parent = document.querySelector(spec.parent);
    }
    const cid =
      spec.cid === null ? null : (spec.cid || `parity_${layout || 'perf'}_${index}`).slice(0, 20);
    const ad = document.createElement('owadview');
    ad.setAttribute('id', spec.id || `ad${index}`);
    if (cid !== null) ad.setAttribute('cid', cid);
    if (layout && spec.slotsize !== false) ad.setAttribute('slotsize', layout);
    if (spec.customTracking !== false) {
      ad.setAttribute('customTracking', JSON.stringify({ parityHarness: layout || 'performance' }));
    }
    for (const [name, value] of Object.entries(spec.attrs || {})) ad.setAttribute(name, value);
    for (const name of EVENTS) {
      ad.addEventListener(name, (event) => {
        report({
          kind: 'owadview-event',
          event: name,
          cid,
          layout,
          at: since(),
          info: describeEvent(event),
        });
        if (spec.sampleOnEvent !== false) setTimeout(() => sampleLayout(`after ${name}`), 50);
      });
    }
    // Removal by the host (performance ads remove themselves when done).
    const watch = new MutationObserver(() => {
      if (!ad.isConnected) {
        report({ kind: 'owadview-removed', cid, layout, at: since() });
        watch.disconnect();
      }
    });
    watch.observe(document.body, { childList: true, subtree: true });
    parent.appendChild(ad);
    report({
      kind: 'owadview-created',
      cid,
      layout,
      at: since(),
      spec,
      shadowRoot: Boolean(ad.shadowRoot),
      ownKeys: Object.getOwnPropertyNames(Object.getPrototypeOf(ad)),
    });
    for (const ms of [100, 1500, 5000, 15000])
      setTimeout(() => sampleLayout(`${cid} +${ms}ms`), ms);
    return cid;
  };

  /**
   * The documented high-impact ad zone (unique-ad-sizes/high-impact-ads):
   * #ads-parent 440 wide, window high (min 670), a 400x600 container whose
   * owadview has adstyle="high-impact-ad;", and a 400x60 container. The
   * documented listeners grow the large container and remove the small one on
   * high-impact-ad-loaded, and restore both on high-impact-ad-removed.
   */
  const addHighImpactZone = (spec = {}) => {
    const style = document.createElement('style');
    style.textContent = `#ads-parent{width:440px;min-height:670px;height:100vh;display:flex;
      flex-direction:column;align-items:center}#ads-container-large{width:400px;height:600px}
      #ads-container-small{width:400px;height:60px}
      #ads-container-large.high-impact-loaded{width:100%;height:100%}`;
    document.head.appendChild(style);
    document.body.style.padding = '0';
    const zone = document.createElement('div');
    zone.id = 'ads-parent';
    const large = document.createElement('div');
    large.id = 'ads-container-large';
    const small = document.createElement('div');
    small.id = 'ads-container-small';
    zone.append(large, small);
    document.body.appendChild(zone);
    const bigCid = addAd({
      layout: '400x600',
      parent: '#ads-container-large',
      cid: 'parity_hi_400x600',
      attrs: { adstyle: 'high-impact-ad;', ...(spec.attrs || {}) },
    });
    if (spec.small !== false)
      addAd({ layout: '400x60', parent: '#ads-container-small', cid: 'parity_hi_400x60' });
    const big = document.querySelector(`owadview[cid="${bigCid}"]`);
    big.addEventListener('high-impact-ad-loaded', () => {
      large.classList.add('high-impact-loaded');
      if (small.parentNode) small.remove();
      report({ kind: 'hi-zone', action: 'expanded', at: since() });
    });
    big.addEventListener('high-impact-ad-removed', () => {
      large.classList.remove('high-impact-loaded');
      if (!small.parentNode) zone.appendChild(small);
      report({ kind: 'hi-zone', action: 'restored', at: since() });
    });
  };

  window.__parityAddAd = addAd;
  window.__parityLayout = sampleLayout;

  // Element specs (scenario config `elementSpec`): [{at?: ms, ...addAd spec} | {at?, zone: 'high-impact'}].
  const specs = params.get('spec') ? JSON.parse(params.get('spec')) : null;
  if (specs) {
    for (const spec of specs) {
      const make = () => (spec.zone === 'high-impact' ? addHighImpactZone(spec) : addAd(spec));
      if (spec.at) setTimeout(make, spec.at);
      else make();
    }
  }

  layouts.forEach((layout, index) =>
    addAd({ layout, attrs: attrsFor(index), cid: `parity_${layout}_${index}` }),
  );

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
    // Element API surface after attach: own names and the prototype chain up to HTMLElement.
    for (const ad of document.querySelectorAll('owadview')) {
      const chain = [];
      for (
        let o = Object.getPrototypeOf(ad);
        o && o !== HTMLElement.prototype;
        o = Object.getPrototypeOf(o)
      ) {
        chain.push({
          constructor: o.constructor && o.constructor.name,
          names: Object.getOwnPropertyNames(o),
        });
      }
      report({
        kind: 'owadview-api',
        cid: ad.getAttribute('cid'),
        own: Object.getOwnPropertyNames(ad),
        chain,
        types: Object.fromEntries(
          [
            'setPageUrl',
            'sendCommand',
            'reload',
            'getWebContentsId',
            'send',
            'refreshAd',
            'setAttribute',
          ].map((n) => [n, typeof ad[n]]),
        ),
        customElement: Boolean(customElements.get('owadview')),
      });
    }
    report({
      kind: 'page-state',
      visibilityState: document.visibilityState,
      hasFocus: document.hasFocus(),
    });
  }, 5000);
})();

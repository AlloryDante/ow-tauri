import { useCallback, useEffect, useState } from 'react';
import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

import { AdSlot } from './AdSlot';

function App() {
  const [testAd, setTestAd] = useState<boolean | null>(null);
  const [cmpRequired, setCmpRequired] = useState(false);
  const [events, setEvents] = useState<string[]>([]);

  useEffect(() => {
    let current = true;
    void getInfo().then((info) => {
      if (current) setTestAd(info.testAd);
    });
    void isCMPRequired().then((required) => {
      if (current) setCmpRequired(required);
    });
    return () => {
      current = false;
    };
  }, []);

  const onAdEvent = useCallback((name: string) => {
    setEvents((list) => [...list.slice(-9), name]);
  }, []);

  return (
    <main className="container">
      <h1>My React Game App</h1>
      <p>{testAd === null ? 'Loading…' : testAd ? 'Test ads' : 'Live ads'}</p>
      <AdSlot cid="main-mrec" width={400} height={300} onAdEvent={onAdEvent} />
      {cmpRequired && (
        <button type="button" onClick={() => void openAdPrivacySettingsWindow()}>
          Ad privacy settings
        </button>
      )}
      <ol className="events" aria-label="Ad events">
        {events.map((name, i) => (
          <li key={i}>{name}</li>
        ))}
      </ol>
    </main>
  );
}

export default App;

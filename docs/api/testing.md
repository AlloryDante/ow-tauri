# `ow-tauri/testing`

Helpers for unit tests of code that uses ow-tauri. Built on
`@tauri-apps/api/mocks`: `mockHost()` installs a fake plugin that answers
the plugin's commands with defaults, records every call, and lets a test
push host messages into the webview's channel. `setHostContext()` makes
the code under test act as the main webview or a UI window. Import it from
tests only.

Generated reference: the `testing` module in `packages/ow-tauri/docs-out`
([how to build it](README.md#build-the-reference)).

```ts
import { mockHost, setHostContext, settle } from 'ow-tauri/testing';

const host = mockHost({ label: 'ow-main' });
await settle();
host.push({ type: 'lifecycle', event: 'before-quit', requestId: 1 });
expect(host.callsOf('app_quit_reply')).toHaveLength(1);
setHostContext('ui');
```

The commands and host messages it fakes are those of
[CONTRACT A.2 and A.3](../CONTRACT.md#a2-commands). The package's own tests
(`packages/ow-tauri/src/**/*.test.ts`) use it throughout.

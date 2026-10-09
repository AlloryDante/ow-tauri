# ADR 0022: Keep hyper for Overwolf's endpoints and reqwest for the updater

- Status: Accepted
- Date: 2026-10-08

## Context

The plugin makes HTTP requests for three jobs:

- analytics (`Counter`, `InsertStats`);
- the consent feature request (`cmp-eu-only`);
- the update client.

For the first two, the request shape is part of the wire contract
(CONTRACT E.1). It includes the header set and its order and the absence of
`accept` and `cookie`. High-level clients add or reorder headers. The update
client needs:

- HTTPS-only redirects with a hop limit;
- connect and idle-read timeouts;
- decompression with a size cap after decompression;
- system proxy support.

## Decision

- Analytics and consent use a plain hyper client
  (`analytics/transport.rs`). It writes exactly the headers ow-electron
  sends, in the same order, on one transport thread.
- The update client uses reqwest (`updater/client.rs`, feature `updater`)
  with:
  - HTTPS-only redirects, at most 10 (`MAX_REDIRECTS`);
  - idle-read timeouts;
  - custom headers sent to the feed host only.
- Both use the same native-tls and hyper crates. There is one TLS stack and
  two client layers.

## Consequences

- Header order stays under the plugin's control, and the harness compares
  it byte for byte.
- Apps without the `updater` feature do not compile reqwest.
- Two client layers must be kept up to date. Dependabot and cargo-deny cover
  both.

## Alternatives considered

- **reqwest for everything.** Its default headers and ordering would change
  the wire. Rejected.
- **hyper for the updater.** It would mean writing the redirect, timeout,
  proxy and decompression policy again, the parts reqwest already gets
  right. Rejected.

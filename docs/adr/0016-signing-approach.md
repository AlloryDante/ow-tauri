# ADR 0016: Sign Tauri builds with Overwolf's published flow, never fake integrity

- Status: Accepted
- Date: 2026-10-06

## Context

Overwolf documents that it signs the gaming-package integrity while the
developer signs the exe, that without both GEP, overlay and recorder do not
load, and that unsigned builds still run; ads and analytics never depend on
signing (https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing).
The published builder (`@overwolf/app-builder-lib` 26.9.3) implements the
flow [BUILDER]:

1. `POST /sign/electron` with the packaged `package.json` and a hash of the
   main entry, which returns a signed `package.json` carrying the
   console-assigned `overwolf.uid`, a `_metadata.json`, and an
   `integrity.dll` URL;
2. `POST /sign/asar` with a hash of Electron's asar integrity data, embedded
   as `OWEASARSIG`;
3. the `OWEINTEGRITY/OWE` PE resource `{"appUid": ...}`;
4. optional Authenticode signing of the app exe with Overwolf's certificate
   (`/sign/electron-certificate`);
5. on Windows, a build that requires signing fails without credentials.

The owner asked for the Tauri equivalent if the documented flow allows it.
The runtime verifier is closed and undocumented.

## Decision

- An `ow-tauri sign` step (Node CLI in the `ow-tauri` package) runs steps 1,
  3 and 4 and the gating of step 5 exactly as the builder does, with the same
  credentials and headers (CONTRACT G.4). It ships `_metadata.json` and
  `integrity.dll`, and the signed `overwolf.uid` becomes the app uid.
- The `OWE` resource is compiled in `build.rs`, before Authenticode signing.
- Step 2 is **not done and not faked**: Tauri has no asar, and a token that
  claims Electron asar integrity for a non-asar app would misrepresent the
  build. An off-by-default `signing.assetIntegrity: "tauri-assets"` option is
  reserved for a distinct resource once Overwolf defines a Tauri target.
- Nothing in ow-tauri claims to satisfy Overwolf's runtime integrity check.

## Consequences

- Signed Tauri builds get the console-assigned uid on every OS and the same
  shipped artefacts and Authenticode signature as ow-electron builds.
- Gaming packages still would not load, because no package runtime exists
  (ADR 0004) and the integrity target for Tauri is undefined (OQ-09); ads and
  analytics are unaffected.
- Whether `/sign/electron` accepts a non-Electron manifest without
  `electronVersion` is open with Overwolf; the step reports the server's
  answer verbatim.

## Alternatives considered

- **Only a build warning (the original draft).** Leaves signed apps without
  their console-assigned uid. Superseded.
- **Compute an asar-like hash list over Tauri assets and send it to
  `/sign/asar`.** The endpoint would sign it, but no runtime verifies it and
  it would misstate what was signed; it may also count as circumventing a
  security mechanism under Overwolf's developer terms. Rejected.

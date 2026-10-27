# Security policy

## Reporting

Email security@openbrick3d.org (until the domain is live, work@neuraldeep.net) with a description, affected version and reproduction. Please do not open a public issue for vulnerabilities. We acknowledge within three working days and aim to fix within 90 days (coordinated disclosure); reporters are credited in the release notes unless they prefer not to be.

## Scope

The client applications (CLI, desktop, PWA, mobile), the Rust crates and the TypeScript packages, the file-format readers (`.obrick`, `.obpart`, `.obpack`, LDraw, 3MF, STL, OBJ, glTF, SVG), the scripted-part sandbox, and `openbrick serve`.

## Design commitments

- The client sends no telemetry; bug reports are opt-in and contain only the renderer backend, GPU string, OS, version and the failing command log.
- Zip readers whitelist entry names, reject absolute paths and `..`, and cap inflation at 200 MB and 10,000 entries. All readers are fuzzed in CI.
- Run documents resolve file references only inside their own folder and the desktop app asks before a shared run reads files.
- SVG is parsed by usvg only; artwork is never injected into the DOM.
- `openbrick serve --qr` binds to the LAN with a random token and serves the PWA read-only.
- Scripted parts run in a capability-free WASM sandbox with fuel and memory limits.
- Signing and updater keys live only in CI secrets with named custodians (see GOVERNANCE.md) and an offline backup.

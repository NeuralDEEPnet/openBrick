# Apps

`web/`: the browser app. It already has the M1 browser pipeline: the Rust core as WebAssembly in a Web Worker and a three.js viewer (see [web/README.md](web/README.md)). It grows into the playground and, in M4, the PWA. `desktop/`: the desktop app alpha (M2), the same playground in a Tauri 2 window with native open and save dialogs (see [desktop/README.md](desktop/README.md)); the same project builds Android (M4) and iOS (M5). See docs/plan/09.

# Spike reports

A spike is a short, time-boxed experiment that answers one open question from the [risk register](plan/13-risks-and-open-questions.md#2-open-questions-with-spikes) before the plan commits to an answer. Each report states the question, the answer, the method and what is still open, and marks which claims were measured and which were only read or reported.

| Report | Question | Answer, in short |
|--------|----------|------------------|
| [Q01: kernel conformance](spikes/q01-kernel-conformance.md) | Is the pure-Rust `manifold-rust` kernel correct, deterministic and fast enough on brick geometry, also in WebAssembly? | Yes on the M0 corpus; phone timings are still open. |
| [Q01b: `manifold-csg` comparison](spikes/q01b-manifold-csg-comparison.md) | Does the C++ Manifold backend agree with `manifold-rust`? | Same volumes and topology, not always the same mesh; `manifold-rust` stays the default. |
| [Q04: LDraw connectivity](spikes/q04-ldraw-connectivity.md) | How many common LDraw parts get usable connectors from the primitives they reference? | Met on the parts that could be checked, not yet proven over all 200. |
| [Q18: slicer multi-material import](spikes/q18-slicer-multimaterial-import.md) | How do PrusaSlicer and Bambu Studio import a multi-part 3MF, and do colours reach the filaments? | One object with one part per colour; colours are not a reliable filament mapping. A weekly OrcaSlicer smoke test now checks the CLI's files. |

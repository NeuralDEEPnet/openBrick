# Documentation site

The source of the OpenBrick3D documentation site ([docs/plan/10 section 7](../plan/10-repo-layout-and-dev-tooling.md#7-documentation-site), backlog item 16). It is published with the playground on GitHub Pages at <https://neuraldeepnet.github.io/openBrick/docs/> by the `pages` workflow, and every pull request builds it and checks its links (the `docs` job of the `ci` workflow).

## Build it

```sh
cargo install mdbook --locked --version 0.5.4   # once; `cargo xtask docs --mdbook-version` prints the pinned version
cargo xtask docs                                # CLI reference, staging, mdBook build, link check
```

Open `target/docs/index.html` in a browser. `cargo xtask docs --check` does the same but fails instead of rewriting a stale CLI reference; that is what CI runs. `cargo xtask check` also checks the CLI reference (without mdBook).

## What is here, and what is not

| Path | What |
|------|------|
| `book.toml` | mdBook configuration |
| `src/SUMMARY.md` | the table of contents |
| `src/index.md` | the landing page; `cargo xtask docs` puts the disclaimer from `crates/openbrick-units/disclaimer.txt` in place of its `<!--openbrick:disclaimer-->` marker, the same text the CLI and the playground show |
| `src/cli.md` | the CLI reference, **generated** from the `openbrick` command's `--help` output for every command (its clap definitions); do not edit it by hand |
| `src/spikes.md`, `src/404.md` | the spike overview and the page-not-found page |
| `install-placeholder.md` | stands in for `docs/guides/install.md` until that guide exists |
| `theme/openbrick.css` | link colours from the playground, the landing page cards |

The guides, the plan and the spike reports are not copied here. `cargo xtask docs` stages `docs/guides/*.md`, `docs/plan/*.md` and `docs/spikes/*.md` next to `src/` in `target/docs-src/`, as `guides/`, `plan/` and `spikes/`, so their links to each other keep working on the site; a link from them to any other file of the repository becomes a link to that file on GitHub, and a link to a file that does not exist fails the build. It then checks every relative link and `#anchor` in the generated HTML.

## Adding a page

- A new guide, plan chapter or spike report: write it in `docs/guides/`, `docs/plan/` or `docs/spikes/` as usual and add a line for it to `src/SUMMARY.md`. The build fails until you do, so nothing is left off the site by accident.
- A page that only makes sense on the site: put it in `src/` and add it to `src/SUMMARY.md`.
- A CLI change: run `cargo xtask docs` (or `cargo xtask docs --check` to see whether you need to) and commit `src/cli.md` with the change.

## Choices

mdBook (MPL-2.0) is a Rust tool, so the site needs no JavaScript toolchain of its own. It runs at build time only; nothing from it is shipped in the CLI, the core or the apps. It is pinned to one version because `book.toml` uses options of the 0.5 series. The site's text is CC BY 4.0, like everything under `docs/` ([LICENSE-POLICY.md](../../LICENSE-POLICY.md)).

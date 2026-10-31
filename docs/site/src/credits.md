# Credits and licences

The text of this site is the OpenBrick3D documentation, licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) (see [Help and contributing](index.md#help-and-contributing)). The site is built with [mdBook](https://github.com/rust-lang/mdBook) 0.5.4, and every built copy of it contains files from mdBook and from the projects mdBook bundles. They are served as mdBook writes them, without changes, each under its own licence, listed here. Built file names carry a content hash, for example `book-609e4cb8.js` for `book.js`.

OpenBrick3D adds only its own files: the pages' text, the style sheet `theme/openbrick.css` and the icon `favicon.svg` (which replaces mdBook's). Those are not files of mdBook or of any project below.

## mdBook

| Files on this site | Licence |
|--------------------|---------|
| The scripts `book.js`, `toc.js` and `searcher.js`; the style sheets `css/chrome.css`, `css/general.css`, `css/print.css` and `css/variables.css`; the syntax colour themes `highlight.css`, `tomorrow-night.css` and `ayu-highlight.css`; `fonts/fonts.css`; and the page templates every page and `toc.html` are built from | Mozilla Public License 2.0 (MPL-2.0) |

These files are covered by the Mozilla Public License, version 2.0. A copy of the MPL is at <https://mozilla.org/MPL/2.0/>. Their source code form is in mdBook's repository at the release this site is built with, [`crates/mdbook-html/front-end` at v0.5.4](https://github.com/rust-lang/mdBook/tree/v0.5.4/crates/mdbook-html/front-end), and its licence is [`LICENSE` at v0.5.4](https://github.com/rust-lang/mdBook/blob/v0.5.4/LICENSE).

The three syntax colour themes are based on themes their headers credit: Atelier Dune by Bram de Haan and Base16 by Chris Kempson (`highlight.css`), Tomorrow Night by Chris Kempson, as collected by jmblog (`tomorrow-night.css`), and Ayu by dempfi (`ayu-highlight.css`).

## Libraries and fonts that mdBook bundles

| Component | Files on this site | Licence | Copyright notice |
|-----------|--------------------|---------|------------------|
| [highlight.js](https://highlightjs.org/) 10.1.1 | `highlight.js` (code colouring) | BSD-3-Clause, [text below](#highlightjs-bsd-3-clause) | Copyright (c) 2006, Ivan Sagalaev |
| [elasticlunr.js](http://weixsong.github.io) 0.9.5 | `elasticlunr.min.js` (search) | MIT, [text below](#mit-licence) | Copyright (C) 2017 Oliver Nightingale; Copyright (C) 2017 Wei Song |
| [mark.js](https://markjs.io/) 8.11.1 | `mark.min.js` (search highlighting) | MIT, [text below](#mit-licence) | Copyright (c) 2014–2018, Julian Kühnel |
| [clipboard.js](https://zenorocha.github.io/clipboard.js) 2.0.4 | `clipboard.min.js` (copy buttons on code) | MIT, [text below](#mit-licence) | © Zeno Rocha |
| Open Sans | `fonts/open-sans-*.woff2` | Apache-2.0, [`fonts/OPEN-SANS-LICENSE.txt`](fonts/OPEN-SANS-LICENSE.txt) | as in the font files |
| Source Code Pro | `fonts/source-code-pro-*.woff2` | SIL Open Font License 1.1, [`fonts/SOURCE-CODE-PRO-LICENSE.txt`](fonts/SOURCE-CODE-PRO-LICENSE.txt) | Copyright 2010, 2012 Adobe Systems Incorporated, with Reserved Font Name 'Source' |
| [Font Awesome Free](https://fontawesome.com/license/free) 6.2.0 | the icons in the menu bar, drawn inline in every page | icons CC BY 4.0 | Copyright 2022 Fonticons, Inc. |

### highlight.js: BSD-3-Clause

```text
BSD 3-Clause License

Copyright (c) 2006, Ivan Sagalaev.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### MIT licence

For elasticlunr.js, mark.js and clipboard.js, with the copyright notices in the table above:

```text
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Updating mdBook

A new mdBook release can bring other files, versions or licences. When `MDBOOK_VERSION` in `crates/xtask/src/docs.rs` changes, compare `target/docs/` with this page and update it in the same pull request.

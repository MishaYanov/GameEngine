# Static documentation site

The site builder lives in `docs-site`. Markdown in `docs` and `designs` remains the content source. The build generates one HTML page per document, a project sidebar, section links, highlighted code with copy buttons, and a full-text search index. The homepage selects between Engine Developer and Game App Developer spaces. `docs/main.md` becomes the Engine Developer home; all existing documentation and search belong to that space. The Game App Developer space is empty.

## Install, build, and preview

From the repository root:

```powershell
npm.cmd ci --prefix docs-site
npm.cmd run build --prefix docs-site
npm.cmd run preview --prefix docs-site
```

Open `http://127.0.0.1:4173/`. Stop the preview with Ctrl+C. Set `$env:PORT` to another port before starting if 4173 is occupied. Use an HTTP server rather than opening HTML files directly, because search loads its index using fetch.

## Update documentation

Edit the Markdown sources, then rebuild. The builder discovers all `.md` files under `docs` and `designs` and groups navigation by source directory. Add introductions to `docs/main.md` for new guides. Relative Markdown links are rewritten to generated HTML paths. Unmapped local links and missing generated targets fail the build rather than silently producing broken navigation.

The build writes only `docs-site/dist`. It clears that generated directory before rebuilding; do not put hand-edited content there. Dependencies and build output are ignored by Git. The Markdown files, builder, styles, scripts, and package lockfile are the maintained sources.

## Flow diagrams

Fenced `mermaid` blocks render as SVG flowcharts and sequence diagrams. The pinned `@mermaid-js/tiny` renderer is copied
into the generated site's local assets; rendering needs JavaScript but no CDN or network service. The dependency
requires Node.js 22.12 or newer when installing/building the site. The tiny distribution supports the diagrams used
here; it excludes Mermaid's mindmap, architecture, KaTeX and ELK features.

Each graph keeps its original Markdown diagram source in an expandable section with a copy button. If JavaScript is
disabled or rendering fails, that source stays open. Use **Readable size** for full-size labels and horizontal
scrolling,
or **Fit width** to return to the page width. Diagrams have accessible captions and use Mermaid's strict security mode.

## Check behavior

```powershell
npm.cmd test --prefix docs-site
```

DOM checks cover search from a nested page, mobile-menu state, code copying, and section-anchor targets. The build checks page/asset links. Browser visual verification remains separate; check wide and narrow layouts, keyboard focus, search dialog interaction, and long code/table overflow.

The checks also parse every Mermaid block with the bundled renderer and verify diagram publication, local assets,
readable-size controls and source fallback. Real-browser rendering remains a separate check because DOM-only tests
do not implement SVG text measurement.

## Hosting

Upload the contents of `docs-site/dist` to a static host, or configure hosting to run the install/build commands above and publish that directory. All navigation and assets use relative links, allowing deployment under a subdirectory. No backend, secrets, or external runtime CDN is required.

This site has been prepared locally; a hosting destination has not been selected. See the [documentation home](main.md).

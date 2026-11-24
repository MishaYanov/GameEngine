# Static documentation site

The site publishes the engine developer guides in [the guide index](main.md). Its generated articles include
section links, navigable examples, local diagrams and full-text search.

## Install, build, and preview

From the repository root:

```powershell
npm.cmd ci --prefix docs-site
npm.cmd run build --prefix docs-site
npm.cmd run preview --prefix docs-site
```

Open `http://127.0.0.1:4173/`. Stop the preview with Ctrl+C. Set `$env:PORT` to another port before starting if 4173 is occupied. Use an HTTP server rather than opening HTML files directly, because search loads its index using fetch.

## Update documentation

Edit a guide in `docs`. To add a guide, register its source and navigation group in `guideGroups` in
`docs-site/build.mjs` and link it from a related guide. Use relative Markdown links between pages.
The build checks page targets, and the tests verify section anchors and search.

The build regenerates `docs-site/dist`. Keep authored Markdown, styles and scripts in their source directories.

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

DOM checks cover search from a nested page, mobile-menu state, code copying, and local section-anchor targets within and across generated pages. The build checks page/asset links. Browser visual verification remains separate; check wide and narrow layouts, keyboard focus, search dialog interaction, and long code/table overflow.

The checks also parse every Mermaid block with the bundled renderer and verify diagram publication, local assets,
readable-size controls and source fallback. Real-browser rendering remains a separate check because DOM-only tests
do not implement SVG text measurement.

## Hosting

Upload the contents of `docs-site/dist` to a static host, or configure hosting to run the install/build commands above and publish that directory. All navigation and assets use relative links, allowing deployment under a subdirectory. No backend, secrets, or external runtime CDN is required.

This site has been prepared locally; a hosting destination has not been selected. See the [documentation home](main.md).

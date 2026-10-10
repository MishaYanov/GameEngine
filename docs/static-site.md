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

## Check behavior

```powershell
npm.cmd test --prefix docs-site
```

DOM checks cover search from a nested page, mobile-menu state, code copying, and section-anchor targets. The build checks page/asset links. Browser visual verification remains separate; check wide and narrow layouts, keyboard focus, search dialog interaction, and long code/table overflow.

## Hosting

Upload the contents of `docs-site/dist` to a static host, or configure hosting to run the install/build commands above and publish that directory. All navigation and assets use relative links, allowing deployment under a subdirectory. No backend, secrets, or external runtime CDN is required.

This site has been prepared locally; a hosting destination has not been selected. See the [documentation home](main.md).

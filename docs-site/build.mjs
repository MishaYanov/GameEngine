import { readFile, readdir, mkdir, writeFile, rm, copyFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { marked } from 'marked';
import hljs from 'highlight.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.dirname(here);
const out = path.join(here, 'dist');
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const slash = value => value.split(path.sep).join('/');
async function collect(dir) {
  const entries = await readdir(path.join(root, dir), { withFileTypes: true });
  const groups = await Promise.all(entries.map(e => e.isDirectory() ? collect(path.join(dir, e.name)) : e.name.endsWith('.md') ? [slash(path.join(dir, e.name))] : []));
  return groups.flat();
}
const files = [...await collect('docs'), ...await collect('designs')].sort();
const routes = new Map(files.map(file => [file, file === 'docs/main.md' ? 'developer/index.html' : file.replace(/\.md$/, '.html')]));
const documents = await Promise.all(files.map(async file => {
  const markdown = await readFile(path.join(root, file), 'utf8');
  const title = markdown.match(/^# (.+)$/m)?.[1] ?? path.basename(file, '.md');
  const description = markdown.split(/\r?\n/).find(line => line.trim() && !line.startsWith('#') && !line.startsWith('Source:')) ?? '';
  return { file, title, markdown, description, route: routes.get(file) };
}));
const relative = (from, to) => path.posix.relative(path.posix.dirname(from), to) || path.posix.basename(to);
const group = file => file.startsWith('designs/') ? 'Design references' : file.startsWith('docs/apps/') ? 'Applications' : file.startsWith('docs/crates/') ? 'Shared crates' : file.startsWith('docs/compiler/') ? 'Tools' : 'Start here';
const groups = ['Start here', 'Applications', 'Tools', 'Shared crates', 'Design references'];
await rm(out, { recursive: true, force: true });
await mkdir(path.join(out, 'assets'), { recursive: true });
await copyFile(path.join(here, 'styles.css'), path.join(out, 'assets/styles.css'));
await copyFile(path.join(here, 'app.js'), path.join(out, 'assets/app.js'));
await copyFile(path.join(here, 'node_modules/highlight.js/styles/github-dark.css'), path.join(out, 'assets/highlight.css'));
await writeFile(path.join(out, 'assets/search-index.json'), JSON.stringify(documents.map(d => ({title:d.title, description:d.description, route:d.route, text:d.markdown.replace(/[`#*]/g, '')}))));

for (const doc of documents) {
  const toc = [];
  const slugs = new Map();
  const renderer = new marked.Renderer();
  renderer.heading = function(token) {
    const text = this.parser.parseInline(token.tokens);
    const base = token.text.toLowerCase().replace(/<[^>]*>/g, '').replace(/[^\p{L}\p{N}\s-]/gu, '').trim().replace(/\s+/g, '-') || 'section';
    const count = slugs.get(base) ?? 0;
    slugs.set(base, count + 1);
    const id = count ? `${base}-${count}` : base;
    if (token.depth === 2 || token.depth === 3) toc.push({title:token.text.replace(/[`*]/g, ''), id, depth:token.depth});
    return `<h${token.depth} id="${id}">${text}<a class="anchor" href="#${id}" aria-label="Link to ${escape(token.text)}">#</a></h${token.depth}>`;
  };
  renderer.code = token => {
    const language = (token.lang ?? '').split(/\s/)[0];
    const highlighted = hljs.getLanguage(language) ? hljs.highlight(token.text, {language}).value : escape(token.text);
    return `<div class="code-block"><div class="code-toolbar"><span>${escape(language || 'text')}</span><button class="copy-code" type="button" aria-label="Copy code">Copy</button></div><pre><code class="hljs">${highlighted}</code></pre></div>`;
  };
  const html = marked.parse(doc.markdown, { renderer, walkTokens(token) {
    if (token.type !== 'link' && token.type !== 'image') return;
    if (/^(?:[a-z][\w+.-]*:|#|\/)/i.test(token.href)) return;
    const [href, fragment] = token.href.split('#');
    const target = path.posix.normalize(path.posix.join(path.posix.dirname(doc.file), decodeURIComponent(href)));
    if (routes.has(target)) token.href = relative(doc.route, routes.get(target)) + (fragment ? '#' + fragment : '');
    else throw new Error(`Unmapped local link in ${doc.file}: ${token.href}`);
  }});
  const nav = groups.map(label => `<div class="nav-group"><p>${label}</p>${documents.filter(d => group(d.file) === label).sort((a,b) => a.file === 'docs/main.md' ? -1 : b.file === 'docs/main.md' ? 1 : a.title.localeCompare(b.title)).map(d => `<a href="${relative(doc.route,d.route)}" ${d.file === doc.file ? 'aria-current="page"' : ''}>${escape(d.file === 'docs/main.md' ? 'Documentation home' : d.title)}</a>`).join('')}</div>`).join('');
  const home = doc.file === 'docs/main.md';
  const cards = home ? `<div class="quick-links"><a href="${relative(doc.route, routes.get('docs/usage.md'))}"><span>01 / GET STARTED</span><strong>Run the engine</strong><p>Setup, launch, and common workflows.</p><b>Read the guide →</b></a><a href="${relative(doc.route, routes.get('docs/architecture.md'))}"><span>02 / UNDERSTAND</span><strong>Explore the architecture</strong><p>Follow the path from editor to GPU.</p><b>See how it fits →</b></a><a href="${relative(doc.route, routes.get('docs/graphics-and-assets.md'))}"><span>03 / BUILD</span><strong>Work with graphics</strong><p>Assets, shaders, and resource ownership.</p><b>Start developing →</b></a></div>` : '';
  const page = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="description" content="${escape(doc.description.replace(/[`*]/g,''))}"><title>${escape(doc.title)} · Game Engine</title><link rel="stylesheet" href="${relative(doc.route,'assets/styles.css')}"><link rel="stylesheet" href="${relative(doc.route,'assets/highlight.css')}"></head><body data-base="${relative(doc.route,'index.html').replace(/index\.html$/,'')}"><a class="skip" href="#content">Skip to content</a><aside class="sidebar" id="navigation"><a class="brand" href="${relative(doc.route,'index.html')}"><span class="brand-mark">△</span><span>GAME ENGINE<small>DEVELOPER DOCUMENTATION</small></span></a><nav class="space-switch" aria-label="Spaces"><a href="${relative(doc.route,'developer/index.html')}" aria-current="true">Engine Developer</a><a href="${relative(doc.route,'game-app-developer/index.html')}">Game App Developer</a><a class="all-spaces" href="${relative(doc.route,'index.html')}">All spaces ↗</a></nav><nav aria-label="Documentation">${nav}</nav><div class="sidebar-footer">Built from repository Markdown<br>Native graphics. Shared foundations.</div></aside><div class="page"><header class="topbar"><button id="menu-toggle" aria-controls="navigation" aria-expanded="false" aria-label="Toggle navigation">☰</button><div class="breadcrumb">Engine Developer <span>/</span> ${escape(group(doc.file))}</div><button id="search-open"><span>⌕</span> Search documentation <kbd>Ctrl K</kbd></button></header><div class="reading-layout"><main id="content" tabindex="-1"><div class="eyebrow">${home ? 'THE ENGINE WORKSPACE' : escape(group(doc.file).toUpperCase())}</div>${home ? '<div class="intro-label">From first launch to the rendering loop.</div>' : ''}${cards}<article>${html}</article><footer class="doc-footer"><span>Source: ${escape(doc.file)}</span><a href="${relative(doc.route,'developer/index.html')}">Developer home ↑</a></footer></main><aside class="toc" aria-label="On this page"><p>ON THIS PAGE</p>${toc.map(t=>`<a class="level-${t.depth}" href="#${t.id}">${escape(t.title)}</a>`).join('')}</aside></div></div><dialog id="search-dialog"><div class="search-header"><label class="sr-only" for="search-input">Search documentation</label><input id="search-input" type="search" placeholder="Search projects, commands, APIs…" autocomplete="off"><button id="search-close" aria-label="Close search">Esc</button></div><div id="search-results" aria-live="polite"></div><div class="search-hint">Search across every guide and design reference.</div></dialog><script src="${relative(doc.route,'assets/app.js')}" defer></script></body></html>`;
  await mkdir(path.dirname(path.join(out,doc.route)), {recursive:true});
  await writeFile(path.join(out,doc.route),page);
}
// Space pages deliberately contain no documentation from the other audience.
function spacePage(route, empty = false) {
  const home = relative(route, 'index.html');
  const developer = relative(route, 'developer/index.html');
  const game = relative(route, 'game-app-developer/index.html');
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${empty ? 'Game App Developer' : 'Documentation spaces'} · Game Engine</title><link rel="stylesheet" href="${relative(route,'assets/styles.css')}"></head><body class="spaces-page"><a class="skip" href="#content">Skip to content</a><header class="spaces-header"><a class="brand" href="${home}"><span class="brand-mark">△</span><span>GAME ENGINE<small>DOCUMENTATION SPACES</small></span></a><nav aria-label="Spaces"><a href="${developer}">Engine Developer</a><a href="${game}" ${empty ? 'aria-current="page"' : ''}>Game App Developer</a></nav></header><main id="content" class="spaces-content" tabindex="-1"><div class="eyebrow">${empty ? 'GAME APP DEVELOPER SPACE' : 'TWO SPACES. ONE ENGINE.'}</div><h1>${empty ? 'Game App Developer' : 'Choose your space.'}</h1>${empty ? '<div class="empty-space"><span class="empty-symbol" aria-hidden="true">□</span><h2>This space is empty.</h2><p>No documentation has been added yet.</p></div>' : `<p class="spaces-intro">Documentation for the people building the engine and the people building games with it.</p><div class="space-cards"><a href="${developer}"><span>01 / ENGINE DEVELOPER</span><h2>Build the engine.</h2><p>Architecture, shared crates, graphics, applications, and development workflows.</p><b>Enter developer space →</b></a><a href="${game}"><span>02 / GAME APP DEVELOPER</span><h2>Build with the engine.</h2><p>A dedicated space for game application developers.</p><span class="empty-badge">Empty space</span><b>Enter game app space →</b></a></div>`}<footer class="spaces-footer"><a href="${home}">All spaces</a><span>Game Engine documentation</span></footer></main></body></html>`;
}
await mkdir(path.join(out, 'game-app-developer'), {recursive:true});
await writeFile(path.join(out, 'index.html'), spacePage('index.html'));
await writeFile(path.join(out, 'game-app-developer/index.html'), spacePage('game-app-developer/index.html', true));

// Verify every generated local document and asset link exists.
for (const doc of [...documents, {route:'index.html'}, {route:'game-app-developer/index.html'}]) {
  const html = await readFile(path.join(out,doc.route),'utf8');
  for (const match of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    const href = match[1];
    if (/^(?:[a-z][\w+.-]*:|#)/i.test(href)) continue;
    await readFile(path.resolve(out,path.posix.dirname(doc.route),href.split('#')[0]));
  }
}
console.log(`Built and link-checked ${documents.length + 2} pages across two spaces in ${out}`);

import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {JSDOM} from 'jsdom';
import vm from 'node:vm';

const html = await readFile(new URL('./dist/docs/crates/renderer/README.html',import.meta.url),'utf8');
const script = await readFile(new URL('./app.js',import.meta.url),'utf8');
const search = JSON.parse(await readFile(new URL('./dist/assets/search-index.json',import.meta.url),'utf8'));
function page() {
  const dom = new JSDOM(html,{url:'http://localhost/docs/crates/renderer/README.html',runScripts:'outside-only'});
  const w=dom.window;
  w.HTMLDialogElement.prototype.showModal=function(){this.open=true;};
  w.HTMLDialogElement.prototype.close=function(){this.open=false;};
  w.fetch=async url=>{assert.equal(new URL(url,w.location).pathname,'/assets/search-index.json');return {ok:true,json:async()=>search};};
  w.eval(script);
  return dom;
}
test('search finds API documentation and keeps nested-page result links valid',async()=>{
  const dom=page();const d=dom.window.document;
  d.querySelector('#search-open').click();
  const input=d.querySelector('#search-input');input.value='EngineStatusDto';input.dispatchEvent(new dom.window.Event('input'));
  await new Promise(resolve=>setTimeout(resolve,30));
  const results=[...d.querySelectorAll('#search-results a')];
  assert.ok(results.some(a=>a.textContent.includes('Shared engine contracts')));
  assert.ok(results.every(a=>new URL(a.href).pathname.startsWith('/docs/')));
  assert.equal(d.querySelector('#search-dialog').open,true);
  d.querySelector('#search-close').click();assert.equal(d.querySelector('#search-dialog').open,false);dom.window.close();
});
test('mobile menu reflects state and Escape closes it',()=>{
  const dom=page();const d=dom.window.document;const toggle=d.querySelector('#menu-toggle');toggle.click();
  assert.equal(toggle.getAttribute('aria-expanded'),'true');assert.ok(d.querySelector('#navigation').classList.contains('open'));
  d.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Escape'}));assert.equal(toggle.getAttribute('aria-expanded'),'false');dom.window.close();
});
test('copy sends original code text to clipboard',async()=>{
  const dom=page();const d=dom.window.document;let copied;
  Object.defineProperty(dom.window.navigator,'clipboard',{value:{writeText:async text=>{copied=text;}}});
  const button=d.querySelector('.copy-code');button.click();await new Promise(resolve=>setTimeout(resolve,10));
  assert.equal(copied,button.closest('.code-block').querySelector('code').textContent);assert.equal(button.textContent,'Copied');dom.window.close();
});
test('every section anchor on every generated page resolves',async()=>{
  for(const doc of search){
    const markup=await readFile(new URL('./dist/'+doc.route,import.meta.url),'utf8');const dom=new JSDOM(markup);
    for(const anchor of dom.window.document.querySelectorAll('a[href^="#"]'))assert.ok(dom.window.document.getElementById(anchor.getAttribute('href').slice(1)),doc.route+': '+anchor.href);
    dom.window.close();
  }
});

test('spaces separate existing developer documentation from the empty game app space',async()=>{
  const landing = new JSDOM(await readFile(new URL('./dist/index.html',import.meta.url),'utf8'));
  const links=[...landing.window.document.querySelectorAll('.space-cards>a')].map(a=>a.getAttribute('href'));
  assert.deepEqual(links,['developer/index.html','game-app-developer/index.html']);
  const developer = new JSDOM(await readFile(new URL('./dist/developer/index.html',import.meta.url),'utf8'));
  assert.ok(developer.window.document.querySelector('article').textContent.includes('Workspace architecture'));
  for(const a of developer.window.document.querySelectorAll('.quick-links a'))assert.ok(a.getAttribute('href').startsWith('../docs/'));
  const game = new JSDOM(await readFile(new URL('./dist/game-app-developer/index.html',import.meta.url),'utf8'));
  assert.ok(game.window.document.querySelector('.empty-space'));
  assert.equal(game.window.document.querySelector('article'),null);
  assert.equal(game.window.document.querySelector('#search-open'),null);
  landing.window.close();developer.window.close();game.window.close();
});

test('Mermaid fences publish local diagrams with complete source fallback and valid syntax', async () => {
    const parser = new JSDOM('', {runScripts: 'outside-only'});
    parser.window.structuredClone = globalThis.structuredClone;
    vm.runInContext(await readFile(new URL('./dist/assets/mermaid.tiny.js', import.meta.url), 'utf8'), parser.getInternalVMContext());
    parser.window.mermaid.initialize({startOnLoad: false, securityLevel: 'strict'});
    let diagramCount = 0;
    try {
        for (const doc of search) {
            const sourcePath = doc.route === 'developer/index.html' ? 'docs/main.md' : doc.route.replace(/\.html$/, '.md');
            const markdown = await readFile(new URL('../' + sourcePath, import.meta.url), 'utf8');
            const blocks = [...markdown.matchAll(/^```mermaid[^\n]*\r?\n([\s\S]*?)^```/gm)];
            const dom = new JSDOM(await readFile(new URL('./dist/' + doc.route, import.meta.url), 'utf8'));
            try {
                const d = dom.window.document;
                const figures = [...d.querySelectorAll('[data-diagram]')];
                assert.equal(figures.length, blocks.length, doc.route);
                assert.equal(d.querySelectorAll('script[src$="mermaid.tiny.js"]').length, blocks.length ? 1 : 0);
                assert.equal(d.querySelectorAll('script[src$="diagrams.js"]').length, blocks.length ? 1 : 0);
                for (const [index, figure] of figures.entries()) {
                    const source = figure.querySelector('.diagram-source code').textContent;
                    assert.equal(source, blocks[index][1].replace(/\r\n/g, '\n').trimEnd(), `${doc.route} diagram ${index + 1}`);
                    assert.ok(figure.querySelector('.diagram-source').open);
                    assert.ok(figure.querySelector('.copy-code'));
                    assert.ok(figure.querySelector('.diagram-size').hidden);
                    assert.ok(d.getElementById(figure.getAttribute('aria-labelledby')));
                    assert.ok(await parser.window.mermaid.parse(source), `${doc.route} diagram ${index + 1}`);
                    diagramCount++;
                }
                for (const asset of [...d.querySelectorAll('script[src]')]) {
                    const src = asset.getAttribute('src');
                    assert.ok(!/^(?:[a-z]+:|\/\/)/i.test(src), `Remote runtime asset: ${src}`);
                    await readFile(new URL(src, new URL('./dist/' + doc.route, import.meta.url)));
                }
            } finally {
                dom.window.close();
            }
        }
        assert.ok(diagramCount > 0, 'Expected published method-flow diagrams');
    } finally {
        parser.window.close();
    }
});

test('diagram rendering supports readable size and preserves source on rendering failure', async () => {
    const markup = await readFile(new URL('./dist/docs/editor-flow.html', import.meta.url), 'utf8');
    const dom = new JSDOM(markup, {runScripts: 'outside-only'});
    const d = dom.window.document;
    let renders = 0;
    let configuration;
    dom.window.mermaid = {
        initialize: options => {
            configuration = options;
        },
        render: async () => {
            if (++renders === 2) throw new Error('Rejected diagram');
            return {svg: '<svg viewBox="0 0 1000 200"><text>start()</text></svg>'};
        },
    };
    Object.defineProperty(dom.window.SVGElement.prototype, 'viewBox', {get: () => ({baseVal: {width: 1000}})});
    try {
        await dom.window.eval(await readFile(new URL('./diagrams.js', import.meta.url), 'utf8'));
        assert.equal(configuration.securityLevel, 'strict');
        const [rendered, failed] = d.querySelectorAll('[data-diagram]');
        assert.equal(rendered.dataset.rendered, 'true');
        assert.equal(rendered.querySelector('.diagram-source').open, false);
        assert.equal(rendered.querySelector('svg').getAttribute('role'), 'img');
        assert.ok(rendered.querySelector('svg').getAttribute('aria-label'));
        const button = rendered.querySelector('.diagram-size');
        assert.equal(button.hidden, false);
        button.click();
        assert.equal(button.getAttribute('aria-pressed'), 'true');
        assert.equal(rendered.querySelector('svg').style.width, '1000px');
        button.click();
        assert.equal(button.getAttribute('aria-pressed'), 'false');
        assert.equal(rendered.querySelector('svg').style.width, '');
        assert.equal(failed.dataset.rendered, 'false');
        assert.equal(failed.querySelector('.diagram-source').open, true);
        assert.equal(failed.querySelector('.diagram-size').hidden, true);
        assert.match(failed.querySelector('.diagram-status').textContent, /could not render/);
    } finally {
        dom.window.close();
    }
});

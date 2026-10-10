import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
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

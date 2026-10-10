const nav = document.querySelector('#navigation');
const toggle = document.querySelector('#menu-toggle');
toggle.addEventListener('click', () => { const open = nav.classList.toggle('open'); toggle.setAttribute('aria-expanded',String(open)); });
document.addEventListener('click', event => { if (nav.classList.contains('open') && !nav.contains(event.target) && !toggle.contains(event.target)) { nav.classList.remove('open'); toggle.setAttribute('aria-expanded','false'); }});
const dialog = document.querySelector('#search-dialog');
const input = document.querySelector('#search-input');
const results = document.querySelector('#search-results');
const base = document.body.dataset.base;
let index;
const loadIndex = async () => index ??= await fetch(base + 'assets/search-index.json').then(r => { if (!r.ok) throw new Error('Search unavailable'); return r.json(); });
const renderResults = async () => {
  try {
    const docs = await loadIndex();
    const terms = input.value.toLowerCase().trim().split(/\s+/).filter(Boolean);
    const hits = docs.map(d => ({...d, score:terms.reduce((s,t) => s + (d.title.toLowerCase().includes(t) ? 8 : 0) + (d.text.toLowerCase().includes(t) ? 1 : 0),0)})).filter(d => !terms.length || terms.every(t => (d.title+' '+d.text).toLowerCase().includes(t))).sort((a,b) => b.score-a.score).slice(0,12);
    results.replaceChildren();
    if (!hits.length) { const empty = document.createElement('p'); empty.className='search-empty'; empty.textContent='No matching guides. Try a project name, command, or API.'; results.append(empty); }
    for (const hit of hits) { const a=document.createElement('a'); a.href=base+hit.route; const title=document.createElement('strong'); title.textContent=hit.title; const p=document.createElement('p'); const location=terms.length ? hit.text.toLowerCase().indexOf(terms[0]) : -1; p.textContent=location>=0 ? '…'+hit.text.slice(Math.max(0,location-50),location+160).replace(/\s+/g,' ')+'…' : hit.description.replace(/[`*]/g,'').slice(0,180); a.append(title,p); results.append(a); }
  } catch { results.textContent='Search could not load. Use the project navigation to browse.'; }
};
const openSearch=()=>{ if(!dialog.open)dialog.showModal(); input.focus(); renderResults(); };
document.querySelector('#search-open').addEventListener('click',openSearch);
document.querySelector('#search-close').addEventListener('click',()=>dialog.close());
input.addEventListener('input',renderResults);
dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();openSearch();}if(e.key==='Escape'){nav.classList.remove('open');toggle.setAttribute('aria-expanded','false');}});
input.addEventListener('keydown',e=>{if(e.key==='ArrowDown'){e.preventDefault();results.querySelector('a')?.focus();}});
for(const button of document.querySelectorAll('.copy-code'))button.addEventListener('click',async()=>{const text=button.closest('.code-block').querySelector('code').textContent;try{await navigator.clipboard.writeText(text);button.textContent='Copied';setTimeout(()=>button.textContent='Copy',1600);}catch{button.textContent='Select code';const range=document.createRange();range.selectNodeContents(button.closest('.code-block').querySelector('code'));const selection=window.getSelection();selection.removeAllRanges();selection.addRange(range);}});

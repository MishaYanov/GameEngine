// Progressive enhancement: the source remains readable if the library or a graph fails.
async function renderDocumentationDiagrams() {
    const figures = document.querySelectorAll('[data-diagram]');
    if (!figures.length || !globalThis.mermaid) return;
    mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'base',
        fontFamily: 'Segoe UI, system-ui, sans-serif',
        themeVariables: {
            primaryColor: '#edf5f0',
            primaryTextColor: '#23313a',
            primaryBorderColor: '#7b9e92',
            lineColor: '#526b65',
            secondaryColor: '#eef3f7',
            tertiaryColor: '#fafbf9',
            fontSize: '15px',
        },
    });
    await document.fonts?.ready;
    for (const [index, figure] of Array.from(figures).entries()) {
        const canvas = figure.querySelector('.diagram-canvas');
        const source = figure.querySelector('.diagram-source');
        const button = figure.querySelector('.diagram-size');
        try {
            canvas.replaceChildren();
            const {svg} = await mermaid.render(`documentation-diagram-${index + 1}`, source.querySelector('code').textContent, canvas);
            canvas.innerHTML = svg;
            const graphic = canvas.querySelector('svg');
            graphic.setAttribute('role', 'img');
            graphic.setAttribute('aria-label', figure.querySelector('figcaption').textContent);
            graphic.removeAttribute('aria-labelledby');
            source.open = false;
            figure.dataset.rendered = 'true';
            button.hidden = false;
            button.addEventListener('click', () => {
                const expanded = button.getAttribute('aria-pressed') !== 'true';
                button.setAttribute('aria-pressed', String(expanded));
                button.textContent = expanded ? 'Fit width' : 'Readable size';
                canvas.classList.toggle('diagram-natural-size', expanded);
                graphic.style.width = expanded ? `${graphic.viewBox.baseVal.width}px` : '';
                graphic.style.maxWidth = expanded ? 'none' : '';
            });
        } catch {
            figure.dataset.rendered = 'false';
            canvas.replaceChildren();
            const message = document.createElement('p');
            message.className = 'diagram-status';
            message.textContent = 'This diagram could not render. Its complete source is available below.';
            canvas.append(message);
            source.open = true;
        }
    }
}

renderDocumentationDiagrams();

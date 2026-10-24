import {Component, DestroyRef, DOCUMENT, effect, inject, signal} from '@angular/core';
import {EditorPane} from '../../../src/app/shared/pane/pane';
import {DemoWorkspace} from '../../../src/app/demo/demo-workspace';
import {environment} from '../../../src/environments/environment';

@Component({
    selector: 'editor-design-workbench',
    imports: [EditorPane, DemoWorkspace],
    templateUrl: './workbench.html',
    styleUrl: './workbench.scss',
})
export class DesignWorkbench {
    protected readonly environment = environment.name;
    protected readonly view = signal<'pane' | 'controls' | 'workspace'>('pane');
    protected readonly theme = signal<'dark' | 'light'>('dark');
    protected readonly compact = signal(false);
    protected readonly accent = signal('#5e6ad2');
    protected readonly selected = signal('Player');
    private readonly document = inject(DOCUMENT);

    constructor() {
        const root = this.document.documentElement;
        const previousTheme = root.getAttribute('data-theme');
        const previousAccent = root.style.getPropertyValue('--palette-brand-secondary');
        const previousPriority = root.style.getPropertyPriority('--palette-brand-secondary');
        effect(() => {
            root.setAttribute('data-theme', this.theme());
            root.style.setProperty('--palette-brand-secondary', this.accent());
        });
        inject(DestroyRef).onDestroy(() => {
            if (previousTheme === null) root.removeAttribute('data-theme');
            else root.setAttribute('data-theme', previousTheme);
            if (previousAccent)
                root.style.setProperty('--palette-brand-secondary', previousAccent, previousPriority);
            else root.style.removeProperty('--palette-brand-secondary');
        });
    }

    protected changeAccent(event: Event): void {
        this.accent.set((event.target as HTMLInputElement).value);
    }
}

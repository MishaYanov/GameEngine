import {Component, computed, input, linkedSignal, output, signal} from '@angular/core';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorTree, type EditorTreeNode} from '../../../../shared/components/tree/tree';
import type {SceneReference} from './scene-references';

@Component({
    selector: 'editor-find-usages',
    imports: [EditorDialog, EditorTree],
    template: `
        <editor-dialog
                [title]="title()"
                icon="graph"
                preferenceKey="editor.find-usages"
                [config]="dialogConfig"
                [open]="open()"
                (openChange)="open.set($event)"
                (closed)="didClose()"
                [renderFields]="false">
            @if (matches().length) {
                <editor-tree label="Usages in the active scene" [nodes]="nodes()"
                             [(selectedId)]="selectedId" (selectedIdChange)="activate($event)"/>
                <p class="selection-hint">{{ disabled() ? 'Finish the current editor action before opening a result.' :
                    'Select a result to reveal its source.' }}</p>
            } @else {
                <p role="status">No references to this entity in the active scene.</p>
            }
            <button type="button" dialog-actions [disabled]="disabled() || !selection()"
                    (click)="activate(selectedId())">Go to Source</button>
        </editor-dialog>
    `,
    styles: `
        p {
            margin-block: 0;
            color: var(--color-text-secondary);
            font-size: var(--font-size-sm);
        }
        .selection-hint { margin-block-start: var(--space-2); }
    `,
})
export class FindUsages {
    readonly target = input<SceneEntity | null>(null);
    readonly references = input<readonly SceneReference[]>([]);
    readonly contextKey = input('');
    readonly disabled = input(false);
    readonly closed = output<void>();
    readonly revealRequested = output<SceneReference>();

    protected readonly dialogConfig = {width: '34rem', height: '25rem', draggable: false};
    private readonly identity = computed(() => {
        const target = this.target();
        return target === null ? null : JSON.stringify([this.contextKey(), target.key]);
    });
    protected readonly open = linkedSignal({source: this.identity, computation: identity => identity !== null});
    protected readonly selectedId = linkedSignal({source: this.identity, computation: (): string | null => null});
    protected readonly title = computed(() => {
        const target = this.target();
        return target ? `Find Usages: ${target.name}` : 'Find Usages';
    });
    protected readonly matches = computed(() => {
        const key = this.target()?.key;
        return key === undefined ? [] : this.references().filter(reference => reference.targetKey === key);
    });
    protected readonly selection = computed(() => this.matches().find(result => result.id === this.selectedId()));
    protected readonly nodes = computed<readonly EditorTreeNode[]>(() => {
        const groups = new Map<SceneReference['group'], EditorTreeNode[]>();
        for (const match of this.matches()) {
            const component = match.location.kind === 'entity' ? match.location.componentId : undefined;
            const name = component ? `${match.sourceName} · ${component}` : match.sourceName;
            const warning = match.issue === 'missing-component' ? ' (missing target component)' : '';
            const node = {id: match.id, label: `${name} · ${match.field}${warning}`};
            const rows = groups.get(match.group);
            if (rows) rows.push(node); else groups.set(match.group, [node]);
        }
        return [...groups].map(([label, children]) => ({id: label, label, children}));
    });
    private readonly pending = signal<{ readonly id: string; readonly identity: string } | null>(null);

    protected activate(id: string | null): void {
        if (this.disabled() || !this.open()) return;
        const identity = this.identity();
        if (id === null || identity === null || !this.matches().some(result => result.id === id)) return;
        this.pending.set({id, identity});
        this.open.set(false);
    }

    protected didClose(): void {
        const pending = this.pending();
        this.pending.set(null);
        const reference = pending !== null && this.identity() === pending.identity
            ? this.matches().find(candidate => candidate.id === pending.id) : undefined;
        this.closed.emit();
        if (reference && !this.disabled()) this.revealRequested.emit(reference);
    }
}

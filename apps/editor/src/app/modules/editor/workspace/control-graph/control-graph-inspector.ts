import {Component, computed, inject, input, linkedSignal, signal} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {SceneEditingService} from '../scene-editing.service';
import {ControlGraphWorkspaceService} from './control-graph-workspace.service';

@Component({
    selector: 'editor-control-graph-inspector',
    template: `
        @if (workspace?.supported() && actor().player_controller) {
            <fieldset class="controls">
                <legend>Controls Graph</legend>
                <label>Control route
                    <select #choice [value]="selected()" (change)="selected.set(choice.value)" [disabled]="!available()">
                        <option value="" [selected]="!selected()">Built-in Controller</option>
                        @for (asset of workspace?.assets() ?? []; track asset.id) {
                            <option [value]="asset.id" [selected]="asset.id === selected()">{{ asset.path }}</option>
                        }
                        @if (selected() && !listed()) {
                            <option [value]="selected()" [selected]="true">Unavailable Controls Graph</option>
                        }
                    </select>
                </label>
                <button type="button" [disabled]="!available() || workspace?.loadingCatalog()" (click)="workspace?.refresh()">Refresh Controls Graphs</button>
                <button type="button" [disabled]="!available() || !selected()" (click)="workspace?.open(selected())">Open Controls Graph</button>
                <button type="button" [disabled]="!available()" (click)="workspace?.create()">New Controls Graph</button>
                <button type="button" (click)="apply()" [disabled]="!available() || selected() === attached() || (!!selected() && !listed())">
                    Apply Control Route
                </button>
                @if (error(); as failure) { <p role="alert">{{ failure }}</p> }
            </fieldset>
        }
    `,
    styles: '.controls { display: grid; gap: .35rem; min-width: 0; margin: .5rem 0; } select { max-width: 100%; }',
})
export class ControlGraphInspector {
    readonly actor = input.required<SceneEntity>();
    readonly editable = input(false);
    protected readonly workspace = inject(ControlGraphWorkspaceService, {optional: true});
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    protected readonly attached = computed(() => this.actor().player_controller?.settings.control_graph ?? '');
    protected readonly selected = linkedSignal({
        source: () => ({key: this.actor().key, attached: this.attached()}),
        computation: source => source.attached,
    });
    protected readonly listed = computed(() => this.workspace?.assets().some(asset => asset.id === this.selected()) === true);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly available = computed(() => this.editable() && this.editing.ready() && !this.pending() && !!this.workspace?.scope());

    protected async apply(): Promise<void> {
        const workspace = this.workspace;
        if (!workspace || !this.available()) return;
        const key = this.actor().key;
        const id = this.selected();
        if (id === this.attached()) return;
        this.pending.set(true);
        this.error.set(null);
        try {
            await this.editing.withCommittedEdit(async () => {
                const scope = workspace.scope();
                const controller = this.editor.document()?.entities.find(entity => entity.key === key)?.player_controller;
                if (!scope || !controller || this.actor().key !== key) return;
                if (id) {
                    const document = await this.editor.openControlGraph(id, scope);
                    if (document.asset.id !== id) throw new Error('The editor opened a different Controls Graph asset.');
                    await this.editor.probeControlsGraph(document.graph, [],
                        {id: document.asset.id, fingerprint: document.asset.fingerprint}, key, scope);
                }
                if (workspace.scope()?.workspaceGeneration !== scope.workspaceGeneration || this.actor().key !== key) return;
                const {control_graph: _previous, ...settings} = controller.settings;
                await this.editor.execute({type: 'replace_component', key, component: {
                    id: controller.id, schema_version: 1, enabled: controller.enabled,
                    data: {kind: 'player_controller', settings: {...settings, ...(id ? {control_graph: id} : {})}},
                }}, scope.workspaceGeneration, scope.revision);
            });
        } catch (error) {
            if (this.actor().key === key) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }
}
import {Component, computed, ElementRef, inject, input, linkedSignal, signal, viewChild} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {SceneEditingService} from '../scene-editing.service';
import {type ControlGraphPaneTarget, ControlGraphWorkspaceService} from './control-graph-workspace.service';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';

@Component({
    selector: 'editor-control-graph-inspector',
    template: `
        @if (workspace && workspace.supported() && actor().player_controller) {
            <fieldset class="controls">
                <legend>Controls Graph</legend>
                <label>Control route
                    <select #choice [value]="selected()" (change)="selected.set(choice.value)" [disabled]="!available()">
                        <option value="" [selected]="!selected()">Built-in Controller</option>
                        @for (asset of workspace.assets(); track asset.id) {
                            <option [value]="asset.id" [selected]="asset.id === selected()">{{ asset.path }}</option>
                        }
                        @if (selected() && !listed()) {
                            <option [value]="selected()" [selected]="true">Unavailable Controls Graph</option>
                        }
                    </select>
                </label>
                <button type="button" [disabled]="!available() || workspace.loadingCatalog()" (click)="workspace.refresh()">Refresh Controls Graphs</button>
                @if (attached() && selected() !== attached()) { <p>Saved control route: {{ attachedLabel() }}</p> }
                <button type="button" [disabled]="!canOpenAttached()" (click)="openAttached()">Open Controls Graph</button>
                <button type="button" [disabled]="!available()" (click)="workspace.create()">New Controls Graph</button>
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
    readonly subject = input<InspectorSubject | null>(null);
    private readonly routePicker = viewChild<ElementRef<HTMLSelectElement>>('choice');
    protected readonly workspace = inject(ControlGraphWorkspaceService, {optional: true});
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    protected readonly attached = computed(() => this.actor().player_controller?.settings.control_graph ?? '');
    protected readonly selected = linkedSignal({
        source: () => ({key: this.actor().key, attached: this.attached()}),
        computation: source => source.attached,
    });
    protected readonly listed = computed(() => this.workspace?.assets().some(asset => asset.id === this.selected()) === true);
    protected readonly attachedLabel = computed(() =>
        this.workspace?.assets().find(asset => asset.id === this.attached())?.path ?? 'Unavailable Controls Graph');
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly available = computed(() => this.editable() && this.editing.ready() && !this.pending() && !!this.workspace?.scope());

    protected readonly canOpenAttached = computed(() => this.available() && !!this.paneTarget() && !this.workspace?.pending());

    focusAttachment(): void {
        if (this.available()) this.routePicker()?.nativeElement.focus();
    }

    protected async openAttached(): Promise<void> {
        const workspace = this.workspace, target = this.paneTarget();
        if (!workspace || !target || !this.canOpenAttached()) return;
        this.error.set(null);
        if (!await workspace.openScoped(target, () => this.paneTarget()) &&
            this.paneTarget()?.actorKey === target.actorKey && workspace.error()) {
            this.error.set(workspace.error());
        }
    }

    private paneTarget(): ControlGraphPaneTarget | null {
        const subject = this.subject(), snapshot = this.editor.snapshot();
        const key = this.actor().key;
        if (!subject || subject.address.kind === 'prop' || snapshot?.mode !== 'edit' ||
            snapshot.active_scene_id !== subject.address.scene || snapshot.selection[0] !== key ||
            (subject.address.kind === 'scene' && subject.address.object !== key)) return null;
        const controller = snapshot.document.entities.find(entity => entity.key === key)?.player_controller;
        const id = controller?.settings.control_graph;
        if (!controller || !id) return null;
        const asset = this.workspace?.assets().find(candidate => candidate.id === id) ?? {id};
        return {actorKey: key, subject, componentId: controller.id, asset};
    }

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
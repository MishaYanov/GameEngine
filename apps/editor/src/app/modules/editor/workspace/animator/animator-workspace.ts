import {afterNextRender, Component, computed, ElementRef, inject, Injector, signal} from '@angular/core';
import {
    type AnimatorClipRef, type AnimatorCondition, type AnimatorGraph, type AnimatorParameterSource,
    type AnimatorState, type AnimatorTransition, sameAnimatorSource,
} from '../../../../core/editor/animator.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {validGameActionId} from '../../../../core/game-input/game-input.models';
import {resolveEditorShortcut} from '../../../../core/editor/editor-keymap';
import {EditorGraph, type GraphCanvasEdit} from '../../../../shared/components/graph/graph';
import {AnimatorAssets} from './animator-assets.service';
import {animatorCanvas, isTrigger} from './animator-draft';
import {AnimatorWorkspaceService} from './animator-workspace.service';

interface ClipChoice {
    readonly label: string;
    readonly clip: AnimatorClipRef;
}

@Component({
    selector: 'editor-animator-workspace',
    imports: [EditorGraph],
    templateUrl: './animator-workspace.html',
    styleUrl: './animator-workspace.scss',
    host: {'(keydown)': 'shortcut($event)'},
})
export class AnimatorWorkspace {
    protected readonly workspace = inject(AnimatorWorkspaceService);
    protected readonly assets = inject(AnimatorAssets);
    private readonly editor = inject(EditorService);
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly injector = inject(Injector);
    protected readonly canvas = computed(() => {
        const graph = this.workspace.graph();
        return graph ? animatorCanvas(graph) : null;
    });
    protected readonly selection = this.workspace.selection;
    protected readonly selectedState = computed(() =>
        this.selection()?.kind === 'node' ? this.workspace.graph()?.states.find(state => state.id === this.selection()?.id) : undefined,
    );
    protected readonly selectedEdge = computed(() =>
        this.selection()?.kind === 'connection' ? this.workspace.graph()?.transitions.find(edge => edge.id === this.selection()?.id) : undefined,
    );
    protected readonly issueByState = computed(() => new Map(this.workspace.issues().map(issue => [issue.target, issue.message])));
    protected readonly parameterKind = signal<AnimatorParameterSource['kind']>('boolean');
    private readonly reviewed = signal<{assetId: string; clips: readonly ClipChoice[]} | null>(null);
    protected readonly reviewingClips = signal(false);
    protected readonly clipChoices = computed(() => {
        const document = this.workspace.document();
        const choices = new Map<string, ClipChoice>();
        const reviewed = this.reviewed();
        for (const choice of reviewed?.assetId === document?.asset.id ? reviewed?.clips ?? [] : []) {
            choices.set(this.clipKey(choice.clip), choice);
        }
        for (const state of this.workspace.graph()?.states ?? []) {
            const key = this.clipKey(state.clip);
            if (!choices.has(key)) choices.set(key, {label: state.clip.kind === 'imported'
                ? `Clip ${state.clip.clip_index + 1}` : state.clip.asset_id, clip: state.clip});
        }
        return [...choices.values()];
    });

    protected addState(graph: AnimatorGraph, name: string): void {
        const id = this.newId('state');
        const clip = this.selectedState()?.clip ?? graph.states[0].clip;
        if (this.workspace.edit({kind: 'add_state', state: {id, name: name.trim() || 'New State', clip, looping: true}})) {
            this.workspace.selection.set({kind: 'node', id});
            this.focusItem(id, 'state');
        }
    }

    protected updateState(state: AnimatorState, update: Partial<Pick<AnimatorState, 'name' | 'clip' | 'looping'>>): void {
        this.workspace.edit({kind: 'update_state', state: {...state, ...update}});
    }

    protected chooseClip(state: AnimatorState, key: string): void {
        const choice = this.clipChoices().find(choice => this.clipKey(choice.clip) === key);
        if (choice) this.updateState(state, {clip: choice.clip});
    }

    protected clipKey(clip: AnimatorClipRef): string {
        return clip.kind === 'imported' ? JSON.stringify([clip.kind, clip.source.asset_id, clip.source.expected_sha256,
            clip.source.scene_index, clip.source.mesh_node, clip.source.skin_index, clip.clip_index])
            : JSON.stringify([clip.kind, clip.asset_id, clip.expected_sha256]);
    }

    protected async reviewClips(state: AnimatorState): Promise<void> {
        const scope = this.assets.scope();
        const document = this.workspace.document();
        if (!scope || !document || this.reviewingClips()) return;
        const current = () => this.assets.current(scope) && this.workspace.document() === document;
        this.reviewingClips.set(true);
        this.workspace.error.set(null);
        try {
            let clips: readonly ClipChoice[];
            if (state.clip.kind === 'imported') {
                const expected = state.clip.source;
                const review = await this.editor.inspectAnimation(expected.asset_id, scope.nativeGeneration, scope.revision);
                if (!sameAnimatorSource(expected, review.metadata.source)) throw new Error('The imported animation source changed. Review its binding before changing clips.');
                clips = review.metadata.clips.map(clip => ({
                    label: clip.name,
                    clip: {kind: 'imported', source: expected, clip_index: clip.index},
                }));
            } else {
                const entries = await this.editor.listAuthoredClips(scope.nativeGeneration);
                clips = entries.map(asset => ({
                    label: asset.path,
                    clip: {kind: 'authored', asset_id: asset.asset_id, expected_sha256: asset.fingerprint},
                }));
            }
            if (current()) this.reviewed.set({assetId: document.asset.id, clips});
        } catch (error) {
            if (current()) this.workspace.showError(error);
        } finally {
            this.reviewingClips.set(false);
        }
    }

    protected connect(from: string, to: string, expectedRevision = this.workspace.revision(), focus = true): void {
        const id = this.newId('transition');
        if (this.workspace.edit({kind: 'connect_states', from, to, id, expectedRevision})) {
            this.workspace.selection.set({kind: 'connection', id});
            if (focus) this.focusItem(id, 'transition');
        }
    }

    protected canvasEdit(edit: GraphCanvasEdit): void {
        if (edit.kind === 'connect') this.connect(edit.from.node, edit.to.node, edit.revision, false);
        else if (edit.kind === 'move_node') this.workspace.edit({kind: 'move_state', id: edit.id, position: edit.position});
        else if (edit.kind === 'remove_node') this.remove('state', edit.id, false);
        else this.remove('transition', edit.id, false);
    }

    protected remove(kind: 'state' | 'transition', id: string, focus = true): void {
        if (!this.workspace.edit({kind: kind === 'state' ? 'remove_state' : 'remove_transition', id})) return;
        if (focus) {
            const selected = this.selection();
            if (selected) this.focusItem(selected.id, selected.kind === 'node' ? 'state' : 'transition');
        }
    }

    protected updateEdge(edge: AnimatorTransition, update: Partial<Omit<AnimatorTransition, 'id'>>): void {
        this.workspace.edit({kind: 'update_transition', transition: {...edge, ...update}});
    }

    protected chooseCondition(graph: AnimatorGraph, edge: AnimatorTransition, id: string): void {
        const parameter = graph.parameters.find(parameter => parameter.id === id);
        let condition: AnimatorCondition = {kind: 'always'};
        if (parameter) condition = isTrigger(parameter) ? {kind: 'trigger', parameter: id} : {kind: 'bool', parameter: id, value: true};
        this.updateEdge(edge, {condition});
    }

    protected chooseParameterKind(kind: string): void {
        if (kind === 'boolean' || kind === 'grounded' || kind === 'moving' || kind === 'jumped' || kind === 'action_accepted') {
            this.parameterKind.set(kind);
        }
    }

    protected addParameter(name: string, sourceValue: string, initial: boolean): void {
        const kind = this.parameterKind();
        let source: AnimatorParameterSource;
        switch (kind) {
            case 'boolean':
                source = {kind, default: initial};
                break;
            case 'grounded':
            case 'jumped':
                source = {kind};
                break;
            case 'moving': {
                const minimum_mm_per_second = Number(sourceValue);
                if (!Number.isInteger(minimum_mm_per_second) || minimum_mm_per_second < 1 || minimum_mm_per_second > 10_000) {
                    this.workspace.showError('Enter a movement threshold from 1 to 10,000 mm/s.');
                    return;
                }
                source = {kind, minimum_mm_per_second};
                break;
            }
            case 'action_accepted':
                if (!validGameActionId(sourceValue)) {
                    this.workspace.showError('Enter the ID of an existing game action.');
                    return;
                }
                source = {kind, action: sourceValue};
                break;
        }
        this.workspace.edit({kind: 'add_parameter', parameter: {id: this.newId('parameter'), name: name.trim(), source}});
    }

    protected parameterSource(kind: string): string {
        switch (kind) {
            case 'boolean': return 'Boolean';
            case 'grounded': return 'Grounded';
            case 'moving': return 'Moving';
            case 'jumped': return 'Jumped';
            default: return 'Action Accepted';
        }
    }

    protected stateName(graph: AnimatorGraph, id: string): string {
        return graph.states.find(state => state.id === id)?.name ?? id;
    }

    protected shortcut(event: KeyboardEvent): void {
        if (event.defaultPrevented || (event.target instanceof Element && event.target.closest('input,select,textarea'))) return;
        switch (resolveEditorShortcut(event)) {
            case 'document.save':
                void this.workspace.save();
                break;
            case 'document.undo':
                this.workspace.undo();
                break;
            case 'document.redo':
                this.workspace.redo();
                break;
            default:
                return;
        }
        event.preventDefault();
        event.stopPropagation();
    }

    private focusItem(id: string, kind: 'state' | 'transition'): void {
        afterNextRender(() => {
            const item = [...this.element.querySelectorAll<HTMLButtonElement>(`[data-${kind}-id]`)]
                .find(button => button.dataset[kind + 'Id'] === id);
            item?.focus();
        }, {injector: this.injector});
    }

    private newId(kind: 'state' | 'parameter' | 'transition'): string {
        return `${kind}_${globalThis.crypto.randomUUID().replaceAll('-', '').slice(0, 20)}`;
    }
}
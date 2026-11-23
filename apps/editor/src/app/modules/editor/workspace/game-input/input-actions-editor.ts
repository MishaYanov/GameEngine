import {Component, computed, effect, inject, input, signal, untracked} from '@angular/core';
import {EditorService, type InputActionsScope} from '../../../../core/editor/editor.service';
import {DEFAULT_GAME_INPUT, MOVEMENT_AXES, type MovementAxis} from '../../../../core/game-input/game-input.models';
import {
    ACTION_BINDINGS, ACTION_VALUES, type ConditionalInputLayout, effectiveLayoutActions, FACE_BUTTONS, INPUT_ACTIONS_SUFFIX,
    type InputActionBinding, type InputActionMap, type InputActionsAsset, type InputActionValue, type InputLayoutCondition,
    MAX_ACTION_BINDINGS, MAX_CONDITIONAL_INPUT_LAYOUTS, MAX_INPUT_ACTIONS, type NamedInputAction, starterInputActions,
    validInputActionsPath, validateInputActions,
} from '../../../../core/game-input/input-actions.models';
import {SceneEditingService} from '../scene-editing.service';
import {InputActionsCatalog} from './input-actions-catalog.service';
import {InputActionsKeyField} from './input-actions-key-field';
import {InputActionsMonitor} from './input-actions-monitor';

const DEFAULT_PATH = 'controls.input-actions.json';
type LayoutPreset = 'land' | 'airborne' | 'world_scene';
const PRESETS: readonly {id: LayoutPreset; label: string}[] = [
    {id: 'land', label: 'Land'},
    {id: 'airborne', label: 'Airborne'},
    {id: 'world_scene', label: 'World Scene'},
];

@Component({
    selector: 'editor-input-actions-editor',
    imports: [InputActionsKeyField, InputActionsMonitor],
    templateUrl: './input-actions-editor.html',
    styleUrl: './input-actions-editor.scss',
})
export class InputActionsEditor {
    readonly active = input(false);
    readonly preferredAsset = input<string | null>(null);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly catalog = inject(InputActionsCatalog);
    protected readonly selectedId = signal('');
    protected readonly source = signal<InputActionsAsset | null>(null);
    protected readonly draft = signal<InputActionMap | null>(null);
    protected readonly path = signal(DEFAULT_PATH);
    protected readonly movePath = signal('');
    protected readonly moving = signal(false);
    protected readonly expandedAction = signal<number | null>(null);
    protected readonly pickedLayout = signal<string | null>(null);
    protected readonly presets = PRESETS;
    protected readonly layoutLimit = MAX_CONDITIONAL_INPUT_LAYOUTS;
    protected readonly layouts = computed(() => this.draft()?.layouts ?? []);
    protected readonly selectedLayout = computed(() => this.layouts().find(layout => layout.id === this.pickedLayout()) ?? null);
    protected readonly layoutPosition = computed(() => this.layouts().findIndex(layout => layout.id === this.pickedLayout()));
    protected readonly editingActions = computed(() => {
        const map = this.draft();
        return map ? effectiveLayoutActions(map, this.selectedLayout()) : [];
    });
    readonly pending = signal(false);
    protected readonly failure = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    private readonly saved = signal<InputActionMap | null>(null);
    protected readonly scenes = computed(() => this.editor.snapshot()?.project.scenes ?? []);
    protected readonly sceneIds = computed(() => new Set(this.scenes().map(scene => scene.id)));
    private generation = '';
    private autoOpened = false;
    private nextLayout = 1;
    protected readonly invalid = computed(() => {
        const map = this.draft();
        return map ? validateInputActions(map, this.sceneIds()) : null;
    });
    readonly dirty = computed(() => this.draft() !== null &&
        (JSON.stringify(this.draft()) !== JSON.stringify(this.saved()) || !this.source() && this.path() !== DEFAULT_PATH));
    protected readonly available = computed(() => this.active() && !this.pending() && !!this.catalog.scope() &&
        this.editor.snapshot()?.mode === 'edit');
    readonly savable = computed(() => this.available() && this.draft() !== null &&
        !this.invalid() && (!this.source() || this.dirty()) && validInputActionsPath(this.path()));
    protected readonly canMove = computed(() => this.available() && !!this.source() && !this.dirty() &&
        this.movePath() !== this.source()?.path && validInputActionsPath(this.movePath()));
    protected readonly types = ACTION_VALUES;
    protected readonly buttons = FACE_BUTTONS;
    protected readonly axes = MOVEMENT_AXES;
    protected readonly maxActions = MAX_INPUT_ACTIONS;
    protected readonly maxBindings = MAX_ACTION_BINDINGS;
    protected readonly extension = INPUT_ACTIONS_SUFFIX;

    constructor() {
        effect(() => {
            const scope = this.catalog.scope();
            const generation = scope ? `${scope.workspaceGeneration}:${scope.nativeGeneration}` : '';
            if (generation === this.generation) return;
            this.generation = generation;
            untracked(() => {
                this.reset();
                this.selectedId.set('');
                this.pending.set(false);
            });
        });
        effect(() => {
            if (!this.active()) {
                this.autoOpened = false;
                return;
            }
            const id = this.preferredAsset();
            if (this.autoOpened || !id || !this.available() ||
                !this.catalog.assets().some(asset => asset.id === id)) return;
            this.autoOpened = true;
            if (this.dirty() || this.source()?.id === id) return;
            untracked(() => {
                this.selectedId.set(id);
                void this.openSelected();
            });
        });
    }

    protected create(): void {
        if (!this.available() || this.dirty()) return;
        this.reset();
        const draft = starterInputActions();
        this.draft.set(draft);
        this.saved.set(draft);
    }

    protected async openSelected(): Promise<void> {
        const id = this.selectedId();
        if (!this.available() || this.dirty() || !this.catalog.assets().some(asset => asset.id === id)) return;
        await this.run(false, async (scope, current) => {
            const loaded = await this.editor.openInputActions(id, scope);
            if (!current()) return;
            if (loaded.asset.id !== id) throw new Error('The editor opened a different Input Actions asset.');
            this.accept(loaded.asset, loaded.map, scope);
        });
    }

    async save(): Promise<void> {
        const draft = this.draft();
        if (!draft || !this.savable()) return;
        const previous = this.source();
        const path = previous?.path ?? this.path();
        await this.run(true, async (scope, current) => {
            const capture = previous ? {id: previous.id, fingerprint: previous.fingerprint} : null;
            const result = await this.editor.saveInputActions(path, draft, capture, scope);
            if (!current()) return;
            if (result.asset.path !== path || previous && result.asset.id !== previous.id) {
                throw new Error('The editor returned a different saved Input Actions asset.');
            }
            this.accept(result.asset, result.map, scope);
            this.message.set('Input Actions saved.');
        });
    }

    protected beginMove(): void {
        const source = this.source();
        if (!source || this.dirty() || !this.available()) return;
        this.movePath.set(source.path);
        this.moving.set(true);
    }

    protected async move(): Promise<void> {
        const source = this.source();
        const map = this.saved();
        if (!source || !map || !this.canMove()) return;
        const path = this.movePath();
        await this.run(true, async (scope, current) => {
            const moved = await this.editor.moveInputActions(path, {id: source.id, fingerprint: source.fingerprint}, scope);
            if (!current()) return;
            if (moved.id !== source.id || moved.fingerprint !== source.fingerprint || moved.path !== path) {
                throw new Error('The editor returned a different moved Input Actions asset.');
            }
            this.accept(moved, map, scope);
            this.message.set('Input Actions moved. Player assignments use the same asset ID.');
        });
    }

    discard(): void {
        if (!this.available()) return;
        this.draft.set(this.saved());
        this.expandedAction.set(null);
        this.pickedLayout.set(null);
        this.path.set(this.source()?.path ?? DEFAULT_PATH);
        this.failure.set(null);
        this.message.set(null);
    }

    protected patchAction(index: number, patch: Partial<NamedInputAction>): void {
        if (!this.available() || this.selectedLayout()) return;
        this.draft.update(draft => {
            const previous = draft?.actions[index];
            if (!draft || !previous) return draft;
            const next = {...previous, ...patch};
            const map = {...draft, actions: draft.actions.map((action, i) => i === index ? next : action)};
            if (!draft.layouts || previous.id === next.id || draft.actions.some((action, i) => i !== index && action.id === previous.id)) {
                return map;
            }
            return {...map, layouts: draft.layouts.map(layout => ({...layout, overrides: layout.overrides.map(item =>
                item.action_id === previous.id ? {...item, action_id: next.id} : item)}))};
        });
    }

    protected changeValue(index: number, value: string): void {
        const choice = this.types.find(type => type.value === value);
        if (choice) this.patchAction(index, {value: choice.value});
    }

    protected addAction(): void {
        const draft = this.draft();
        if (!this.available() || !draft || this.selectedLayout() || draft.actions.length >= MAX_INPUT_ACTIONS) return;
        const used = new Set(draft.actions.map(action => action.id));
        let suffix = draft.actions.length + 1;
        while (used.has('action_' + suffix)) suffix++;
        this.draft.set({...draft, actions: [...draft.actions, {
            id: 'action_' + suffix, label: 'Action ' + suffix, value: 'button', bindings: [],
        }]});
        this.expandedAction.set(draft.actions.length);
    }

    protected removeAction(index: number): void {
        if (!this.available() || this.selectedLayout()) return;
        this.draft.update(draft => {
            const removed = draft?.actions[index];
            if (!draft || !removed) return draft;
            const actions = draft.actions.filter((_, i) => i !== index);
            if (!draft.layouts || actions.some(action => action.id === removed.id)) return {...draft, actions};
            return {...draft, actions, layouts: draft.layouts.map(layout => ({...layout,
                overrides: layout.overrides.filter(item => item.action_id !== removed.id)}))};
        });
        this.expandedAction.set(null);
    }

    protected selectLayout(id: string | null): void {
        if (!this.available()) return;
        this.pickedLayout.set(id);
        this.expandedAction.set(null);
    }

    protected addLayout(preset: string): void {
        const definition = PRESETS.find(item => item.id === preset);
        const draft = this.draft();
        if (!definition || !draft || !this.available() || this.layouts().length >= this.layoutLimit) return;
        const id = this.nextLayoutId();
        const jump = draft.actions.find(action => action.id === 'jump' && action.value === 'button');
        const attack = draft.actions.some(action => action.id === 'attack' && action.value === 'button');
        const overrides = preset === 'airborne' && jump && attack ? [
            {action_id: 'jump', bindings: []},
            {action_id: 'attack', bindings: jump.bindings},
        ] : [];
        this.setLayouts([...this.layouts(), {id, label: definition.label, when: this.presetCondition(definition.id), overrides}]);
        this.selectLayout(id);
    }

    protected duplicateLayout(): void {
        const source = this.selectedLayout();
        if (!source || !this.available() || this.layouts().length >= this.layoutLimit) return;
        const id = this.nextLayoutId();
        const layouts = [...this.layouts()];
        layouts.splice(this.layoutPosition() + 1, 0, {...source, id});
        this.setLayouts(layouts);
        this.selectLayout(id);
    }

    protected removeLayout(): void {
        const id = this.pickedLayout();
        if (!this.available() || !id) return;
        this.setLayouts(this.layouts().filter(layout => layout.id !== id));
        this.selectLayout(null);
    }

    protected moveLayout(offset: -1 | 1): void {
        const index = this.layoutPosition();
        const target = index + offset;
        const layouts = [...this.layouts()];
        if (!this.available() || index < 0 || target < 0 || target >= layouts.length) return;
        [layouts[index], layouts[target]] = [layouts[target], layouts[index]];
        this.setLayouts(layouts);
    }

    protected renameLayout(label: string): void {
        this.updateLayout(layout => ({...layout, label}));
    }

    protected changeCondition(value: string): void {
        const preset = PRESETS.find(item => item.id === value);
        if (preset) this.updateLayout(layout => ({...layout, when: this.presetCondition(preset.id)}));
    }

    protected chooseConditionScene(scene_id: string): void {
        this.updateLayout(layout => ({...layout, when: {kind: 'scene_is', scene_id}}));
    }

    protected conditionPreset(condition: InputLayoutCondition): LayoutPreset {
        return condition.kind === 'scene_is' ? 'world_scene' : condition.value ? 'land' : 'airborne';
    }

    protected bindingOrigin(actionId: string): string {
        const layout = this.selectedLayout();
        if (!layout) return 'Default';
        const override = layout.overrides.find(entry => entry.action_id === actionId);
        return !override ? 'Inherited' : override.bindings.length ? 'Override' : 'Unbound';
    }

    protected inheritBindings(actionId: string): void {
        this.updateLayout(layout => ({...layout, overrides: layout.overrides.filter(item => item.action_id !== actionId)}));
    }

    protected unbindAction(index: number): void {
        this.updateAction(index, action => ({...action, bindings: []}));
    }

    private setLayouts(layouts: readonly ConditionalInputLayout[]): void {
        this.draft.update(draft => draft && {...draft, schema_version: 2, layouts});
    }

    private updateLayout(update: (layout: ConditionalInputLayout) => ConditionalInputLayout): void {
        const id = this.pickedLayout();
        if (id && this.available()) this.setLayouts(this.layouts().map(layout => layout.id === id ? update(layout) : layout));
    }

    private nextLayoutId(): string {
        let id: string;
        do id = 'layout_' + this.nextLayout++;
        while (this.layouts().some(layout => layout.id === id));
        return id;
    }

    private presetCondition(preset: LayoutPreset): InputLayoutCondition {
        if (preset !== 'world_scene') return {kind: 'character_grounded', value: preset === 'land'};
        return {kind: 'scene_is', scene_id: this.editor.snapshot()?.active_scene_id ?? ''};
    }

    protected toggleAction(index: number): void {
        if (this.available()) this.expandedAction.update(current => current === index ? null : index);
    }

    protected bindingKinds(value: InputActionValue, current: InputActionBinding['kind']) {
        return ACTION_BINDINGS.filter(option => option.value === value || option.kind === current);
    }

    protected addBinding(index: number): void {
        this.updateAction(index, action => {
            const option = ACTION_BINDINGS.find(item => item.value === action.value);
            return option && action.bindings.length < MAX_ACTION_BINDINGS
                ? {...action, bindings: [...action.bindings, this.defaultBinding(option.kind)]} : action;
        });
    }

    protected changeBinding(index: number, binding: number, kind: string): void {
        const option = ACTION_BINDINGS.find(item => item.kind === kind && item.value === this.editingActions()[index]?.value);
        if (option) this.updateBinding(index, binding, () => this.defaultBinding(option.kind));
    }

    protected changeKey(index: number, binding: number, key: string, axis: MovementAxis | null = null): void {
        this.updateBinding(index, binding, item => {
            if (axis && 'keys' in item) return {...item, keys: {...item.keys, [axis]: key}};
            return item.kind === 'key' && !axis ? {...item, key} : item;
        });
    }

    protected changeButton(index: number, binding: number, value: string): void {
        const button = FACE_BUTTONS.find(item => item === value);
        if (button) this.updateBinding(index, binding, item => item.kind === 'gamepad_button' ? {...item, button} : item);
    }

    protected removeBinding(index: number, binding: number): void {
        this.updateAction(index, action => ({...action, bindings: action.bindings.filter((_, i) => i !== binding)}));
    }

    private updateAction(index: number, update: (action: NamedInputAction) => NamedInputAction): void {
        if (!this.available()) return;
        if (this.selectedLayout()) {
            const action = this.editingActions()[index];
            if (!action) return;
            const next = update(action);
            if (next === action) return;
            this.updateLayout(layout => ({...layout, overrides: [
                ...layout.overrides.filter(entry => entry.action_id !== action.id), {action_id: action.id, bindings: next.bindings},
            ]}));
        } else this.draft.update(draft => draft && {
            ...draft, actions: draft.actions.map((action, i) => i === index ? update(action) : action),
        });
    }

    private updateBinding(index: number, binding: number, update: (item: InputActionBinding) => InputActionBinding): void {
        this.updateAction(index, action => ({...action, bindings: action.bindings.map((item, i) => i === binding ? update(item) : item)}));
    }

    private defaultBinding(kind: InputActionBinding['kind']): InputActionBinding {
        switch (kind) {
            case 'key': return {kind, key: 'space'};
            case 'gamepad_button': return {kind, button: 'a'};
            case 'keyboard_vector': return {kind, keys: {...DEFAULT_GAME_INPUT.movement}};
            case 'keyboard_look': return {kind, keys: {negative_x: 'arrow_left', positive_x: 'arrow_right',
                negative_y: 'arrow_down', positive_y: 'arrow_up'}};
            default: return {kind};
        }
    }

    private reset(): void {
        this.source.set(null);
        this.saved.set(null);
        this.draft.set(null);
        this.path.set(DEFAULT_PATH);
        this.moving.set(false);
        this.expandedAction.set(null);
        this.pickedLayout.set(null);
        this.failure.set(null);
        this.message.set(null);
    }

    private accept(asset: InputActionsAsset, map: InputActionMap, scope: InputActionsScope): void {
        if (this.source()?.id !== asset.id && this.source() !== null) {
            this.expandedAction.set(null);
            this.pickedLayout.set(null);
        }
        this.catalog.accept(asset, scope);
        this.selectedId.set(asset.id);
        this.source.set(asset);
        this.path.set(asset.path);
        this.saved.set(map);
        this.draft.set(map);
        this.moving.set(false);
    }

    private async run(
        mutating: boolean, action: (scope: InputActionsScope, current: () => boolean) => Promise<void>,
    ): Promise<void> {
        const initial = this.catalog.scope();
        if (!this.available() || !initial) return;
        const current = () => this.catalog.isCurrent(initial);
        this.pending.set(true);
        this.failure.set(null);
        this.message.set(null);
        try {
            const execute = async () => {
                const scope = this.catalog.scope();
                if (scope && current()) await action(scope, current);
            };
            if (mutating && this.editing) {
                if (!await this.editing.withCommittedEdit(execute) && current()) {
                    this.failure.set(this.editing.error() ?? 'Finish the current scene edit before changing Input Actions.');
                }
            } else await execute();
        } catch (error) {
            if (current()) this.failure.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.pending.set(false);
        }
    }
}

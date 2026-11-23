import {
    afterNextRender,
    Component,
    computed,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    output,
    signal
} from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import type {
	ComponentData,
	ComponentRecord,
	ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';
import type { Collider, ModelAsset } from '../../../../core/editor/editor.models';
import { ColliderInspector } from '../collider/collider-inspector';
import {
	changeColliderInteraction,
	defaultCollider,
	type ColliderInteractionChange,
	type ColliderKind,
} from '../collider/collider';
import { componentDraft, componentFromDraft, type ComponentTarget } from './component-draft';
import type {ScriptDescriptor, ScriptField, ScriptReferenceField} from '../../../../core/editor/object-scripts.models';
import {scriptDraft, servesScriptReference} from './script-component-draft';
import { MachineEditor } from './machine-editor';
import { CONTROLLER_FIELDS } from '../../../../core/editor/character-controller.models';
import type { MachineDefinition } from '../../../../core/editor/state-machine.models';
import type { AnimationSettings } from '../../../../core/editor/animation.models';
import { AnimationEditor } from './animation-editor';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {sceneReferenceOptions} from '../scene-reference-options';
import {CameraRigFields} from '../game-settings/camera-rig-fields';
import {skyColorChannels} from './sky-component-draft';
import {SkyPanoramaPicker} from './sky-panorama-picker';
import {validateCameraLens} from './camera-component-draft';
import type {InputActionsAsset} from '../../../../core/game-input/input-actions.models';
import type {InputActionsRef} from '../../../../core/editor/player-controller.models';
import {AudioSourceInspector} from '../audio-source/audio-source-inspector';
import {audioGameplaySignals, legacyCombatCues, type AudioAuditionScope} from '../../../../core/editor/audio-source.models';
import {AudioMarkerFields} from '../audio-source/audio-marker-fields';
import {audioClipDragType, readAudioClipDrag} from '../audio-source/audio-clip-drag';

export type ComponentEditIntent =
	| { readonly kind: 'add'; readonly component: ComponentRecord; readonly expected: string | null }
	| {
			readonly kind: 'replace';
			readonly component: ComponentRecord;
			readonly expected: string | null;
	  }
	| { readonly kind: 'remove'; readonly component_id: string; readonly expected: string }
	| { readonly kind: 'enabled'; readonly component_id: string; readonly enabled: boolean };

@Component({
	selector: 'editor-component-editor',
    imports: [FormField, ColliderInspector, MachineEditor, AnimationEditor, EditorSearchPicker, CameraRigFields, AudioSourceInspector, AudioMarkerFields, SkyPanoramaPicker],
	templateUrl: './component-editor.html',
	styleUrl: './component-editor.scss',
})
export class ComponentEditor {
	readonly components = input<readonly ComponentRecord[]>([]);
	readonly types = input<readonly ComponentTypeDescriptor[]>([]);
	readonly scriptTypes = input<readonly ScriptDescriptor[]>([]);
	readonly scriptSlots = input<readonly string[]>([]);
	readonly entities = input<readonly ComponentTarget[]>([]);
	readonly models = input<readonly ModelAsset[]>([]);
    readonly gameActions = input<readonly string[]>([]);
    readonly interactablePinned = input(false);
    readonly playerBodyIssue = input<string | null>(null);
    readonly inputActionAssets = input<readonly InputActionsAsset[]>([]);
    readonly inputActionAssetsEnabled = input(false);
    readonly skyPanoramaAssetsEnabled = input(false);
	readonly ownerKey = input('');
	readonly contextKey = input('');
	readonly editable = input(false);
	readonly pending = input(false);
	readonly error = input<string | null>(null);
	readonly acknowledged = input(0);
	readonly previewAllowed = input(false);
    readonly audioScope = input<AudioAuditionScope | null>(null);
    readonly assetGeneration = input<number | null>(null);
	readonly actionRequested = output<ComponentEditIntent>();
	readonly draftOpenChanged = output<boolean>();
    private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
    private readonly injector = inject(Injector);
	protected readonly open = signal(false);
	protected readonly selectedKind = signal<ComponentData['kind']>('native_behavior');
	protected readonly draft = signal(componentDraft('native_behavior', '', ''));
	protected readonly fields = form(this.draft);
	protected readonly collider = signal<Collider>(defaultCollider('box')!);
	protected readonly colliderInvalid = signal(false);
	protected readonly animationPending = signal(false);
	protected readonly original = signal<ComponentRecord | undefined>(undefined);
	protected readonly axes = ['x', 'y', 'z'] as const;
	protected readonly colors = ['r', 'g', 'b', 'a'] as const;
	protected readonly controllerFields = CONTROLLER_FIELDS;
	protected readonly previewSourceMatches = computed(() => {
		const original = this.original();
		return (
			this.previewAllowed() &&
			original?.data.kind === 'animation' &&
			JSON.stringify(original.data.settings.source) ===
				JSON.stringify(this.draft().animation?.source)
		);
	});
    protected readonly audioGameplaySignals = audioGameplaySignals;
    protected readonly legacyCombatCues = legacyCombatCues;
    protected chooseAudioEventClip(id: string): void {
        if (this.fields.audioSource.eventClip().value() === id) return;
        this.fields.audioSource.eventClip().value.set(id);
        this.fields.audioSource.eventMarker().value.set('');
    }

    protected readonly audioClips = computed(() => this.models().filter(asset => asset.kind === 'audio_clip' && asset.asset_id));
    protected readonly audioClipOptions = computed(() => this.audioClips().flatMap(clip =>
        clip.asset_id ? [{id: clip.asset_id, label: clip.name, description: clip.path}] : []
    ));
    protected readonly meshModels = computed(() => this.models().filter(asset => !asset.kind || asset.kind === 'model'));
	protected readonly available = computed(() => this.editable() && !this.pending());
	protected readonly addTypes = computed(() =>
		this.components().length >= 16
			? []
			: this.types().filter(
					(type) =>
                        !this.pinned(type.kind) &&
						this.components().filter((component) => component.data.kind === type.kind).length <
						type.max_per_object,
				),
	);
	protected readonly canAdd = computed(() =>
		this.addTypes().some((type) => type.kind === this.selectedKind()),
	);
    protected readonly typeOptions = computed(() => this.addTypes().map(type => ({
        id: type.kind,
        label: type.label,
        description: type.description,
        keywords: type.type_id,
    })));
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.entities()));
	protected readonly cameraTargets = computed(() => this.entities().filter(entity => entity.key !== this.ownerKey()));
	protected readonly cameraOptions = computed(() => sceneReferenceOptions(this.cameraTargets()));
	protected readonly cameraLensFields = [
		{key: 'fovDegrees', label: 'Vertical field of view (degrees)'},
		{key: 'near', label: 'Near clip'},
		{key: 'far', label: 'Far clip'},
	] as const;
	protected readonly cameraLensErrors = computed(() => {
		const draft = this.draft();
		if (draft.kind !== 'camera') return {};
		const original = this.original()?.data;
		return validateCameraLens(draft.camera.lens, original?.kind === 'camera' ? original.settings.lens : undefined).errors;
	});
	protected readonly candidateError = computed(() => this.cameraLensFields.some(field => !!this.cameraLensErrors()[field.key])
		? null : this.candidate().error);
	protected readonly candidate = computed(() => {
		try {
			const draft = this.draft();
			const bodyIssue = this.playerBodyIssue();
			if (draft.kind === 'player_controller' && draft.enabled && bodyIssue) throw new Error(bodyIssue);
			if (this.animationPending()) throw new Error('Wait for animation preparation to finish.');
			if (this.colliderInvalid()) throw new Error('Finish or discard the invalid collider value.');
			return {
				value: componentFromDraft(
					this.draft(),
					this.collider(),
					this.draft().kind === 'camera' ? this.cameraTargets() : this.entities(),
					this.models(),
					this.original(),
					this.scriptTypes(),
					this.scriptSlots(),
				),
				error: null,
			};
		} catch (error) {
			return { value: null, error: error instanceof Error ? error.message : String(error) };
		}
	});
    protected readonly scriptType = computed(() => this.scriptTypes().find(type => type.id === this.draft().script.id));
    protected readonly scriptCanReset = computed(() => {
        const original = this.original()?.data;
        const script = this.scriptType();
        return original?.kind === 'script' && script?.id === original.settings.script_id &&
            script.config_version !== original.settings.config_version && !this.draft().script.resetConfig;
    });
    protected chooseScript(id: string): void {
        if (!this.scriptTypes().some(type => type.id === id)) return;
        this.draft.update(value => value.script.id === id ? value : {...value, script: {...scriptDraft(), id}});
    }
    protected resetScriptConfig(): void {
        if (!this.scriptCanReset()) return;
        this.draft.update(value => ({
            ...value,
            script: {...scriptDraft(), id: value.script.id, resetConfig: true},
        }));
    }
    protected scriptValue(field: ScriptField): string | boolean {
        return this.draft().script.properties[field.key] ?? String(field.default);
    }
    protected scriptChecked(field: ScriptField & {kind: 'boolean'}): boolean {
        return (this.draft().script.properties[field.key] ?? field.default) === true;
    }
    protected setScriptValue(key: string, next: string | boolean): void {
        this.draft.update(value => ({...value, script: {...value.script, properties: {...value.script.properties, [key]: next}}}));
    }
    protected scriptTargets(field: ScriptReferenceField): readonly ComponentTarget[] {
        return this.entities().filter(target => servesScriptReference(target, field.requires));
    }
    protected scriptTarget(key: string): string {
        const reference = this.draft().script.references[key];
        if (!reference) return '';
        return 'entity' in reference ? 'entity:' + reference.entity : 'slot:' + reference.slot;
    }
    protected setScriptTarget(key: string, option: string): void {
        const reference = option.startsWith('slot:') ? {slot: option.slice(5)} : {entity: option.slice(7)};
        this.draft.update(value => ({...value, script: {...value.script, references: {...value.script.references, [key]: reference}}}));
    }
	protected readonly references = computed<
		readonly {
			key: 'actor' | 'key_trigger' | 'door' | 'key_visual' | 'trigger' | 'visual';
			label: string;
			optional?: boolean;
            usageField?: string;
		}[]
	>(() => {
		if (this.draft().behavior_kind === 'key_door')
			return [
				{ key: 'actor', label: 'Actor' },
                {key: 'key_trigger', label: 'Key sensor', usageField: 'Key trigger'},
				{ key: 'door', label: 'Door' },
				{ key: 'key_visual', label: 'Key visual', optional: true },
			];
		return [
			{ key: 'actor', label: 'Actor' },
            {key: 'trigger', label: 'Trigger sensor', usageField: 'Trigger'},
			{ key: 'visual', label: 'Visual', optional: true },
		];
	});
	protected readonly modelListId = `component-models-${crypto.randomUUID()}`;
    protected readonly actionListId = 'component-actions-' + crypto.randomUUID();
	protected readonly prefabListId = `component-prefabs-${crypto.randomUUID()}`;

	constructor() {
		effect(() => {
			this.contextKey();
			this.acknowledged();
			this.open.set(false);
			this.colliderInvalid.set(false);
		});
		effect(() => {
			this.draftOpenChanged.emit(this.open());
		});
		effect(() => {
			const types = this.addTypes();
			if (!types.some((type) => type.kind === this.selectedKind()) && types[0])
				this.selectedKind.set(types[0].kind);
		});
	}

	protected chooseKind(value: string): void {
		const type = this.addTypes().find((type) => type.kind === value);
		if (type) this.selectedKind.set(type.kind);
	}

	protected readonly skyChannels = skyColorChannels;

	protected label(component: ComponentRecord): string {
		if (component.data.kind === 'audio_source') return 'Audio Source';
		if (component.data.kind === 'camera') return 'Camera';
		if (component.data.kind === 'sky') return 'Sky';
		if (component.data.kind === 'player_controller') return 'Player Controller';
        if (component.data.kind === 'script') {
            const id = component.data.settings.script_id;
            return this.scriptTypes().find(script => script.id === id)?.label ?? 'Script: ' + id;
        }
		if (component.data.kind === 'native_behavior')
			return {
				key_door: 'Key and Door',
				collectible: 'Collectible',
				timed_spawner: 'Timed Spawner',
				state_machine: 'State Machine',
			}[component.data.behavior.kind];
		return (
			this.types().find((type) => type.kind === component.data.kind)?.label ??
			(component.data.kind === 'collider' ? 'Collider' : 'Mesh Renderer')
		);
	}

    protected inputAssetId(ref: InputActionsRef): string { return ref.kind === 'asset' ? ref.asset_id : ''; }

    protected inputSource(ref: InputActionsRef): string {
        if (ref.kind === 'project_default') return 'Project Input';
        return this.inputActionAssets().find(asset => asset.id === ref.asset_id)?.path ?? 'Unavailable Input Actions asset';
    }

    protected chooseInputAsset(id: string): void {
        if (!this.available() || !this.open() || !this.inputActionAssetsEnabled() ||
            this.draft().kind !== 'player_controller') return;
        if (id && !this.inputActionAssets().some(asset => asset.id === id)) return;
        const input: InputActionsRef = id ? {kind: 'asset', asset_id: id} : {kind: 'project_default'};
        this.fields.playerController.input().value.set(input);
    }

    protected pinned(kind: ComponentData['kind']): boolean {
        return (kind === 'interactable' || kind === 'audio_source') && this.interactablePinned();
    }

    protected chooseAudioClip(id: string): void {
        if (!this.available() || !this.open() || this.draft().kind !== 'audio_source') return;
        if (!id || this.audioClips().some(clip => clip.asset_id === id)) this.fields.audioSource.clipId().value.set(id);
    }

    protected chooseSkyPanorama(id: string): void {
        if (this.available() && this.open() && this.draft().kind === 'sky') {
            this.fields.sky.panorama().value.set(id);
        }
    }

    protected allowAudioDrop(event: DragEvent): void {
        const transfer = event.dataTransfer;
        if (!this.available() || this.assetGeneration() === null || !transfer?.types.includes(audioClipDragType)) return;
        event.preventDefault();
        transfer.dropEffect = 'copy';
    }

    protected dropAudioClip(event: DragEvent): void {
        const id = readAudioClipDrag(event.dataTransfer, this.assetGeneration());
        if (id === null || !this.audioClips().some(clip => clip.asset_id === id)) return;
        event.preventDefault();
        this.chooseAudioClip(id);
    }

	protected start(component?: ComponentRecord): void {
		if (!this.available() || this.open()) return;
		const kind = component?.data.kind ?? this.selectedKind();
        if (this.pinned(kind) || (!component && !this.addTypes().some((type) => type.kind === kind))) return;
		this.original.set(component);
		this.draft.set(
			componentDraft(kind, component?.id ?? crypto.randomUUID(), this.ownerKey(), component),
		);
		this.collider.set(
			component?.data.kind === 'collider' ? component.data.collider : defaultCollider('box')!,
		);
		this.colliderInvalid.set(false);
		this.open.set(true);
	}

    revealReference(id: string, field: string): void {
        const component = this.components().find(record => record.id === id);
        if (!component) return;
        const context = this.contextKey();
        if (!this.open()) this.start(component);
        afterNextRender(() => {
            if (this.contextKey() !== context) return;
            const host = this.host.nativeElement;
            const reference = this.open() && this.original()?.id === id
                ? [...host.querySelectorAll<HTMLElement>('[data-component-reference-field]')]
                    .find(element => element.dataset['componentReferenceField'] === field)
                : undefined;
            const selector = 'input:enabled, select:enabled';
            const control = reference?.matches(selector) ? reference : reference?.querySelector<HTMLElement>(selector);
            const article = [...host.querySelectorAll<HTMLElement>('[data-reference-component]')]
                .find(element => element.dataset['referenceComponent'] === id);
            const destination = control ?? article;
            destination?.scrollIntoView?.({block: 'nearest'});
            destination?.focus({preventScroll: true});
        }, {injector: this.injector});
    }

	protected submit(): void {
		const component = this.candidate().value;
        if (!component || this.pinned(component.data.kind) || !this.available() || !this.open()) return;
		this.actionRequested.emit({
			kind: this.original() ? 'replace' : 'add',
			component,
			expected: this.original() ? JSON.stringify(this.original()) : null,
		});
	}

	protected toggle(component: ComponentRecord): void {
        if (this.available() && !this.open() && !this.pinned(component.data.kind))
			this.actionRequested.emit({
				kind: 'enabled',
				component_id: component.id,
				enabled: !component.enabled,
			});
	}

	protected remove(component: ComponentRecord): void {
        if (this.available() && !this.open() && !this.pinned(component.data.kind))
			this.actionRequested.emit({
				kind: 'remove',
				component_id: component.id,
				expected: JSON.stringify(component),
			});
	}

	protected changeCollider(collider: Collider): void {
		this.collider.set(collider);
		this.colliderInvalid.set(false);
	}
	protected changeInteraction(change: ColliderInteractionChange): void {
		this.changeCollider(changeColliderInteraction(this.collider(), change));
	}
	protected shape(kind: ColliderKind): void {
		const collider = defaultCollider(kind, this.collider().offset, this.collider().interaction);
		if (collider) this.changeCollider(collider);
		else this.colliderInvalid.set(true);
	}
	protected resetCollider(): void {
		const original = this.original();
		this.changeCollider(
			original?.data.kind === 'collider' ? original.data.collider : defaultCollider('box')!,
		);
	}
	protected addBinding(): void {
		if (this.draft().bindings.length < 32)
			this.draft.update((value) => ({
				...value,
				bindings: [...value.bindings, { name: '', target: '' }],
			}));
	}
	protected machineChanged(machine: MachineDefinition): void {
		this.draft.update((draft) => ({ ...draft, machine }));
	}
	protected animationChanged(animation: AnimationSettings): void {
		this.draft.update((draft) => ({ ...draft, animation }));
	}
	protected binding(index: number, field: 'name' | 'target', value: string): void {
		this.draft.update((draft) => ({
			...draft,
			bindings: draft.bindings.map((binding, position) =>
				position === index ? { ...binding, [field]: value } : binding,
			),
		}));
	}
	protected removeBinding(index: number): void {
		this.draft.update((draft) => ({
			...draft,
			bindings: draft.bindings.filter((_, position) => position !== index),
		}));
	}
}

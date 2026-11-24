import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import type {AnimationPaneRequest} from '../animation/animation-pane-request';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {checkedComponentChange, sceneComponentCommand} from './component-actions';
import {Component, computed, effect, inject, input, signal, viewChild} from '@angular/core';
import type {AudioAuditionScope} from '../../../../core/editor/audio-source.models';
import type { Collider, SceneEntity } from '../../../../core/editor/editor.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { ViewportEditorService } from '../../../../core/editor/viewport-editor.service';
import { SceneEditingService } from '../scene-editing.service';
import { WorkspaceViewService } from '../scene-viewport/workspace-view.service';
import { SelectedCameraPreviewService } from '../game-settings/selected-camera-preview.service';
import { InputActionsCatalog } from '../game-input/input-actions-catalog.service';
import { AnimatorInspector } from '../animator/animator-inspector';
import {ControlGraphInspector} from '../control-graph/control-graph-inspector';
import { ComponentEditor, type ComponentEditIntent } from './component-editor';
import type {SpecialistAttachment, SpecialistAttachmentChoice} from './add-component-catalog';
import {ComponentWorkspaceSession} from './component-workspace-session';
import {ParticleDesignerSession} from '../particle-designer/particle-designer-session';
import {MaterialGraphDialog} from '../material-graph/material-graph-dialog';

@Component({
	selector: 'editor-scene-component-inspector',
	imports: [ComponentEditor, AnimatorInspector, ControlGraphInspector, MaterialGraphDialog],
	template: `<editor-component-editor
		[components]="basicComponents()"
		[types]="types()"
        [attachmentCount]="entity()?.component_views?.length ?? null"
        [specialistAttachments]="specialistAttachments()"
        (specialistAttachmentRequested)="focusSpecialistAttachment($event)"
		[scriptTypes]="scriptTypes()"
		[ownerKey]="entity()?.key ?? ''"
        [initialCollider]="initialCollider()"
		[contextKey]="context()"
		[entities]="targets()"
		[models]="models()"
		[gameActions]="gameActions()"
		[interactablePinned]="interactablePinned()"
		[playerBodyIssue]="playerBodyIssue()"
		[inputActionAssets]="inputActions?.assets() ?? []"
		[inputActionAssetsEnabled]="inputActions?.supported() ?? false"
		[skyPanoramaAssetsEnabled]="skyPanoramaAssetsEnabled()"
        [behaviorWorkspaceAvailable]="behaviorWorkspaceAvailable()"
        (behaviorWorkspaceRequested)="openBehavior($event)"
        [audioWorkspaceAvailable]="audioWorkspaceAvailable()"
        (audioWorkspaceRequested)="openAudio($event)"
        [animationWorkspaceAvailable]="animationWorkspaceAvailable()"
        (animationWorkspaceRequested)="openAnimation($event)"
		[particleDesignerAvailable]="particleDesignerAvailable()"
		(particleDesignerRequested)="openParticleDesigner($event)"
		[editable]="available()"
		[previewAllowed]="true"
		[audioScope]="audioScope()"
		[assetGeneration]="generation()"
		[pending]="pending()"
		[error]="error()"
		[acknowledged]="acknowledged()"
		(actionRequested)="apply($event)"
	/>
    <editor-material-graph [entity]="entity()" [subject]="subject()" [editable]="editable()"/>
    @if (animatorSupported() && entity(); as actor) {
        <editor-animator-inspector [subject]="subject()" [actor]="actor" [editable]="editable()"/>
    }
    @if (canShowCameraActions()) {
        <button type="button" [disabled]="!canAlignCamera()" (click)="alignCamera()">Align Camera to Scene View</button>
    }
    @if (supportsDefaultCamera() && entity()?.camera) {
        <button #previewButton type="button" [disabled]="!canPreviewCamera()" (click)="previewCamera(previewButton)">Preview Selected Camera</button>
    }
    @if (entity(); as actor) {
        @if (actor.player_controller) { <editor-control-graph-inspector [subject]="subject()" [actor]="actor" [editable]="editable()"/> }
    }
    @if (showSceneRoles()) {
        <section aria-label="Scene roles" class="scene-roles">
            <h3>Scene roles</h3>
            @if (supportsDefaultCamera() && entity()?.camera) {
                @if (isDefaultCamera()) {
                    <p role="status">Default Game Camera</p>
                } @else {
                    <button type="button" [disabled]="!canSetDefault()" (click)="setDefaultCamera()">Set as Game Camera</button>
                }
            }
            @if (supportsSceneSky() && entity()?.sky; as sky) {
                @if (isSceneSky()) {
                    <p role="status">Active Scene Sky</p>
                } @else if (!sky.enabled) {
                    <p>Enable the Sky component to use it as the scene background.</p>
                } @else if (!canUseSceneSky()) {
                    <p>Choose a Sky on an independent root object.</p>
                }
                <button type="button" [disabled]="!available() || (!isSceneSky() && !canUseSceneSky())"
                        (click)="setSceneSky(!isSceneSky())">
                    {{ isSceneSky() ? 'Clear Scene Sky' : 'Set as Scene Sky' }}
                </button>
            }
            @if (supportsPlayerOwnership() && entity()?.player_controller; as player) {
                @if (playerBodyIssue(); as issue) {
                    <p role="status">{{ issue }}</p>
                } @else if (!player.enabled) {
                    <p role="status">Enable the Player Controller to assign local input.</p>
                }
                @if (player.settings.local) {
                    <button type="button" [disabled]="!available()" (click)="setLocalPlayer(false)">Clear Local Player</button>
                } @else {
                    <button type="button" [disabled]="!available() || !player.enabled || !!playerBodyIssue()"
                            (click)="setLocalPlayer(true)">Set Local Player</button>
                }
            }
        </section>
    }`,
    styles: '.scene-roles { display: grid; gap: .35rem; margin-block: .65rem; } .scene-roles :is(h3,p) { margin: 0; }',
})
export class SceneComponentInspector {
	readonly entity = input<SceneEntity | null>(null);
	readonly editable = input(false);
    readonly subject = input<InspectorSubject | null>(null);
    private readonly componentWorkspace = inject(ComponentWorkspaceSession, {optional: true});
    protected readonly behaviorWorkspaceAvailable = computed(() => {
        const access = this.subject()?.access;
        return !!this.componentWorkspace && (access === 'scene-edit' || access === 'linked-overrides');
    });
    protected readonly audioWorkspaceAvailable = computed(() =>
        this.componentWorkspace !== null && this.subject()?.access === 'scene-edit');
    private readonly animation = inject(AnimationWorkspaceService, {optional: true});
    protected readonly animationWorkspaceAvailable = computed(() => this.animation !== null && this.subject() !== null);
    private readonly componentEditor = viewChild(ComponentEditor);
    private readonly animatorInspector = viewChild(AnimatorInspector);
    private readonly controlsInspector = viewChild(ControlGraphInspector);
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService);
    private readonly particleDesigner = inject(ParticleDesignerSession, {optional: true});
    protected readonly particleDesignerAvailable = computed(() => {
        const access = this.subject()?.access;
        return !!this.particleDesigner && (access === 'scene-edit' || access === 'linked-overrides');
    });
	private readonly viewport = inject(ViewportEditorService, {optional: true});
	private readonly views = inject(WorkspaceViewService, {optional: true});
	private readonly selectedPreview = inject(SelectedCameraPreviewService, {optional: true});
	protected readonly inputActions = inject(InputActionsCatalog, {optional: true});
	protected readonly basicComponents = computed(() => this.entity()?.component_views?.filter(component => component.data.kind !== 'animator') ?? []);
    protected readonly scriptTypes = computed(() => this.editor.capabilities()?.object_scripts ?? []);
	protected readonly types = computed(() => this.editor.capabilities()?.component_types?.filter(type =>
        type.kind !== 'animator' && (type.kind !== 'script' || this.scriptTypes().length > 0)) ?? []);
	protected readonly animatorSupported = computed(() => this.editor.capabilities()?.animator_assets === true);
    protected readonly initialCollider = computed<Collider | null>(() =>
        this.entity()?.model_path === 'models/primitives/v1/plane.glb'
            ? {kind: 'box', half_extents: [0.5, 0.05, 0.5], offset: [0, -0.05, 0]}
            : null);
    protected readonly specialistAttachments = computed<readonly SpecialistAttachmentChoice[]>(() => {
        const actor = this.entity();
        if (!actor) return [];
        const choices: SpecialistAttachmentChoice[] = [];
        if (this.animatorSupported()) {
            const previous = actor.animator !== undefined && actor.animator !== null;
            const legacy = actor.animation?.enabled || this.basicComponents().some(component =>
                component.enabled && component.data.kind === 'animation');
            choices.push({
                id: 'animator',
                unavailableReason: previous ? 'The object already has an Animator.'
                    : legacy ? 'Disable its existing Animation component before attaching an Animator.' : undefined,
            });
        }
        if (this.editor.capabilities()?.controls_graph_assets) {
            const controller = actor.player_controller;
            choices.push({
                id: 'controls_graph',
                unavailableReason: !controller ? 'Attach a Player Controller before assigning a Controls Graph.'
                    : controller.settings.control_graph ? 'The Player Controller already has a Controls Graph.' : undefined,
            });
        }
        return choices;
    });
	protected readonly targets = computed(() => this.editor.snapshot()?.document.entities ?? []);
	protected readonly models = computed(() => this.editor.modelAssets() ?? []);
	protected readonly generation = this.editor.workspaceGeneration;
	protected readonly audioScope = computed<AudioAuditionScope | null>(() => {
		const key = this.entity()?.key;
		const snapshot = this.editor.snapshot();
		const nativeGeneration = this.viewport?.state()?.workspace_generation;
		if (!key || !snapshot || snapshot.mode !== 'edit' || snapshot.transaction ||
			nativeGeneration == null || !snapshot.selection.includes(key) || this.selectedPreview?.active() ||
			(this.views && this.views.activeViewKind() !== 'scene-viewport')) return null;
		return {owner: {kind: 'scene', scene_id: snapshot.active_scene_id, key}, generation: this.generation(),
			nativeGeneration, revision: snapshot.revision, contentRevision: snapshot.scene_revision};
	});
    protected readonly gameActions = computed(() =>
        this.editor.snapshot()?.project.input_profile?.actions?.map(action => action.id) ?? [],
    );
    protected readonly interactablePinned = computed(() => {
        const entity = this.entity();
        if (!entity) return false;
        if (entity.linked_prefab) return true;
        if (!entity.parent) return false;
        const byKey = new Map(this.targets().map(candidate => [candidate.key, candidate]));
        const visited = new Set([entity.key]);
        let parentKey: string | null | undefined = entity.parent;
        while (parentKey && !visited.has(parentKey)) {
            visited.add(parentKey);
            const ancestor = byKey.get(parentKey);
            if (ancestor?.linked_prefab) return true;
            parentKey = ancestor?.parent;
        }
        return false;
    });
	protected readonly pending = signal(false);
	protected readonly error = signal<string | null>(null);
	protected readonly acknowledged = signal(0);
	protected readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.active_scene_id,
			this.entity()?.key,
		]),
	);
	protected readonly available = computed(
		() => this.editable() && !this.pending() && this.types().length > 0,
	);
	protected readonly canShowCameraActions = computed(() => !!this.entity()?.camera &&
		(this.editor.capabilities()?.commands?.some(command => command.name === 'align_camera_to_scene_view') ?? false));
	protected readonly supportsDefaultCamera = computed(() =>
		this.editor.capabilities()?.commands?.some(command => command.name === 'set_default_game_camera') ?? false);
	protected readonly supportsSceneSky = computed(() =>
		this.editor.capabilities()?.commands?.some(command => command.name === 'set_active_sky') ?? false);
	protected readonly skyPanoramaAssetsEnabled = computed(() => this.editor.capabilities()?.sky_panorama_assets === true);
	protected readonly isSceneSky = computed(() => {
		const key = this.entity()?.key;
		return key !== undefined && this.editor.document()?.environment?.active_sky === key;
	});
	protected readonly canUseSceneSky = computed(() => sceneSkyCandidate(this.entity()));
	protected readonly supportsPlayerOwnership = computed(() =>
		this.editor.capabilities()?.commands?.some(command => command.name === 'set_local_player') ?? false);
    protected readonly showSceneRoles = computed(() => {
        const object = this.entity();
        return !!(object?.camera && this.supportsDefaultCamera() ||
            object?.sky && this.supportsSceneSky() ||
            object?.player_controller && this.supportsPlayerOwnership());
    });
	protected readonly playerBodyIssue = computed(() => playerBodyIssue(this.entity()));
	protected readonly isDefaultCamera = computed(() => {
		const key = this.entity()?.key;
		return !!key && this.editor.document()?.default_game_camera === key;
	});
	protected readonly canSetDefault = computed(() => this.supportsDefaultCamera() && this.available() &&
		this.entity()?.camera?.enabled === true && !this.isDefaultCamera() && !this.selectedPreview?.active());
	protected readonly canPreviewCamera = computed(() => this.supportsDefaultCamera() && this.available() &&
		this.entity()?.camera?.enabled === true && this.selectedPreview?.available() === true);
	protected readonly canAlignCamera = computed(() => {
		const view = this.viewport?.state();
		const snapshot = this.editor.snapshot();
		return this.canShowCameraActions() && this.available() && snapshot?.mode === 'edit' &&
			!this.selectedPreview?.active() &&
			snapshot.selection.length === 1 && snapshot.selection[0] === this.entity()?.key &&
			(!this.views || this.views.activeViewKind() === 'scene-viewport') &&
			view?.settings.view === 'scene' && view.workspace_generation !== null &&
			!view.gesture && !view.placement && !this.viewport?.pending();
	});

	constructor() {
		effect(() => {
			this.context();
			this.error.set(null);
		});
	}

    protected openBehavior(componentId: string): void {
        const subject = this.subject();
        if (subject && this.available()) void this.componentWorkspace?.open('behavior-workspace', subject, componentId);
    }

    protected openAudio(componentId: string): void {
        const subject = this.subject();
        if (subject && this.available()) void this.componentWorkspace?.open('audio-workspace', subject, componentId);
    }

    protected async openAnimation(componentId: string): Promise<void> {
        if (!this.animation || !this.available()) return;
        const context = this.context();
        const resolve = (): AnimationPaneRequest | null => {
            const subject = this.subject();
            const record = this.basicComponents().find(component => component.id === componentId);
            if (!subject || record?.data.kind !== 'animation') return null;
            const {source, playback} = record.data.settings;
            return {
                library: 'imported', subject, componentId,
                asset: {id: source.asset_id, fingerprint: source.expected_sha256},
                clipIndex: playback.kind === 'single_clip' ? playback.clip_index : playback.idle,
            };
        };
        const request = resolve();
        if (!request) return;
        try {
            await this.animation.openScoped(request, resolve);
        } catch (error) {
            if (context === this.context()) this.error.set(error instanceof Error ? error.message : String(error));
        }
    }

    protected focusSpecialistAttachment(kind: SpecialistAttachment): void {
        if (!this.available() || !this.specialistAttachments().some(choice => choice.id === kind && !choice.unavailableReason)) return;
        if (kind === 'animator') this.animatorInspector()?.focusAttachment();
        else this.controlsInspector()?.focusAttachment();
    }

    revealReference(componentId: string, field: string): boolean {
        const editor = this.componentEditor();
        if (!editor || !this.entity()?.component_views?.some(component => component.id === componentId))
            return false;
        editor.revealReference(componentId, field);
        return true;
    }

	protected async alignCamera(): Promise<void> {
		const context = this.context();
		const key = this.entity()?.key;
		const viewport = this.viewport;
		if (!key || !viewport || !this.canAlignCamera()) return;
		this.pending.set(true);
		this.error.set(null);
		const current = () => this.context() === context;
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				const snapshot = this.editor.snapshot();
				const view = viewport.state();
				if (!current() || snapshot?.mode !== 'edit' || snapshot.selection.length !== 1 || snapshot.selection[0] !== key ||
					view?.settings.view !== 'scene' || view.workspace_generation === null) return;
				await viewport.action({
					type: 'align_camera_to_scene_view', key, expected_revision: snapshot.revision,
					expected_generation: view.workspace_generation, scene_id: snapshot.active_scene_id,
				});
				if (current()) await this.editor.refresh();
			});
			if (!accepted && current()) this.error.set(this.editing.error() ?? 'Finish the current edit before aligning the Camera.');
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}

	protected previewCamera(origin: HTMLElement): void {
		const key = this.entity()?.key;
		if (key && this.canPreviewCamera()) void this.selectedPreview?.open(key, origin);
	}

	protected async setDefaultCamera(): Promise<void> {
		const context = this.context();
		const key = this.entity()?.key;
		const generation = this.editor.workspaceGeneration();
		if (!key || !this.canSetDefault()) return;
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				const snapshot = this.editor.snapshot();
				if (context !== this.context() || snapshot?.mode !== 'edit' || this.entity()?.camera?.enabled !== true) return;
				await this.editor.execute({type: 'set_default_game_camera', key}, generation, snapshot.revision);
			});
			if (!accepted && context === this.context()) this.error.set(this.editing.error() ?? 'Finish the current edit before choosing the Game Camera.');
		} catch (error) {
			if (context === this.context()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}

	protected async setLocalPlayer(assign: boolean): Promise<void> {
		const entity = this.entity();
		if (!this.supportsPlayerOwnership() || !this.available() || !entity?.player_controller ||
			assign === entity.player_controller.settings.local ||
			(assign && (!entity.player_controller.enabled || playerBodyIssue(entity)))) return;
		const context = this.context();
		const generation = this.editor.workspaceGeneration();
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				const snapshot = this.editor.snapshot();
				if (context !== this.context() || snapshot?.mode !== 'edit') return;
				const latest = snapshot.document.entities.find(item => item.key === entity.key);
				if (!latest?.player_controller || latest.player_controller.settings.local === assign ||
					(assign && (!latest.player_controller.enabled || playerBodyIssue(latest)))) return;
				await this.editor.execute({type: 'set_local_player', key: assign ? entity.key : null}, generation, snapshot.revision);
			});
			if (!accepted && context === this.context()) this.error.set(this.editing.error() ?? 'Finish the current edit before changing local input ownership.');
		} catch (error) {
			if (context === this.context()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}

    protected openParticleDesigner(componentId: string): void {
        const key = this.entity()?.key;
        const subject = this.subject();
        if (key && subject && this.available()) void this.particleDesigner?.openScoped(subject, key, componentId);
    }

	protected async setSceneSky(assign: boolean): Promise<void> {
		const key = this.entity()?.key;
		const context = this.context();
		const generation = this.editor.workspaceGeneration();
		if (!key || !this.available() || !this.supportsSceneSky() || assign === this.isSceneSky() ||
			(assign && !this.canUseSceneSky())) return;
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				const snapshot = this.editor.snapshot();
				if (context !== this.context() || snapshot?.mode !== 'edit') return;
				const latest = snapshot.document.entities.find(entity => entity.key === key);
				const active = snapshot.document.environment?.active_sky;
				if (assign ? !sceneSkyCandidate(latest) || active === key : active !== key) return;
				await this.editor.execute({type: 'set_active_sky', key: assign ? key : null}, generation, snapshot.revision);
			});
			if (!accepted && context === this.context()) {
				this.error.set(this.editing.error() ?? 'Finish the current edit before choosing the Scene Sky.');
			}
		} catch (error) {
			if (context === this.context()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}

	protected async apply(intent: ComponentEditIntent): Promise<void> {
		const entity = this.entity();
		const context = this.context();
		const generation = this.editor.workspaceGeneration();
		if (!entity || !this.available()) return;
		const current = () => context === this.context() && this.editor.snapshot()?.mode === 'edit';
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				try {
					const snapshot = this.editor.snapshot();
					if (!snapshot || !current()) return;
					const latest = snapshot.document.entities.find(
						(candidate) => candidate.key === entity.key,
					);
					if (!latest) return;
					if (!checkedComponentChange(latest.component_views ?? [], intent)) return;
					await this.editor.execute(sceneComponentCommand(entity.key, intent), generation, snapshot.revision);
					if (current()) this.acknowledged.update((value) => value + 1);
				} catch (error) {
					if (current()) this.error.set(error instanceof Error ? error.message : String(error));
				}
			});
			if (!accepted && current())
				this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
}

function sceneSkyCandidate(entity: SceneEntity | null | undefined): boolean {
	return entity?.sky?.enabled === true && entity.parent == null && entity.linked_prefab == null;
}

function playerBodyIssue(entity: SceneEntity | null): string | null {
	if (!entity) return null;
	if (entity.parent || entity.linked_prefab) return 'Use an independent scene-root object for the Player Controller.';
	const motor = entity.character_controller;
	if (!motor?.enabled) return 'Enable a Character Controller on this object before assigning player input.';
	if (entity.transform.rotation.some(axis => Math.abs(axis) > 1e-6) ||
		entity.transform.scale.some(axis => Math.abs(axis - 1) > 1e-6)) {
		return 'Set the Player body to zero rotation and unit scale.';
	}
	const collider = entity.collider;
	if (collider?.kind !== 'box' || collider.interaction?.sensor || entity.collision_enabled === false) {
		return 'Add an enabled box collider and turn off Sensor.';
	}
	if (motor.settings.ground_probe >= Math.min(...collider.half_extents) ||
		motor.settings.step_height >= 2 * collider.half_extents[1]) {
		return 'Adjust the ground probe or step height to fit the box collider.';
	}
	return null;
}

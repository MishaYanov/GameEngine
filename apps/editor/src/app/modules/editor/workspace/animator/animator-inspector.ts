import {Component, computed, DestroyRef, effect, ElementRef, inject, input, linkedSignal, signal, untracked, viewChild} from '@angular/core';
import {canResolveAnimationSource, type AnimationReview} from '../../../../core/editor/animation.models';
import {
    animatorChannelKey, type AnimatorActorValidation, type AnimatorBinding, type AnimatorDocument, type AnimatorParameterSource,
    type AnimatorSettings,
    type FootIkSettings,
} from '../../../../core/editor/animator.models';
import type {EditorCommand, SceneEntity} from '../../../../core/editor/editor.models';
import type {WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {AnimatorAssetEditor} from './animator-asset-editor';
import {animatorDescendants, type ActorGraph, reviewActorGraph} from './animator-actor-binding';
import {AnimatorAssets} from './animator-assets.service';
import {type AnimatorPaneTarget, AnimatorWorkspaceService} from './animator-workspace.service';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';

interface ReviewedAnimator {
    readonly document: AnimatorDocument;
    readonly actor: ActorGraph | null;
    readonly ankleJoints: readonly AnkleJoint[];
}

interface AnkleJoint {
    readonly id: string;
    readonly name: string;
}

@Component({
    selector: 'editor-animator-inspector',
    imports: [AnimatorAssetEditor],
    providers: [AnimatorAssets],
    templateUrl: './animator-inspector.html',
    styleUrl: './animator-inspector.scss',
})
export class AnimatorInspector {
    protected readonly workspace = inject(AnimatorWorkspaceService, {optional: true});
    readonly actor = input.required<SceneEntity>();
    readonly editable = input(false);
    readonly subject = input<InspectorSubject | null>(null);
    private readonly assetPicker = viewChild<ElementRef<HTMLSelectElement>>('asset');
    protected readonly library = inject(AnimatorAssets);
    private readonly editor = this.library.editor;
    private readonly editing = inject(SceneEditingService);
    protected readonly selected = signal('');
    protected readonly rootMotionNode = linkedSignal(() => {
        const settings = this.actor().animator?.settings;
        return settings?.asset_id === this.selected() ? settings.root_motion_source_node?.toString() ?? '' : '';
    });
    protected readonly footIkDraft = linkedSignal(() => {
        const settings = this.actor().animator?.settings;
        const foot = settings?.asset_id === this.selected() ? settings.foot_ik : undefined;
        return {
            joint: foot?.foot_joint ?? '', reach: (foot?.reach_mm ?? 250).toString(),
            offset: (foot?.sole_offset_mm ?? 100).toString()
        };
    });
    protected readonly ankleJoints = computed(() => this.reviewed()?.ankleJoints ?? []);
    protected readonly footIkIssue = computed(() => {
        const draft = this.footIkDraft();
        if (!draft.joint || this.plan().binding?.kind !== 'imported') return null;
        if (!this.ankleJoints().some(joint => joint.id === draft.joint)) return 'Choose an available foot joint.';
        if (!validMm(draft.reach, 1) || !validMm(draft.offset, 0)) {
            return 'Enter a whole number from 1 to 1000 for reach and 0 to 1000 for the sole offset.';
        }
        return null;
    });
    protected readonly rootMotionIssue = computed(() => {
        const value = this.rootMotionNode();
        if (this.plan().binding?.kind !== 'imported' || !value) return null;
        if (!/^[0-9]+$/.test(value) || Number(value) > 0xffffffff) return 'Enter a nonnegative glTF node index.';
        return this.actor().character_controller?.enabled ||
            this.actor().component_views?.some(component => component.enabled && component.data.kind === 'character_controller')
            ? null : 'Root motion requires an enabled Character Controller on this object.';
    });
    private readonly reviewed = signal<ReviewedAnimator | null>(null);
    protected readonly graph = computed(() => this.reviewed()?.document.graph ?? null);
    protected readonly entry = computed(() => this.graph()?.states.find(state => state.id === this.graph()?.entry));
    protected readonly channels = computed(() => {
        const review = this.reviewed()?.actor;
        return review?.kind === 'visual' ? review.channels : [];
    });
    protected readonly targets = computed(() => this.channels().length
        ? animatorDescendants(this.actor(), this.editor.document() ?? null) : []);
    protected readonly choices = signal<ReadonlyMap<string, string>>(new Map());
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    private readonly receipt = signal<{readonly revision: number; readonly value: AnimatorActorValidation} | null>(null);
    protected readonly validation = computed(() => {
        const receipt = this.receipt();
        return receipt && receipt.revision === this.editor.snapshot()?.revision ? receipt.value : null;
    });
    protected readonly available = computed(() => this.editable() && !this.pending() && !!this.library.scope() &&
        this.editor.snapshot()?.mode === 'edit');
    protected readonly attachedAnimator = computed(() => {
        const id = this.actor().animator?.settings.asset_id;
        return id ? this.library.assets().find(asset => asset.id === id)?.path ?? 'Unavailable Animator' : null;
    });
    protected readonly canOpenAttached = computed(() => this.available() && !!this.paneTarget() && !this.workspace?.pending());
    protected readonly legacyActive = computed(() => this.actor().animation?.enabled === true ||
        this.actor().component_views?.some(component => component.enabled && component.data.kind === 'animation') === true);
    protected readonly plan = computed<{binding: AnimatorBinding | null; issue: string | null}>(() => {
        const review = this.reviewed();
        if (!review?.actor) return {binding: null, issue: null};
        const current = this.library.assets().find(asset => asset.id === review.document.asset.id);
        if (current && current.fingerprint !== review.document.asset.fingerprint) {
            return {binding: null, issue: 'This Animator asset changed. Review it again.'};
        }
        if (review.actor.kind === 'imported') {
            const actor = this.actor();
            const model = actor.asset_id ?? this.editor.modelAssets()?.find(asset => asset.path === actor.model_path)?.asset_id;
            if (!actor.renderable || model !== review.actor.source.asset_id) {
                return {binding: null, issue: 'Assign the graph’s GLB model to this actor before attaching its Animator.'};
            }
            return {binding: review.actor, issue: null};
        }
        const keys = new Set(this.targets().map(entity => entity.key));
        const targets = [];
        for (const row of review.actor.channels) {
            const chosen = this.choices().get(row.id);
            if (chosen && keys.has(chosen)) targets.push({channel: row.channel, entity_key: chosen});
            else if (chosen || !row.automatic || row.channel.kind !== 'scene' || !keys.has(row.channel.entity_key)) {
                return {binding: null, issue: `Choose a renderable child for visual channel "${row.label}".`};
            }
        }
        if (targets.length > 128) return {binding: null, issue: 'An Animator supports up to 128 explicitly bound visual channels.'};
        return {binding: targets.length ? {kind: 'visual', targets} : {kind: 'visual'}, issue: null};
    });
    private readonly context = computed(() => JSON.stringify([
        this.actor().key, this.editor.workspaceGeneration(), this.editor.snapshot()?.active_scene_id,
        this.library.scope()?.nativeGeneration,
    ]));
    private seenContext = '';
    private request = 0;

    constructor() {
        effect(() => {
            const context = this.context();
            const scope = this.library.scope();
            const attached = this.actor().animator;
            if (context === this.seenContext) return;
            this.seenContext = context;
            untracked(() => {
                this.request++;
                this.pending.set(false);
                this.error.set(null);
                this.receipt.set(null);
                this.selected.set('');
                this.reviewed.set(null);
                this.choices.set(new Map());
                if (scope && attached) void this.review(attached.settings.asset_id);
            });
        });
        inject(DestroyRef).onDestroy(() => this.request++);
    }

    focusAttachment(): void {
        if (this.available()) this.assetPicker()?.nativeElement.focus();
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

    private paneTarget(): AnimatorPaneTarget | null {
        const subject = this.subject(), snapshot = this.editor.snapshot();
        const key = this.actor().key;
        if (!subject || subject.address.kind === 'prop' || snapshot?.mode !== 'edit' ||
            snapshot.active_scene_id !== subject.address.scene || snapshot.selection[0] !== key ||
            (subject.address.kind === 'scene' && subject.address.object !== key)) return null;
        const attachment = snapshot.document.entities.find(entity => entity.key === key)?.animator;
        if (!attachment) return null;
        const id = attachment.settings.asset_id;
        const asset = this.library.assets().find(candidate => candidate.id === id) ?? {id};
        return {actorKey: key, subject, componentId: attachment.id, asset};
    }

    protected async review(id: string): Promise<void> {
        const scope = this.library.scope();
        if (!scope || this.pending()) return;
        this.selected.set(id);
        this.reviewed.set(null);
        this.choices.set(new Map());
        this.receipt.set(null);
        if (!id) return;
        await this.run(async (current, currentScope) => {
            const document = await this.editor.openAnimator(id, currentScope);
            if (!current()) return;
            if (document.asset.id !== id) throw new Error('The editor opened a different Animator asset.');
            await this.adopt(document, currentScope, current);
        });
    }

    protected async fromEditor(document: AnimatorDocument): Promise<void> {
        if (this.pending()) return;
        this.selected.set(document.asset.id);
        this.reviewed.set(null);
        this.receipt.set(null);
        await this.run((current, scope) => this.adopt(document, scope, current));
    }

    protected chooseTarget(id: string, key: string): void {
        if (!this.available() || !this.channels().some(channel => channel.id === id) ||
            key && !this.targets().some(entity => entity.key === key)) return;
        this.choices.update(prior => {
            const later = new Map(prior);
            if (key) later.set(id, key);
            else later.delete(id);
            return later;
        });
    }

    protected updateFootIk(field: 'joint' | 'reach' | 'offset', value: string): void {
        this.footIkDraft.update(draft => ({...draft, [field]: value}));
    }

    protected async assign(): Promise<void> {
        const document = this.reviewed()?.document;
        const binding = this.plan().binding;
        const prior = this.actor().animator;
        const key = this.actor().key;
        if (!this.available() || !document || !binding || this.legacyActive() || this.rootMotionIssue() || this.footIkIssue()) return;
        const rootNode = this.rootMotionNode();
        const foot = this.footIkDraft();
        const footIk: FootIkSettings | undefined = binding.kind === 'imported' && foot.joint
            ? {foot_joint: foot.joint, reach_mm: Number(foot.reach), sole_offset_mm: Number(foot.offset)} : undefined;
        const settings: AnimatorSettings = {
            asset_id: document.asset.id, binding,
            ...(binding.kind === 'imported' && rootNode ? {root_motion_source_node: Number(rootNode)} : {}),
            ...(footIk ? {foot_ik: footIk} : {}),
        };
        const component = {
            id: prior?.id ?? crypto.randomUUID(), schema_version: 1, enabled: true,
            data: {kind: 'animator', settings},
        } as const;
        await this.mutate({type: prior ? 'replace_component' : 'add_component', key, component}, true);
    }

    protected async validate(): Promise<void> {
        if (!this.available() || !this.actor().animator) return;
        const key = this.actor().key;
        await this.run((current, scope) => this.recordValidation(key, scope, current));
    }

    protected toggle(enabled: boolean): void {
        const attachment = this.actor().animator;
        if (!attachment || enabled && this.legacyActive()) return;
        void this.mutate({type: 'set_component_enabled', key: this.actor().key, component_id: attachment.id, enabled}, enabled);
    }

    protected remove(): void {
        const attachment = this.actor().animator;
        if (attachment) void this.mutate({type: 'remove_component', key: this.actor().key, component_id: attachment.id}, false);
    }

    protected parameterKind(source: AnimatorParameterSource): string {
        switch (source.kind) {
            case 'action_accepted': return 'Accepted action · ' + source.action;
            case 'boolean': return source.default ? 'Boolean · true' : 'Boolean · false';
            case 'grounded': return 'Grounded';
            case 'jumped': return 'Jumped trigger';
            case 'moving': return 'Moving · ' + source.minimum_mm_per_second + ' mm/s';
        }
    }

    private async adopt(document: AnimatorDocument, scope: WorkspaceAssetScope, current: () => boolean): Promise<void> {
        const scene = this.editor.snapshot()?.active_scene_id;
        if (!scene) return;
        this.reviewed.set({document, actor: null, ankleJoints: []});
        const actor = await reviewActorGraph(document.graph, scene, async id => {
            if (!current()) throw new Error('The Animator review expired.');
            return this.editor.openAuthoredClip(id, scope.workspaceGeneration);
        });
        if (actor.kind === 'imported' && !this.actor().asset_id) await this.editor.assets();
        let ankleJoints: readonly AnkleJoint[] = [];
        if (current() && actor.kind === 'imported' && actor.source.character) {
            const rig = await this.editor.inspectAnimation(actor.source.asset_id, scope.workspaceGeneration, scope.revision);
            if (rig.workspace_generation !== scope.nativeGeneration || !canResolveAnimationSource(actor.source, rig.metadata.source)) {
                throw new Error('The imported Character changed during Animator review.');
            }
            ankleJoints = availableAnkles(rig, new Set(Object.values(actor.source.character.joints)));
        }
        if (!current()) return;
        this.library.accept(document.asset, scope);
        const attachment = this.actor().animator;
        const bound = attachment?.settings.asset_id === document.asset.id ? attachment.settings.binding : null;
        this.choices.set(new Map(bound?.kind === 'visual'
            ? (bound.targets ?? []).map(item => [animatorChannelKey(item.channel), item.entity_key]) : []));
        this.reviewed.set({document, actor, ankleJoints});
    }

    private async mutate(command: EditorCommand, validate: boolean): Promise<void> {
        if (!this.available()) return;
        const key = this.actor().key;
        const prior = JSON.stringify(this.actor().animator ?? null);
        await this.run(async current => {
            const accepted = await this.editing.withCommittedEdit(async () => {
                const scope = this.library.scope();
                const latest = this.editor.document()?.entities.find(entity => entity.key === key);
                if (!scope || !current() || !latest || JSON.stringify(latest.animator ?? null) !== prior) return;
                await this.editor.execute(command, scope.workspaceGeneration, scope.revision);
                if (!current()) return;
                this.receipt.set(null);
                const after = this.library.scope();
                if (validate && after) await this.recordValidation(key, after, current);
            });
            if (!accepted && current()) this.error.set(this.editing.error() ?? 'Finish the current edit before changing the Animator.');
        });
    }

    private async recordValidation(key: string, scope: WorkspaceAssetScope, current: () => boolean): Promise<void> {
        const value = await this.editor.validateAnimatorActor(key, scope);
        if (current() && this.editor.snapshot()?.revision === scope.revision) {
            this.receipt.set({revision: scope.revision, value});
        }
    }

    private async run(operation: (current: () => boolean, scope: WorkspaceAssetScope) => Promise<void>): Promise<void> {
        const scope = this.library.scope();
        if (!scope || this.pending()) return;
        const request = ++this.request;
        const context = this.context();
        const current = () => request === this.request && this.context() === context && this.library.current(scope);
        this.error.set(null);
        this.pending.set(true);
        try {
            await operation(current, scope);
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.pending.set(false);
        }
    }
}

function availableAnkles(rig: AnimationReview, pinned: ReadonlySet<string>): AnkleJoint[] {
    const joints = rig.metadata.joint_metadata ?? [];
    const parents = new Map(joints.map(joint => [joint.source_node, joint]));
    return joints.flatMap(joint => {
        const knee = parents.get(joint.parent_source_node ?? -1);
        const hip = parents.get(knee?.parent_source_node ?? -1);
        return joint.stable_id && pinned.has(joint.stable_id) && knee && hip
            ? [{id: joint.stable_id, name: joint.name}] : [];
    });
}

function validMm(value: string, minimum: number): boolean {
    return /^\d{1,4}$/.test(value) && Number(value) >= minimum && Number(value) <= 1000;
}

import {computed, effect, inject, Injectable, signal} from '@angular/core';
import type {EditorSnapshot, ParticleEmitterComponent, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {type InspectorSubject, inspectorAddressKey, linkedInspectorRoot, linkedInspectorSubject, sceneInspectorSubject} from '../inspector-panel/inspector-subject';
import {ScopedPaneRequests} from '../scoped-pane-request';
import {DEFAULT_PARTICLE_EMITTER} from '../../../../core/editor/particle-emitter.models';
import {SceneEditingService} from '../scene-editing.service';
import {
    draftFromSettings,
    type ParticleDraft,
    PARTICLE_PRESETS,
    particlePresetLabel,
    sameParticleSettings,
    settingsFromDraft,
} from './particle-designer.models';

interface DraftTarget {
    readonly generation: number;
    readonly scene: string;
    readonly key: string;
    readonly name: string;
    readonly original: ParticleEmitterComponent;
    readonly subject: InspectorSubject;
}

@Injectable()
export class ParticleDesignerSession {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly panes = inject(ScopedPaneRequests);
    private readonly target = signal<DraftTarget | null>(null);
    private readonly exactColor = signal(DEFAULT_PARTICLE_EMITTER.color);

    readonly draft = signal<ParticleDraft>(draftFromSettings(DEFAULT_PARTICLE_EMITTER));
    readonly candidate = computed(() => settingsFromDraft(this.draft(), this.exactColor()));
    readonly active = computed(() => this.target() !== null);
    readonly spritePreview = signal<Blob | null>(null);
    readonly spriteError = signal<string | null>(null);
    private readonly spriteId = computed(() => this.draft().sprite_asset_id);
    private readonly assetRevision = computed(() => this.editor.snapshot()?.revision);
    readonly objectName = computed(() => this.target()?.name ?? '');
    private readonly pendingState = signal(false);
    readonly pending = this.pendingState.asReadonly();
    private readonly closeReviewState = signal(false);
    readonly closeReview = this.closeReviewState.asReadonly();
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly announcementState = signal<string | null>(null);
    readonly announcement = this.announcementState.asReadonly();
    readonly targetError = computed(() => this.targetIssue(this.target(), this.editor.snapshot()));
    readonly dirty = computed(() => {
        const target = this.target();
        const candidate = this.candidate();
        return !!target && (!candidate || !sameParticleSettings(candidate, target.original.settings));
    });
    readonly canApply = computed(() => this.active() && this.dirty() && !!this.candidate() &&
        !this.targetError() && !this.pending() && this.editing.ready());
    readonly preset = computed(() => {
        const candidate = this.candidate();
        return candidate ? particlePresetLabel(candidate) : '';
    });

    constructor() {
        effect(onCleanup => {
            const target = this.target();
            const id = this.spriteId();
            const revision = this.assetRevision();
            this.spritePreview.set(null);
            this.spriteError.set(null);
            if (!target || !id || revision === undefined || this.targetError()) return;
            let current = true;
            onCleanup(() => {
                current = false;
            });
            void this.editor.particleSprite(id, target.generation, revision).then(sprite => {
                if (current) this.spritePreview.set(sprite);
            }, (cause: unknown) => {
                if (current) this.spriteError.set(cause instanceof Error ? cause.message : 'Could not preview the selected sprite.');
            });
        });
    }

    /** Opens the exact selected emitter created by the last accepted Scene command. */
    openSelected(key: string): Promise<boolean> {
        const selected = this.selectedEmitter(key);
        return selected ? this.openScoped(selected.subject, key, selected.original.id) : Promise.resolve(false);
    }

    async openScoped(subject: InspectorSubject, key: string, componentId: string): Promise<boolean> {
        const address = subject.address;
        if (address.kind === 'prop' || (address.kind === 'scene' &&
            (subject.access !== 'scene-edit' || address.object !== key)) ||
            (address.kind === 'linked' && subject.access !== 'linked-overrides')) return false;
        return this.panes.open<DraftTarget>({pane: 'particle-designer', subject, componentId}, {
            resolve: () => {
                const selected = this.selectedEmitter(key, subject);
                return selected?.original.id === componentId ? {subject: selected.subject, componentId} : null;
            },
            load: async target => {
                const selected = this.selectedEmitter(key, subject);
                if (!selected || selected.original.id !== componentId ||
                    !await this.validLinkedAddress(key, target.subject)) return null;
                return {value: {
                    generation: target.subject.scope.workspaceGeneration, subject: target.subject,
                    scene: address.scene, key, name: selected.name, original: selected.original,
                }};
            },
            activate: target => {
                if (this.active()) return false;
                this.activate(target);
                return true;
            },
        });
    }

    private selectedEmitter(key: string, expected?: InspectorSubject):
        {subject: InspectorSubject; name: string; original: ParticleEmitterComponent} | null {
        const snapshot = this.editor.snapshot();
        const nativeGeneration = this.viewport.state()?.workspace_generation;
        if (!snapshot || snapshot.mode !== 'edit' || nativeGeneration == null || snapshot.selection[0] !== key) return null;
        const object = snapshot.document.entities.find(entity => entity.key === key);
        const original = particleEmitter(object);
        if (!object || !original) return null;
        const scope = {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision};
        const root = linkedInspectorRoot(object, snapshot.document.entities);
        const linked = expected?.address.kind === 'linked' ? expected.address : null;
        if (root) {
            if (linked?.root !== root || linked.scene !== snapshot.active_scene_id) return null;
            return {original, name: object.name,
                subject: {address: linked, scope, contentRevision: snapshot.scene_revision, access: 'linked-overrides'}};
        }
        return linked ? null : {original, name: object.name, subject: sceneInspectorSubject(snapshot, key, scope, true)};
    }

    private async validLinkedAddress(key: string, subject: InspectorSubject): Promise<boolean> {
        const {address, scope} = subject;
        if (address.kind !== 'linked') return true;
        try {
            const result = await this.editor.inspectLinkedPropAddress(key, scope);
            const snapshot = this.editor.snapshot();
            const verified = snapshot && linkedInspectorSubject(snapshot,
                {object: key, scene: address.scene, root: address.root, scope}, result, true);
            return !!verified && inspectorAddressKey(address, scope.workspaceGeneration, scope.nativeGeneration) ===
                inspectorAddressKey(verified.address, scope.workspaceGeneration, scope.nativeGeneration);
        } catch {
            return false;
        }
    }

    private activate(target: DraftTarget): void {
        this.setDraft(target.original.settings);
        this.errorState.set(null);
        this.announcementState.set(null);
        this.closeReviewState.set(false);
        this.target.set(target);
    }

    canLeaveProject(): boolean {
        if (!this.active()) return true;
        if (this.pending() || this.dirty()) {
            this.requestCloseReview();
            return false;
        }
        this.discardAndClose();
        return true;
    }

    pickPreset(label: string): void {
        if (this.pending()) return;
        const preset = PARTICLE_PRESETS.find(item => item.label === label);
        if (preset) {
            const {sprite_asset_id, linger_on_despawn} = this.draft();
            this.setDraft({...preset.settings, ...(sprite_asset_id ? {sprite_asset_id} : {}), linger_on_despawn});
        }
    }

    requestCloseReview(): void {
        if (this.active()) this.closeReviewState.set(true);
    }

    keepEditing(): void {
        if (!this.pending()) this.closeReviewState.set(false);
    }

    discardAndClose(): void {
        if (this.pending()) return;
        this.target.set(null);
        this.closeReviewState.set(false);
    }

    async apply(close = false): Promise<void> {
        const target = this.target();
        const candidate = this.candidate();
        if (!this.canApply() || !target || !candidate) return;
        this.pendingState.set(true);
        this.errorState.set(null);
        this.announcementState.set(null);
        let applied = false;
        try {
            const accepted = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                const issue = this.targetIssue(target, snapshot);
                if (issue || !snapshot || this.target() !== target) {
                    this.errorState.set(issue ?? 'Reopen Particle Designer for this object.');
                    return;
                }
                const selected = this.selectedEmitter(target.key, target.subject);
                if (!selected || !await this.validLinkedAddress(target.key, selected.subject) ||
                    snapshot.revision !== this.editor.snapshot()?.revision || this.targetIssue(target, this.editor.snapshot())) {
                    this.errorState.set('The emitter address changed. Reopen Particle Designer.');
                    return;
                }
                try {
                    const saved = await this.editor.execute({
                        type: 'replace_component',
                        key: target.key,
                        component: {
                            id: target.original.id,
                            schema_version: 1,
                            enabled: target.original.enabled,
                            data: {kind: 'particle_emitter', settings: candidate},
                        },
                    }, target.generation, snapshot.revision);
                    const updated = this.component(saved, target.key);
                    if (!updated || updated.id !== target.original.id) {
                        this.errorState.set('The emitter is no longer available. Reopen it to continue.');
                        return;
                    }
                    this.setDraft(updated.settings);
                    this.target.set({...target, original: updated});
                    this.announcementState.set('Particle settings applied. Use Save All to save the scene.');
                    this.closeReviewState.set(false);
                    applied = true;
                } catch (error: unknown) {
                    this.errorState.set(error instanceof Error ? error.message : String(error));
                }
            });
            if (!accepted) this.errorState.set(this.editing.error() ?? 'Finish the current edit and try again.');
        } finally {
            this.pendingState.set(false);
        }
        if (applied && close) this.discardAndClose();
    }

    private setDraft(settings: DraftTarget['original']['settings']): void {
        this.exactColor.set(settings.color);
        this.draft.set(draftFromSettings(settings));
    }

    private targetIssue(target: DraftTarget | null, snapshot: EditorSnapshot | null): string | null {
        if (!target) return null;
        if (target.generation !== this.editor.workspaceGeneration() || snapshot?.mode !== 'edit' ||
            snapshot.active_scene_id !== target.scene) {
            return 'The project or scene changed. Discard this draft and reopen Particle Designer.';
        }
        if (this.viewport.state()?.workspace_generation !== target.subject.scope.nativeGeneration ||
            snapshot.selection[0] !== target.key) {
            return 'The selected object or its workspace changed. Discard this draft and reopen Particle Designer.';
        }
        const component = this.component(snapshot, target.key);
        if (!component || component.id !== target.original.id) {
            return 'The emitter is no longer on this object. Discard this draft to close Particle Designer.';
        }
        if (component.enabled !== target.original.enabled ||
            !sameParticleSettings(component.settings, target.original.settings)) {
            return 'Particle settings changed while this window was open. Discard the draft and reopen the emitter.';
        }
        return null;
    }

    private component(snapshot: EditorSnapshot, key: string): ParticleEmitterComponent | null {
        return particleEmitter(snapshot.document.entities.find(entity => entity.key === key));
    }
}

function particleEmitter(entity: SceneEntity | undefined): ParticleEmitterComponent | null {
    if (entity?.particle_emitter) return entity.particle_emitter;
    const record = entity?.component_views?.find(component => component.data.kind === 'particle_emitter');
    return record?.data.kind === 'particle_emitter' ?
        {id: record.id, enabled: record.enabled, settings: record.data.settings} : null;
}

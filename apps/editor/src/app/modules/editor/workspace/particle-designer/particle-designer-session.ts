import {computed, inject, Injectable, signal} from '@angular/core';
import type {EditorSnapshot, ParticleEmitterComponent, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {DEFAULT_PARTICLE_EMITTER} from '../../../../core/editor/particle-emitter.models';
import {SceneEditingService} from '../scene-editing.service';
import {
    draftFromSettings,
    type ParticleDraft,
    PARTICLE_PRESETS,
    sameParticleSettings,
    settingsFromDraft,
} from './particle-designer.models';

interface DraftTarget {
    readonly generation: number;
    readonly scene: string;
    readonly key: string;
    readonly name: string;
    readonly original: ParticleEmitterComponent;
}

@Injectable()
export class ParticleDesignerSession {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly target = signal<DraftTarget | null>(null);
    private readonly exactColor = signal(DEFAULT_PARTICLE_EMITTER.color);

    readonly draft = signal<ParticleDraft>(draftFromSettings(DEFAULT_PARTICLE_EMITTER));
    readonly candidate = computed(() => settingsFromDraft(this.draft(), this.exactColor()));
    readonly active = computed(() => this.target() !== null);
    readonly objectName = computed(() => this.target()?.name ?? '');
    readonly pending = signal(false);
    readonly closeReview = signal(false);
    readonly error = signal<string | null>(null);
    readonly announcement = signal<string | null>(null);
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
        return candidate ? PARTICLE_PRESETS.find(item => sameParticleSettings(candidate, item.settings))?.label ?? '' : '';
    });

    open(key: string): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit' || this.active()) return false;
        const entity = snapshot.document.entities.find(item => item.key === key);
        const original = particleEmitter(entity);
        if (!entity || !original) return false;
        this.setDraft(original.settings);
        this.error.set(null);
        this.announcement.set(null);
        this.closeReview.set(false);
        this.target.set({
            generation: this.editor.workspaceGeneration(),
            scene: snapshot.active_scene_id,
            key,
            name: entity.name,
            original,
        });
        return true;
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
        if (preset) this.setDraft(preset.settings);
    }

    requestCloseReview(): void {
        if (this.active()) this.closeReview.set(true);
    }

    discardAndClose(): void {
        if (this.pending()) return;
        this.target.set(null);
        this.closeReview.set(false);
    }

    async apply(close = false): Promise<void> {
        const target = this.target();
        const candidate = this.candidate();
        if (!this.canApply() || !target || !candidate) return;
        this.pending.set(true);
        this.error.set(null);
        this.announcement.set(null);
        let applied = false;
        try {
            const accepted = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                const issue = this.targetIssue(target, snapshot);
                if (issue || !snapshot || this.target() !== target) {
                    this.error.set(issue ?? 'Reopen Particle Designer for this object.');
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
                        this.error.set('The emitter is no longer available. Reopen it to continue.');
                        return;
                    }
                    this.setDraft(updated.settings);
                    this.target.set({...target, original: updated});
                    this.announcement.set('Particle settings applied. Use Save All to save the scene.');
                    this.closeReview.set(false);
                    applied = true;
                } catch (error: unknown) {
                    this.error.set(error instanceof Error ? error.message : String(error));
                }
            });
            if (!accepted) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
        } finally {
            this.pending.set(false);
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

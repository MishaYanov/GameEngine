import {
    Component,
    computed,
    DestroyRef,
    effect,
    ElementRef,
    inject,
    input,
    signal,
    untracked,
    viewChild
} from '@angular/core';
import {DecimalPipe} from '@angular/common';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {PreviewDefinition} from '../../../../core/editor/camera-preview';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {SceneEditingService} from '../scene-editing.service';
import {sceneReferenceOptions} from '../scene-reference-options';
import {cameraFromDraft, cameraSignature, gameCameraDraft, GameCameraDraft} from './camera-draft';
import {orbitDraft} from "./orbit-camera";

@Component({
    selector: 'editor-camera-settings',
    imports: [EditorDialog, EditorSearchPicker, FormField, DecimalPipe],
    templateUrl: './camera-settings.html',
    styleUrl: './settings.scss',
})
export class CameraSettings {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly open = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    readonly previewing = signal(false);
    protected readonly fields = form(this.draft);
    protected readonly draft = signal<GameCameraDraft>({
        mode: 'fixed',
        follow: {
            mode: 'fixed', target: '', offset: {x: '0', y: '2', z: '5'},
            look_offset: {x: '0', y: '0', z: '0'}, smoothing: '0.15'
        },
        orbit: orbitDraft(null),
    });
    protected readonly focusTarget = signal(false);
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.editor.snapshot()?.document.entities ?? []));
    protected readonly orbitNumbers = [
        {key: 'yawDegrees', label: 'Initial yaw in degrees'},
        {key: 'pitchDegrees', label: 'Initial pitch in degrees'},
        {key: 'minimumPitchDegrees', label: 'Minimum pitch in degrees'},
        {key: 'maximumPitchDegrees', label: 'Maximum pitch in degrees'},
        {key: 'distance', label: 'Initial distance'},
        {key: 'minimumDistance', label: 'Minimum distance'},
        {key: 'maximumDistance', label: 'Maximum distance'},
        {key: 'followSeconds', label: 'Follow response in seconds'},
        {key: 'recoverySeconds', label: 'Outward recovery in seconds'},
        {key: 'teleportDistance', label: 'Teleport snap distance'},
        {key: 'probeRadius', label: 'Probe radius'},
        {key: 'obstructionMask', label: 'Obstacle layer mask'},
    ] as const;
    protected readonly groups = [{name: 'offset', label: 'Camera offset'}, {
        name: 'look_offset',
        label: 'Look offset'
    }] as const;
    protected readonly candidate = computed(() => {
        const snapshot = this.editor.snapshot();
        if (!snapshot) return {value: null, error: 'No scene is open.'};
        try {
            return {
                value: cameraFromDraft(snapshot.document.camera, this.draft(),
                    snapshot.document.entities.map(entity => entity.key)),
                error: null,
            };
        } catch (error: unknown) {
            return {value: null, error: error instanceof Error ? error.message : 'Invalid camera.'};
        }
    });
    protected readonly canRevealTarget = computed(() => {
        const base = this.editor.snapshot()?.document.camera;
        const candidate = this.candidate().value;
        return !!this.editing && this.available() && !!base && !!candidate &&
            this.draft().mode !== 'fixed' && cameraSignature(candidate) === cameraSignature(base);
    });
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    protected readonly nativeGeneration = computed(() => this.viewport?.state()?.workspace_generation ?? null);
    protected readonly axes = ['x', 'y', 'z'] as const;
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        this.editor.snapshot()?.mode === 'edit' && !this.opening() && !this.pending() &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    private readonly backButton = viewChild<ElementRef<HTMLButtonElement>>('backButton');
    private previewOwner: PreviewDefinition | null = null;
    private dialogSerial = 0;
    private applyOwner: number | null = null;
    private context: {
        serial: number;
        generation: number;
        sceneId: string;
        signature: string;
    } | null = null;

    constructor() {
        effect(() => {
            if ((!this.open() && !this.previewing()) || !this.context) return;
            const snapshot = this.editor.snapshot();
            if (this.context.generation !== this.editor.workspaceGeneration() ||
                this.context.sceneId !== snapshot?.active_scene_id || this.editor.playing()) {
                untracked(() => this.close());
            } else if (this.previewing() && this.previewOwner &&
                (this.previewOwner.expectedRevision !== snapshot.revision || snapshot.transaction || this.opening())) {
                untracked(() => this.backToSettings());
            }
        });
        inject(DestroyRef).onDestroy(() => this.close());
    }

    show(focusTarget = false, expectedMode?: 'follow' | 'orbit'): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || !this.available() || expectedMode && !snapshot.document.camera[expectedMode]) return false;
        this.stopPreview();
        const serial = ++this.dialogSerial;
        this.context = {
            serial,
            generation: this.editor.workspaceGeneration(),
            sceneId: snapshot.active_scene_id,
            signature: cameraSignature(snapshot.document.camera),
        };
        this.draft.set(
            gameCameraDraft(snapshot.document.camera,
                snapshot.selection[0] ?? snapshot.document.entities[0]?.key ?? '')
        );
        this.error.set(null);
        this.message.set(null);
        this.focusTarget.set(focusTarget);
        this.open.set(true);
        return true;
    }

    protected close(): void {
        this.stopPreview();
        ++this.dialogSerial;
        this.context = null;
        this.open.set(false);
        this.applyOwner = null;
        this.pending.set(false);
    }

    protected startPreview(): void {
        const snapshot = this.editor.snapshot(), candidate = this.candidate().value;
        const generation = this.nativeGeneration(), context = this.context;
        if (!snapshot || !candidate?.orbit || !context || generation === null || !this.open() || !this.available()) return;
        if (context.generation !== this.editor.workspaceGeneration() || context.sceneId !== snapshot.active_scene_id ||
            context.signature !== cameraSignature(snapshot.document.camera)) {
            this.error.set('Camera settings changed. Reopen the dialog to review them.');
            return;
        }
        this.previewOwner = {
            camera: candidate, sceneId: snapshot.active_scene_id,
            expectedRevision: snapshot.revision, expectedGeneration: generation
        };
        this.focusTarget.set(false);
        this.previewing.set(true);
        this.open.set(false);
        this.editor.cameraPreview.open(this.previewOwner);
    }

    protected backToSettings(): void {
        this.stopPreview();
        this.open.set(true);
    }

    protected focusPreviewControls(): void {
        if (this.previewing()) this.backButton()?.nativeElement.focus();
    }

    protected sample(yaw: number, pitch: number, zoom: number): void {
        this.editor.cameraPreview.sample({
            yaw_delta: yaw * Math.PI / 180,
            pitch_delta: pitch * Math.PI / 180, log_distance_delta: Math.log(zoom)
        });
    }

    protected async apply(): Promise<void> {
        const context = this.context;
        const candidate = this.candidate().value;
        if (!context || !candidate || !this.open() || !this.available())
            return;
        const current = () =>
            this.open() &&
            this.context === context &&
            context.serial === this.dialogSerial &&
            context.generation === this.editor.workspaceGeneration() &&
            context.sceneId === this.editor.snapshot()?.active_scene_id &&
            !this.editor.playing();

        this.applyOwner = context.serial;
        this.pending.set(true);
        this.error.set(null);
        try {
            const write = async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !current()) return;
                if (cameraSignature(snapshot.document.camera) !== context.signature) {
                    throw new Error('Camera settings changed. Reopen the dialog to review them.');
                }
                // Reparse after the edit coordinator: the target list may have changed.
                const next = cameraFromDraft(snapshot.document.camera, this.draft(),
                    snapshot.document.entities.map(entity => entity.key));
                if (cameraSignature(next) === context.signature) return;
                const accepted = await this.editor.execute(
                    {type: 'set_camera', camera: next},
                    context.generation,
                    snapshot.revision,
                );
                if (current()) {
                    context.signature = cameraSignature(accepted.document.camera);
                    this.draft.set(gameCameraDraft(accepted.document.camera));
                    this.message.set('Game camera applied. Save Scene to persist it.');
                }
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(write) && current()) {
                    this.error.set(this.editing.error() ?? 'Finish the current edit first.');
                }
            } else {
                await write();
            }
        } catch (error: unknown) {
            if (current()) {
                this.error.set(error instanceof Error ? error.message : 'Camera update failed.');
            }
        } finally {
            if (this.applyOwner === context.serial) {
                this.applyOwner = null;
                this.pending.set(false);
            }
        }
    }

    protected async revealTarget(): Promise<void> {
        const editing = this.editing;
        const context = this.context;
        if (!editing || !context || !this.canRevealTarget()) return;
        const target = this.draft().mode === 'orbit' ? this.draft().orbit.target : this.draft().follow.target;
        this.pending.set(true);
        this.error.set(null);
        const current = () => this.open() && this.context === context &&
            context.generation === this.editor.workspaceGeneration() &&
            context.sceneId === this.editor.snapshot()?.active_scene_id;
        try {
            await editing.select(target);
            if (current()) {
                if (this.editor.snapshot()?.selection[0] === target) this.close();
                else this.error.set(editing.error() ?? 'The target could not be selected.');
            }
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (this.context === context) this.pending.set(false);
        }
    }

    private stopPreview(): void {
        if (this.previewOwner) this.editor.cameraPreview.close(this.previewOwner);
        this.previewOwner = null;
        this.previewing.set(false);
    }
}

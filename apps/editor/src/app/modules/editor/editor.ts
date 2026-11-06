import {Component, computed, DestroyRef, inject, signal} from '@angular/core';
import {RouterOutlet} from "@angular/router";
import {EditorHeader} from "./components/editor-header/editor-header";
import {EditorWorkspace} from './workspace/editor-workspace';
import {EditorFooter} from "./components/editor-footer/editor-footer";
import {EditorPortal} from "./components/editor-portal/editor-portal";
import {EditorClientError, EditorService} from "../../core/editor/editor.service";

enum OpenMessages {
    RESTORING = 'Checking for the open project…',
    CREATING = 'Choosing a project…',
    DISCARD = 'Waiting for your discard decision…',
    OPENING = 'Opening project…'
}

@Component({
    selector: 'editor',
    imports: [
        RouterOutlet,
        EditorHeader,
        EditorWorkspace,
        EditorFooter,
        EditorPortal,
    ],
    templateUrl: './editor.html',
    styleUrl: './editor.scss',
})
export class Editor {
    protected readonly editorService = inject(EditorService);
    protected readonly destroyRef = inject(DestroyRef);
    unregisterFn = this.destroyRef.onDestroy(() => this.destroyCallBack());
    protected openMessage = signal<string | null>(null);
    protected openError = signal<string | null>(null);
    protected readonly opening = computed(() => this.openMessage() !== null);
    protected readonly canOpenProject = computed(() =>
        this.editorService.nativeAvailable &&
        !this.opening() &&
        !this.editorService.busy() &&
        !this.editorService.playing() &&
        !this.editorService.snapshot()?.transaction
    );
    private destroyed = false;

    async ngOnInit(): Promise<void> {
        if (!this.editorService.nativeAvailable || this.editorService.snapshot()) return;
        this.openMessage.set(OpenMessages.RESTORING);
        try {
            await this.editorService.restoreWorkspace();
        } catch (error: unknown) {
            if (!this.destroyed) this.openError.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.openMessage.set(null);
        }
    }

    protected async openProject(): Promise<void> {
        if (!this.canOpenProject()) return;

        this.openError.set(null);
        this.openMessage.set(OpenMessages.CREATING);

        try {
            const capabilities = await this.editorService.loadCapabilities();

            if (!capabilities.embedded_viewport_supported) {
                throw new Error(
                    'The native scene viewport is unavailable on this platform.',
                );
            }

            if (this.destroyed) return;

            const manifest = await this.editorService.chooseProjectManifest();

            if (manifest === null || this.destroyed) return;

            const reviewed = this.editorService.snapshot();

            if (reviewed?.mode === 'play') {
                throw new Error('Stop Play before opening another project.');
            }

            if (reviewed?.transaction) {
                throw new Error('Finish the current edit before opening another project.');
            }

            const discardUnsaved = reviewed?.dirty ?? false;

            if (discardUnsaved) {
                this.openMessage.set(OpenMessages.DISCARD);

                const confirmed = await this.editorService.confirmProjectDiscard();
                if (!confirmed || this.destroyed) return;
            }

            this.openMessage.set(OpenMessages.OPENING);

            await this.editorService.openProject(manifest, {
                expectedRevision: reviewed?.revision ?? null,
                discardUnsaved,
            });
        } catch (error: unknown) {
            if (this.destroyed) return;

            if (error instanceof EditorClientError && error.kind === 'conflict') {
                try {
                    await this.editorService.refresh();
                } catch {
                    // Preserve the last known snapshot if refresh fails.
                }
            }

            if (!this.destroyed) {
                this.openError.set(
                    error instanceof Error ? error.message : String(error),
                );
            }
        } finally {
            this.openMessage.set(null);
        }
    }

    private destroyCallBack() {
        this.destroyed = true;
    }
}

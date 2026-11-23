import {Component, computed, DestroyRef, inject, signal, viewChild} from '@angular/core';
import {RouterOutlet} from "@angular/router";
import {EditorHeader} from "./components/editor-header/editor-header";
import {EditorWorkspace} from './workspace/editor-workspace';
import {EditorFooter} from "./components/editor-footer/editor-footer";
import {EditorPortal} from "./components/editor-portal/editor-portal";
import {EditorClientError, EditorService} from "../../core/editor/editor.service";
import {EditorSettingsService} from '../../core/editor/settings/editor-settings.service';
import {EditorSettingsDialog} from '../editor-settings/editor-settings-dialog';
import {EditorContextMenu} from '../../shared/components/context-menu/context-menu';
import {watchDesktopClose} from './desktop-close-review';

enum OpenMessages {
    RESTORING = 'Checking for the open project…',
    CREATING = 'Choosing a project…',
    DISCARD = 'Waiting for your discard decision…',
    OPENING = 'Opening project…',
    EXITING = 'Closing the editor…',
}

@Component({
    selector: 'editor',
    imports: [
        RouterOutlet,
        EditorHeader,
        EditorWorkspace,
        EditorFooter,
        EditorPortal,
        EditorSettingsDialog,
        EditorContextMenu,
    ],
    templateUrl: './editor.html',
    styleUrl: './editor.scss',
})
export class Editor {
    protected readonly editorService = inject(EditorService);
    protected readonly settings = inject(EditorSettingsService);
    protected readonly destroyRef = inject(DestroyRef);
    unregisterFn = this.destroyRef.onDestroy(() => this.destroyCallBack());
    protected openMessage = signal<string | null>(null);
    protected openError = signal<string | null>(null);
    protected readonly opening = computed(() => this.openMessage() !== null);
    private readonly reviewingDeparture = signal(false);
    private readonly idle = computed(() =>
        this.editorService.nativeAvailable &&
        !this.opening() &&
        !this.editorService.busy() &&
        !this.editorService.snapshot()?.transaction
    );
    protected readonly canExit = computed(() => this.idle() && !this.reviewingDeparture());
    protected readonly canOpenProject = computed(() => this.canExit() && !this.editorService.playing());
    private readonly workspace = viewChild(EditorWorkspace);
    private stopWatchingClose?: () => void;
    private destroyed = false;

    async ngOnInit(): Promise<void> {
        if (!this.editorService.nativeAvailable) return;
        this.stopWatchingClose = watchDesktopClose(
            () => void this.exitEditor(true),
            () => this.openError.set('The title-bar close review is unavailable. Use File → Exit to close the editor.'),
        );
        void this.editorService.loadCapabilities().catch(() => undefined);
        if (this.editorService.snapshot()) return;
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
        await this.withReviewedWorkspace('open', () => this.openSelectedProject());
    }

    private async openSelectedProject(): Promise<void> {
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

    protected async exitEditor(fromTitleBar = false): Promise<void> {
        if (this.destroyed || this.reviewingDeparture()) return;
        if (!this.idle()) {
            if (fromTitleBar && this.openMessage() !== OpenMessages.EXITING) {
                this.openError.set(this.editorService.snapshot()?.transaction
                    ? 'Finish the current edit before closing the editor.'
                    : 'Wait for the current editor operation to finish, then close again.');
            }
            return;
        }
        await this.withReviewedWorkspace('exit', async () => {
            this.openError.set(null);
            this.openMessage.set(OpenMessages.EXITING);
            let closing = false;
            try {
                closing = await this.editorService.requestExit();
            } finally {
                if (!closing) this.openMessage.set(null);
            }
        });
    }

    reviewRouteDeparture(): Promise<boolean> {
        if (!this.editorService.nativeAvailable) return Promise.resolve(true);
        return this.withReviewedWorkspace('open', () => undefined);
    }

    private async withReviewedWorkspace(intent: 'open' | 'exit', proceed: () => void | Promise<void>): Promise<boolean> {
        if (this.destroyed || !(intent === 'open' ? this.canOpenProject() : this.canExit())) return false;
        this.reviewingDeparture.set(true);
        try {
            const workspace = this.workspace();
            const generation = this.editorService.workspaceGeneration();
            if (workspace && !await workspace.reviewProjectNavigation()) return false;
            if (this.destroyed || !this.idle() || (intent === 'open' && this.editorService.playing()) ||
                generation !== this.editorService.workspaceGeneration()) return false;
            await proceed();
            return true;
        } catch (error: unknown) {
            if (!this.destroyed) this.openError.set(error instanceof Error ? error.message : String(error));
            return false;
        } finally {
            this.reviewingDeparture.set(false);
        }
    }

    private destroyCallBack() {
        this.destroyed = true;
        this.stopWatchingClose?.();
        this.settings.setOpen(false);
    }
}

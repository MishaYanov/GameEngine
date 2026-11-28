import {Component, computed, DestroyRef, ElementRef, inject, signal, viewChild} from '@angular/core';
import {Router, RouterOutlet} from "@angular/router";
import type {ProjectTemplate} from "../../core/project/project.service";
import {EditorHeader} from "./components/editor-header/editor-header";
import {EditorWorkspace} from './workspace/editor-workspace';
import {EditorFooter} from "./components/editor-footer/editor-footer";
import {EditorPortal} from "./components/editor-portal/editor-portal";
import {EditorClientError, EditorService} from "../../core/editor/editor.service";
import {EditorSettingsService} from '../../core/editor/settings/editor-settings.service';
import {EditorSettingsDialog} from '../editor-settings/editor-settings-dialog';
import {EditorContextMenu} from '../../shared/components/context-menu/context-menu';
import {watchDesktopClose} from './desktop-close-review';
import {EditorDocumentRecovery} from './document-recovery/document-recovery';
import type {NavigationDirection} from './workspace/workspace-navigation';

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
        EditorDocumentRecovery,
    ],
    templateUrl: './editor.html',
    styleUrl: './editor.scss',
    host: {
        '(window:keydown)': 'handleNavigationKey($event)',
        '(window:pointerdown)': 'preventMouseNavigation($event)',
        '(window:mouseup)': 'preventMouseNavigation($event)',
        '(window:auxclick)': 'handleNavigationClick($event)',
    },
})
export class Editor {
    protected readonly editorService = inject(EditorService);
    private readonly router = inject(Router);
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
    private readonly workspaceElement = viewChild<EditorWorkspace, ElementRef<HTMLElement>>(EditorWorkspace, {read: ElementRef});
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
        if (this.destroyed || !this.canOpenProject()) return;
        this.openError.set(null);
        this.openMessage.set(OpenMessages.CREATING);
        const generation = this.editorService.workspaceGeneration();
        const sceneId = this.editorService.snapshot()?.active_scene_id;
        let manifest: string | null;
        try {
            const capabilities = await this.editorService.loadCapabilities();
            if (!capabilities.embedded_viewport_supported) {
                throw new Error('The native scene viewport is unavailable on this platform.');
            }
            if (this.destroyed) return;
            manifest = await this.editorService.chooseProjectManifest();
        } catch (error: unknown) {
            if (!this.destroyed) this.openError.set(error instanceof Error ? error.message : String(error));
            return;
        } finally {
            this.openMessage.set(null);
        }
        if (manifest === null || generation !== this.editorService.workspaceGeneration() ||
            sceneId !== this.editorService.snapshot()?.active_scene_id) return;
        await this.withReviewedWorkspace('open', () => this.openSelectedProject(manifest));
    }

    protected async openProjectSetup(template: ProjectTemplate): Promise<void> {
        if (this.destroyed || !this.canOpenProject()) return;
        try {
            // The route's canDeactivate guard reviews the workspace for both setup entries.
            await this.router.navigate(['/setup'], {
                queryParams: {from: 'editor', template}, replaceUrl: this.editorService.nativeAvailable,
            });
        } catch (error: unknown) {
            if (!this.destroyed) this.openError.set(error instanceof Error ? error.message : String(error));
        }
    }

    private async openSelectedProject(manifest: string): Promise<void> {
        try {
            const generation = this.editorService.workspaceGeneration();
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
                if (!confirmed || this.destroyed || generation !== this.editorService.workspaceGeneration() ||
                    reviewed?.active_scene_id !== this.editorService.snapshot()?.active_scene_id) return;
            }

            this.openMessage.set(OpenMessages.OPENING);

            await this.editorService.openProject(manifest, {
                expectedGeneration: generation,
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
            const workspace = this.workspaceElement()?.nativeElement;
            const focused = workspace?.ownerDocument.activeElement;
            // Close the input window before the native request can run, even before Angular renders the closing status.
            workspace?.setAttribute('inert', '');
            this.openMessage.set(OpenMessages.EXITING);
            let closing = false;
            try {
                closing = await this.editorService.requestExit(
                    this.editorService.workspaceGeneration(), this.editorService.snapshot()?.revision ?? null,
                );
            } finally {
                if (!closing) {
                    workspace?.removeAttribute('inert');
                    this.openMessage.set(null);
                    if (focused instanceof HTMLElement && workspace?.contains(focused) &&
                        workspace.ownerDocument.activeElement === workspace.ownerDocument.body && !this.destroyed) {
                        focused.focus({preventScroll: true});
                    }
                }
            }
        });
    }

    reviewRouteDeparture(browserHistory = false): Promise<boolean> {
        if (!this.editorService.nativeAvailable) return Promise.resolve(true);
        if (browserHistory) return Promise.resolve(false);
        return this.withReviewedWorkspace('open', () => undefined);
    }

    protected handleNavigationKey(event: KeyboardEvent): void {
        if (!this.editorService.nativeAvailable || event.defaultPrevented || event.isComposing ||
            event.ctrlKey || event.metaKey || event.shiftKey) return;
        const back = event.key === 'BrowserBack' || event.altKey && event.key === 'ArrowLeft';
        const forward = event.key === 'BrowserForward' || event.altKey && event.key === 'ArrowRight';
        if (!back && !forward) return;
        event.preventDefault();
        if (!event.repeat) this.navigateWorkspace(back ? 'back' : 'forward');
    }

    protected preventMouseNavigation(event: MouseEvent): void {
        if (this.editorService.nativeAvailable && (event.button === 3 || event.button === 4)) event.preventDefault();
    }

    protected handleNavigationClick(event: MouseEvent): void {
        if (!this.editorService.nativeAvailable || (event.button !== 3 && event.button !== 4)) return;
        event.preventDefault();
        this.navigateWorkspace(event.button === 3 ? 'back' : 'forward');
    }

    protected navigateWorkspace(direction: NavigationDirection): void {
        void this.workspace()?.navigateHistory(direction);
    }

    private async withReviewedWorkspace(intent: 'open' | 'exit', proceed: () => void | Promise<void>): Promise<boolean> {
        if (this.destroyed || !(intent === 'open' ? this.canOpenProject() : this.canExit())) return false;
        this.reviewingDeparture.set(true);
        try {
            const workspace = this.workspace();
            const generation = this.editorService.workspaceGeneration();
            const sceneId = this.editorService.snapshot()?.active_scene_id;
            if (workspace && !await workspace.reviewProjectNavigation()) return false;
            if (this.destroyed || !this.idle() || (intent === 'open' && this.editorService.playing()) ||
                generation !== this.editorService.workspaceGeneration() ||
                sceneId !== this.editorService.snapshot()?.active_scene_id) return false;
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

import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    signal,
    viewChild,
    viewChildren,
} from '@angular/core';
import {applyEach, form, FormField, maxLength} from '@angular/forms/signals';
import type {
    CampaignDiagnosticReport,
    CampaignRepairPreview,
    CampaignRepairTarget,
    SceneId
} from '../../../../core/editor/editor.models';
import {EditorClientError, EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {SceneSwitchService} from '../scene-actions/scene-switch.service';
import {CampaignDiagnostics} from './campaign-diagnostics';
import {CampaignRepair} from './campaign-repair';
import {CampaignRepairQueue} from './campaign-repair-queue';
import {
    campaignDefinition,
    type CampaignDraft,
    type ChapterDraft,
    configuredScenes,
    validateCampaignDraft
} from './campaign-draft';
import {type CourtyardSettings, MAX_CAMPAIGN_CHAPTERS, validChapterId} from './courtyard-settings';

interface ReviewedCampaign {
    readonly generation: number;
    readonly nativeGeneration: number;
    readonly revision: number;
    readonly signature: string;
}

interface DiagnosticRequest {
    readonly context: ReviewedCampaign;
    readonly draft: CampaignDraft;
}

interface CheckedCampaign {
    readonly request: DiagnosticRequest;
    readonly report: CampaignDiagnosticReport;
}

type SceneInspection = {
    readonly sceneId: SceneId;
    readonly context: ReviewedCampaign;
} & ({ readonly source: 'diagnostics'; readonly check: CheckedCampaign } |
    { readonly source: 'queue'; readonly draft: CampaignDraft });

interface ReviewedRepair {
    readonly check: CheckedCampaign;
    readonly sceneId: SceneId;
    readonly rows: readonly CampaignRepairPreview[];
}

@Component({
    selector: 'editor-campaign-settings',
    imports: [CampaignDiagnostics, CampaignRepair, EditorDialog, FormField],
    templateUrl: './campaign-settings.html',
    styleUrls: ['./settings.scss', './campaign-settings.scss'],
})
export class CampaignSettings {
    readonly opening = input(false);
    readonly settings = input.required<CourtyardSettings | null>();

    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly sceneSwitch = inject(SceneSwitchService, {optional: true});
    protected readonly repairQueue = inject(CampaignRepairQueue);
    private readonly injector = inject(Injector);
    private readonly destroyRef = inject(DestroyRef);
    private readonly chapterInputs = viewChildren<ElementRef<HTMLInputElement>>('chapterId');
    private readonly finishSelect = viewChild<ElementRef<HTMLSelectElement>>('finishSelect');
    private readonly failureMessage = viewChild<ElementRef<HTMLElement>>('failureMessage');
    private readonly checkButton = viewChild<ElementRef<HTMLButtonElement>>('checkButton');
    private readonly keepDraft = viewChild<ElementRef<HTMLButtonElement>>('keepDraft');
    private readonly context = signal<ReviewedCampaign | null>(null);
    private readonly checkingRequest = signal<DiagnosticRequest | null>(null);
    private readonly checked = signal<CheckedCampaign | null>(null);
    private readonly inspection = signal<SceneInspection | null>(null);
    private readonly repair = signal<ReviewedRepair | null>(null);
    private inspectAfterClose: SceneInspection | null = null;
    private nextRow = 0;

    protected readonly open = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    protected readonly draft = signal<CampaignDraft>({chapters: [], finish_scene: ''});
    protected readonly fields = form(this.draft, field => applyEach(field.chapters, chapter => {
        maxLength(chapter.id, 32);
        maxLength(chapter.label, 48);
    }));
    protected readonly courses = computed(() =>
        configuredScenes(this.settings(), this.editor.snapshot()?.scenes ?? [], 'course'));
    protected readonly finishes = computed(() =>
        configuredScenes(this.settings(), this.editor.snapshot()?.scenes ?? [], 'finish'));
    private readonly editable = computed(() =>
        this.editor.nativeAvailable && this.settings() !== null && !this.opening() && this.editor.snapshot()?.mode === 'edit');
    protected readonly available = computed(() => this.editable() && !this.pending() &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    protected readonly canAdd = computed(() => this.draft().chapters.length < MAX_CAMPAIGN_CHAPTERS &&
        this.courses().some(scene => !this.draft().chapters.some(chapter => chapter.course_scene === scene.id)));
    protected readonly validation = computed(() => validateCampaignDraft(this.draft(), this.courses(), this.finishes()));
    protected readonly dirty = computed(() =>
        JSON.stringify(campaignDefinition(this.draft())) !== this.context()?.signature);
    protected readonly currentStartup = computed(() => this.editor.snapshot()?.project.startup_scene_id);
    protected readonly chosenStartup = computed(() => this.draft().chapters[0]?.course_scene);
    protected readonly checking = computed(() => this.checkingRequest() !== null);
    protected readonly diagnostics = computed(() => {
        const checked = this.checked();
        return checked && this.currentDiagnosticRequest(checked.request) ? checked : null;
    });
    protected readonly activeScene = computed(() => this.editor.snapshot()?.active_scene_id);
    protected readonly sceneCatalog = computed(() => this.editor.snapshot()?.scenes ?? []);
    protected readonly canInspect = computed(() => this.sceneSwitch?.canSwitch() ?? false);
    protected readonly inspectChoice = computed(() => {
        const next = this.inspection();
        if (!next || next.context !== this.context()) return null;
        return (next.source === 'queue'
            ? next.draft === this.draft() && next.context.revision === this.editor.snapshot()?.revision &&
            this.repairQueue.sceneIds().includes(next.sceneId)
            : next.check === this.diagnostics()) ? next : null;
    });
    protected readonly queueNeedsCheck = computed(() => this.repairQueue.currentCheck()?.revision !==
        this.editor.snapshot()?.revision || this.diagnostics() === null);
    protected readonly repairChoice = computed(() => {
        const review = this.repair();
        return review && review.check === this.diagnostics() &&
        review.sceneId === this.activeScene() ? review : null;
    });
    protected readonly canOpenQueue = computed(() =>
        this.available() && this.canInspect() && !this.checking() && !this.repairChoice() && !this.inspectChoice() &&
        this.context()?.revision === this.editor.snapshot()?.revision);

    constructor() {
        effect(() => {
            const context = this.context();
            if (this.open() && context && !this.currentWorkspace(context.generation)) {
                this.open.set(false);
            } else if (this.open() && this.repair() && !this.repairChoice() && !this.pending()) {
                this.repair.set(null);
                this.showFailure('The scene changed. Reopen Campaign Settings to review the current revision.');
            }
        });
    }

    protected async show(): Promise<void> {
        if (!this.available() || this.open()) return;
        const generation = this.editor.workspaceGeneration();
        this.pending.set(true);
        this.error.set(null);
        this.message.set(null);
        try {
            const openReviewed = async () => {
                const snapshot = this.editor.snapshot();
                const settings = this.settings();
                if (!snapshot || !settings || !this.currentWorkspace(generation)) return;
                const {revision} = snapshot;
                const nativeGeneration = await this.editor.campaignGeneration(generation, revision);
                if (!this.currentWorkspace(generation) || this.editor.snapshot()?.revision !== revision) return;
                this.setDraft(settings, generation, nativeGeneration, revision);
                this.open.set(true);
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(openReviewed) && this.currentWorkspace(generation)) {
                    this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            } else await openReviewed();
        } catch (cause) {
            if (this.currentWorkspace(generation)) this.error.set(this.errorText(cause));
        } finally {
            if (!this.destroyRef.destroyed) this.pending.set(false);
        }
    }

    protected move(index: number, direction: -1 | 1): void {
        const next = index + direction;
        const rows = [...this.draft().chapters];
        if (next < 0 || next >= rows.length || !rows[index]) return;
        const [row] = rows.splice(index, 1);
        rows.splice(next, 0, row);
        this.draft.update(draft => ({...draft, chapters: rows}));
        this.message.set('Chapter moved to position ' + (next + 1) + '.');
        this.focusChapter(next);
    }

    protected add(): void {
        if (!this.canAdd()) return;
        const existing = this.draft().chapters;
        const scene = this.courses().find(course => !existing.some(chapter => chapter.course_scene === course.id));
        if (!scene) return;
        this.draft.update(draft => ({...draft, chapters: [...existing, this.newChapter(scene.id, existing)]}));
        this.focusChapter(existing.length);
    }

    protected remove(index: number): void {
        const rows = this.draft().chapters.filter((_, row) => row !== index);
        this.draft.update(draft => ({...draft, chapters: rows}));
        this.focusChapter(Math.min(index, rows.length - 1));
    }

    protected selectedElsewhere(sceneId: SceneId, index: number): boolean {
        return this.draft().chapters.some((chapter, row) => row !== index && chapter.course_scene === sceneId);
    }

    protected sceneLabel(sceneId: SceneId): string {
        const scene = this.editor.snapshot()?.scenes.find(scene => scene.id === sceneId);
        return scene ? scene.asset_path + ' (' + scene.id + ')' : sceneId;
    }

    protected async checkCampaign(preserveMessage = false): Promise<void> {
        const context = this.context();
        if (!context || !this.open() || !this.available() || this.checking() || this.repairChoice() ||
            this.validation()) return;
        const request = {context, draft: this.draft()};
        if (!this.currentDiagnosticRequest(request)) {
            this.showFailure('The workspace changed. Reopen Campaign Settings to check the current revision.');
            return;
        }
        this.checkingRequest.set(request);
        this.checked.set(null);
        this.inspection.set(null);
        this.repair.set(null);
        this.error.set(null);
        if (!preserveMessage) this.message.set(null);
        try {
            const definition = campaignDefinition(request.draft);
            const report = await this.editor.diagnoseCampaign(
                definition,
                context.generation,
                context.nativeGeneration,
                context.revision,
            );
            if (this.checkingRequest() === request && this.currentDiagnosticRequest(request) &&
                report.workspace_generation === context.nativeGeneration && report.revision === context.revision) {
                this.checked.set({request, report});
                this.repairQueue.accept(context.generation, definition, report);
            }
        } catch (cause) {
            if (this.checkingRequest() === request && this.currentDiagnosticRequest(request)) {
                this.showFailure(cause);
            }
        } finally {
            if (!this.destroyRef.destroyed && this.checkingRequest() === request) this.checkingRequest.set(null);
        }
    }

    protected async reviewRepair(targets: readonly CampaignRepairTarget[]): Promise<void> {
        const check = this.diagnostics();
        const [first] = targets;
        if (!first || !check || !this.available() || this.repairChoice() || this.inspectChoice() ||
            first.scene_id !== this.activeScene() ||
            !check.report.issues.some(issue => issue.repair_batch === targets ||
                (targets.length === 1 && issue.repair === first))) return;
        const {generation, nativeGeneration, revision} = check.request.context;
        const sceneId = first.scene_id;
        this.pending.set(true);
        this.error.set(null);
        this.message.set(null);
        try {
            let rows: readonly CampaignRepairPreview[];
            if (targets.length === 1) {
                rows = [await this.editor.previewCampaignRepair(first, generation, nativeGeneration, revision)];
            } else {
                const batch = await this.editor.previewCampaignRepairBatch(
                    sceneId, targets, generation, nativeGeneration, revision);
                if (batch.scene_id !== sceneId || batch.workspace_generation !== nativeGeneration ||
                    batch.revision !== revision) {
                    throw new Error('The repair preview is out of date. Check the campaign again.');
                }
                rows = batch.repairs;
            }
            if (this.diagnostics() !== check || this.activeScene() !== sceneId) return;
            if (rows.length !== targets.length || !targets.every((target, index) => {
                const returned = rows[index].target;
                return returned.scene_id === target.scene_id && returned.role === target.role &&
                    returned.entity_key === target.entity_key;
            })) {
                this.showFailure('The repair preview does not match the reviewed triggers. Check the campaign again.');
                return;
            }
            this.repair.set({check, sceneId, rows});
        } catch (cause) {
            if (this.diagnostics() === check && this.activeScene() === sceneId) this.showFailure(cause);
        } finally {
            if (!this.destroyRef.destroyed) this.pending.set(false);
        }
    }

    protected cancelRepair(): void {
        if (!this.repairChoice() || this.pending()) return;
        this.repair.set(null);
        this.focusWhileOpen(() => this.checkButton()?.nativeElement);
    }

    protected async applyRepair(): Promise<void> {
        const repair = this.repairChoice();
        if (!repair || !this.available()) return;
        const context = repair.check.request.context;
        const {generation, nativeGeneration, revision} = context;
        this.pending.set(true);
        this.error.set(null);
        const current = () => this.open() && this.context() === context && this.currentWorkspace(generation);
        let recheck = false;
        let revisedContext: ReviewedCampaign | null = null;
        try {
            const [first] = repair.rows;
            const single = repair.rows.length === 1;
            const updated = single
                ? await this.editor.applyCampaignRepair(first, generation, nativeGeneration, revision)
                : await this.editor.applyCampaignRepairBatch(repair.sceneId, repair.rows, generation, nativeGeneration, revision);
            if (current()) {
                recheck = this.repairQueue.sceneIds().includes(repair.sceneId);
                revisedContext = {...context, revision: updated.revision};
                this.context.set(revisedContext);
                this.checked.set(null);
                this.message.set(single ? 'Trigger repaired. Save All to persist the scene.'
                    : 'Triggers repaired. Save All to persist the scene.');
            }
        } catch (cause) {
            if (current()) this.showFailure(cause);
        } finally {
            if (!this.destroyRef.destroyed) {
                this.repair.set(null);
                this.pending.set(false);
            }
        }
        if (recheck) await this.checkCampaign(true);
        if (revisedContext && this.context() === revisedContext && !this.error()) {
            this.focusWhileOpen(() => this.checkButton()?.nativeElement);
        }
    }

    protected inspectScene(sceneId: SceneId): void {
        const check = this.diagnostics();
        if (!check || this.pending() || this.repairChoice() || !this.canInspectScene(sceneId) ||
            !check.report.issues.some(issue => issue.scene_id === sceneId)) return;
        this.chooseInspection({source: 'diagnostics', check, context: check.request.context, sceneId});
    }

    protected openQueuedScene(sceneId: SceneId): void {
        const context = this.context();
        if (!context || !this.canOpenQueue() || !this.repairQueue.sceneIds().includes(sceneId) ||
            !this.canInspectScene(sceneId)) return;
        this.chooseInspection({source: 'queue', draft: this.draft(), context, sceneId});
    }

    private chooseInspection(next: SceneInspection): void {
        if (this.dirty()) {
            this.inspection.set(next);
            this.focusWhileOpen(() => this.keepDraft()?.nativeElement);
        } else this.closeForInspection(next);
    }

    protected cancelInspection(): void {
        this.inspection.set(null);
        this.focusWhileOpen(() => this.checkButton()?.nativeElement);
    }

    protected confirmInspection(): void {
        const next = this.inspectChoice();
        if (next && this.canInspectScene(next.sceneId)) this.closeForInspection(next);
    }

    protected async dialogClosed(): Promise<void> {
        const next = this.inspectAfterClose;
        this.inspectAfterClose = null;
        this.inspection.set(null);
        this.repair.set(null);
        this.checked.set(null);
        this.checkingRequest.set(null);
        this.context.set(null);
        if (!next || !this.currentWorkspace(next.context.generation) ||
            this.editor.snapshot()?.revision !== next.context.revision || !this.canInspectScene(next.sceneId)) return;
        if (next.source === 'diagnostics') {
            await this.sceneSwitch?.request(next.sceneId);
            return;
        }
        this.pending.set(true);
        const switched = await this.sceneSwitch?.request(next.sceneId);
        if (this.destroyRef.destroyed) return;
        this.pending.set(false);
        // Let the workspace deliver its updated scene-switch pending input before reopening a dialog.
        afterNextRender(() => void this.resumeQueuedInspection(next, switched === true), {injector: this.injector});
    }

    private async resumeQueuedInspection(next: Extract<SceneInspection, {
        source: 'queue'
    }>, switched: boolean): Promise<void> {
        if (this.open() || !this.currentWorkspace(next.context.generation)) return;
        if (switched && this.activeScene() === next.sceneId) {
            await this.show();
            await this.checkCampaign();
        } else if (this.editor.snapshot()?.revision === next.context.revision) {
            this.context.set(next.context);
            this.draft.set(next.draft);
            this.open.set(true);
            this.focusWhileOpen(() => this.checkButton()?.nativeElement);
        }
    }

    protected async apply(): Promise<void> {
        const context = this.context();
        if (!context || !this.open() || !this.available() || this.checking() || this.repairChoice() ||
            this.validation() || !this.dirty()) return;
        const definition = campaignDefinition(this.draft());
        this.pending.set(true);
        this.error.set(null);
        this.message.set(null);
        const current = () => this.open() && this.context() === context && this.currentWorkspace(context.generation);
        try {
            const commit = async () => {
                if (!current()) return;
                try {
                    const result = await this.editor.applyCampaign(definition, context.generation, context.nativeGeneration, context.revision);
                    if (current()) {
                        this.context.set({
                            ...context,
                            revision: result.revision,
                            signature: JSON.stringify(definition)
                        });
                        this.message.set('Campaign settings applied. Save All to persist the project.');
                    }
                } catch (cause) {
                    if (current()) this.showFailure(cause);
                }
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(commit) && current()) {
                    this.showFailure(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            } else await commit();
        } finally {
            if (!this.destroyRef.destroyed) this.pending.set(false);
        }
    }

    private currentDiagnosticRequest(request: DiagnosticRequest): boolean {
        return this.open() && this.context() === request.context && this.draft() === request.draft &&
            this.currentWorkspace(request.context.generation) && this.editor.snapshot()?.revision === request.context.revision;
    }

    private canInspectScene(sceneId: SceneId): boolean {
        return this.canInspect() && sceneId !== this.activeScene() && this.sceneCatalog().some(scene => scene.id === sceneId);
    }

    private closeForInspection(next: SceneInspection): void {
        this.inspectAfterClose = next;
        this.open.set(false);
    }

    private currentWorkspace(generation: number): boolean {
        return !this.destroyRef.destroyed && this.editable() && this.editor.workspaceGeneration() === generation;
    }

    private setDraft(settings: CourtyardSettings, generation: number, nativeGeneration: number, revision: number): void {
        const chapters: ChapterDraft[] = [];
        const campaign = settings.campaign;
        if (campaign) {
            for (const chapter of campaign.chapters) chapters.push({...chapter, row: this.nextRow++});
        } else {
            const startup = this.currentStartup();
            const courses = this.courses();
            const first = courses.find(scene => scene.id === startup);
            const ordered = first ? [first, ...courses.filter(scene => scene !== first)] : courses;
            for (const scene of ordered.slice(0, MAX_CAMPAIGN_CHAPTERS)) {
                chapters.push(this.newChapter(scene.id, chapters));
            }
        }
        this.draft.set({chapters, finish_scene: campaign?.finish_scene ?? this.finishes()[0]?.id ?? ''});
        this.context.set({
            generation, nativeGeneration, revision,
            signature: campaign ? JSON.stringify(campaignDefinition(this.draft())) : '',
        });
    }

    private newChapter(scene: SceneId, prior: readonly ChapterDraft[]): ChapterDraft {
        const row = this.nextRow++;
        const ordinal = prior.length + 1;
        const preferred = validChapterId(scene) ? scene : 'chapter-' + ordinal;
        let id = preferred;
        for (let suffix = ordinal; prior.some(chapter => chapter.id === id); suffix++) id = 'chapter-' + suffix;
        return {row, id, label: 'Chapter ' + ordinal, course_scene: scene};
    }

    private focusChapter(index: number): void {
        this.focusWhileOpen(() =>
            this.chapterInputs()[index]?.nativeElement ?? this.finishSelect()?.nativeElement);
    }

    private focusWhileOpen(target: () => HTMLElement | undefined): void {
        const context = this.context();
        if (!context) return;
        afterNextRender(() => {
            if (this.open() && this.context() === context && this.currentWorkspace(context.generation)) {
                target()?.focus();
            }
        }, {injector: this.injector});
    }

    private showFailure(cause: unknown): void {
        this.error.set(this.errorText(cause));
        const details = cause instanceof EditorClientError ? cause.cause : null;
        const scene = typeof details === 'object' && details !== null && 'scene_id' in details ? details.scene_id : null;
        const index = this.draft().chapters.findIndex(chapter => chapter.course_scene === scene);
        if (index >= 0 || scene === this.draft().finish_scene) {
            this.focusChapter(index);
        } else {
            this.focusWhileOpen(() => this.failureMessage()?.nativeElement);
        }
    }

    private errorText(cause: unknown): string {
        return cause instanceof Error ? cause.message : String(cause);
    }
}

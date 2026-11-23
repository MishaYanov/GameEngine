import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    ElementRef,
    inject,
    Injector,
    input,
    linkedSignal,
    output,
    signal,
    viewChild,
} from '@angular/core';
import type {EditorSnapshot, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {
    SceneDependencyAction, SceneDependencyRequest, SceneIssue, SceneIssueScope, SceneVerificationReport,
} from '../../../../core/editor/scene-verification.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {SceneSwitchService} from './scene-switch.service';

const issueScopes = [
    {id: 'all', label: 'All issues'},
    {id: 'intro', label: 'Intro'},
    {id: 'arena', label: 'Arena'},
    {id: 'scene', label: 'Other scenes'},
    {id: 'project', label: 'Project'},
] as const satisfies readonly { id: 'all' | SceneIssueScope; label: string }[];

interface AcceptedReport {
    readonly owner: number;
    readonly report: SceneVerificationReport;
}

interface Visit {
    readonly stamp: string;
    readonly passed: boolean;
}

@Component({
    selector: 'editor-scene-verification',
    templateUrl: './scene-verification.html',
    styleUrl: './scene-verification.scss',
})
export class SceneVerification {
    readonly open = input.required<boolean>();
    readonly available = input.required<boolean>();
    readonly inspectRole = output<string>();
    readonly inspectDependency = output<SceneDependencyRequest>();
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly sceneSwitch = inject(SceneSwitchService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly injector = inject(Injector);
    private readonly destroy = inject(DestroyRef);
    private readonly body = viewChild<ElementRef<HTMLElement>>('body');
    private readonly generation = this.editor.workspaceGeneration;
    private readonly nativeGeneration = computed(() => this.viewport.state()?.workspace_generation ?? null);
    private readonly accepted = linkedSignal({source: this.generation, computation: (): AcceptedReport | null => null});
    private readonly visits = linkedSignal({
        source: this.generation,
        computation: (): ReadonlyMap<SceneId, Visit> => new Map(),
    });
    private readonly preview = linkedSignal({
        source: () => this.report(),
        computation: (): SceneIssue | null => null,
    });
    protected readonly error = linkedSignal({
        source: this.generation,
        computation: (): string | null => null,
    });
    protected readonly busy = signal(false);
    protected readonly checkedBefore = computed(() => this.accepted() !== null);
    protected readonly canCheck = computed(() => this.open() && this.available() && !this.busy() &&
        !this.editor.busy() && this.editor.snapshot()?.transaction === null);
    protected readonly report = computed(() => {
        const accepted = this.accepted();
        const snapshot = this.editor.snapshot();
        return accepted && this.matches(accepted, snapshot) ? accepted.report : null;
    });
    protected readonly scopes = issueScopes;
    protected readonly scope = linkedSignal({
        source: this.generation,
        computation: (): typeof issueScopes[number]['id'] => 'all',
    });
    protected readonly actionableOnly = linkedSignal({source: this.generation, computation: () => false});
    protected readonly visible = computed(() => this.report()?.issues.filter(issue =>
        (this.scope() === 'all' || issue.scope === this.scope()) &&
        (!this.actionableOnly() || issue.role !== null || issue.dependency?.owner != null),
    ) ?? []);
    protected readonly pendingSuggestion = computed(() => {
        const issue = this.preview();
        return issue?.suggestion && this.visible().includes(issue) ? issue : null;
    });
    protected readonly projectIssues = computed(() => this.visible().filter(issue => issue.scope === 'project'));
    protected readonly issueGroups = computed(() => {
        const rows = new Map<SceneId, SceneIssue[]>();
        for (const issue of this.visible()) {
            if (issue.scene_id === null) continue;
            const existing = rows.get(issue.scene_id);
            if (existing) existing.push(issue);
            else rows.set(issue.scene_id, [issue]);
        }
        return Array.from(rows, ([id, issues]) => ({id, issues}));
    });
    private readonly sceneIssues = computed(() =>
        new Set(this.report()?.issues.flatMap(issue => issue.scene_id === null ? [] : [issue.scene_id])),
    );
    private readonly sceneErrors = computed(() => new Set(this.report()?.issues.flatMap(issue =>
        issue.scene_id === null || issue.dependency?.severity === 'warning' ? [] : [issue.scene_id],
    )));
    protected readonly summary = computed(() => {
        const report = this.report();
        if (!report) return null;
        const scenes = report.scenes.map(scene => {
            const visit = this.visits().get(scene.id);
            const checked = scene.checked && visit?.stamp === scene.content_stamp;
            const errors = this.sceneErrors().has(scene.id);
            const warnings = !errors && this.sceneIssues().has(scene.id);
            const passed = checked && !errors && visit.passed;
            const result = errors ? 'Issues' :
                passed ? (warnings ? 'Passed with warnings' : 'Passed') :
                    warnings ? 'Warnings' : scene.checked ? 'Needs visit' : 'Blocked';
            return {id: scene.id, result, passed};
        });
        const passed = scenes.filter(scene => scene.passed).length;
        const projectIssues = report.issues.filter(issue => issue.scope === 'project').length;
        return {scenes, passed, projectIssues, sceneIssues: this.sceneIssues().size, revision: report.revision};
    });
    protected readonly activeScene = computed(() => this.editor.snapshot()?.active_scene_id);

    protected async check(): Promise<void> {
        if (this.canCheck()) await this.checkCurrentScene();
    }

    private async checkCurrentScene(): Promise<void> {
        if (this.destroy.destroyed) return;
        // Scene switching can finish before Angular refreshes the parent input.
        const snapshot = this.editor.snapshot();
        if (!this.open() || this.busy() || this.editor.busy() || this.sceneSwitch.pending() ||
            snapshot?.mode !== 'edit' || snapshot.transaction !== null) return;
        const owner = this.generation();
        this.busy.set(true);
        this.error.set(null);
        try {
            const report = await this.editor.checkScenes(snapshot.active_scene_id, snapshot.revision, this.nativeGeneration());
            const accepted = {owner, report};
            const latest = this.editor.snapshot();
            if (!this.open() || !this.matches(accepted, latest) || !latest) return;
            const active = report.scenes.find(scene => scene.id === latest.active_scene_id && scene.checked);
            const visits = new Map([...this.visits()].filter(([id]) => report.scenes.some(scene => scene.id === id)));
            if (active) {
                visits.set(active.id, {
                    stamp: active.content_stamp,
                    passed: !report.issues.some(issue =>
                        issue.scene_id === active.id && issue.dependency?.severity !== 'warning'),
                });
            }
            this.visits.set(visits);
            this.accepted.set(accepted);
        } catch (error) {
            if (this.open() && this.matchesRequest(owner, snapshot)) {
                this.error.set(error instanceof Error ? error.message : String(error));
            }
        } finally {
            this.busy.set(false);
        }
    }

    protected setScope(value: string): void {
        this.scope.set(issueScopes.find(option => option.id === value)?.id ?? 'all');
        if (!this.pendingSuggestion()) this.preview.set(null);
    }

    protected setActionableOnly(checked: boolean): void {
        this.actionableOnly.set(checked);
        if (!this.pendingSuggestion()) this.preview.set(null);
    }

    protected async visit(id: SceneId): Promise<void> {
        const owner = this.generation();
        if (!this.canCheck() || !this.report()?.scenes.some(scene => scene.id === id)) return;
        const opened = await this.sceneSwitch.request(id);
        if (!this.open() || owner !== this.generation()) return;
        if (opened) await this.checkCurrentScene();
        this.restoreFocus('visit', id, owner);
    }

    protected inspect(issue: SceneIssue): void {
        if (!this.canCheck() || issue.scene_id !== this.activeScene() || !issue.role || !this.visible().includes(issue)) return;
        this.inspectRole.emit(issue.role);
    }

    protected repair(issue: SceneIssue, action: SceneDependencyAction): void {
        const accepted = this.accepted();
        const owner = issue.dependency?.owner;
        if (!accepted || !owner || !this.canCheck() || !issue.scene_id ||
            issue.scene_id !== this.activeScene() || !this.visible().includes(issue) ||
            !this.matches(accepted, this.editor.snapshot())) return;
        this.inspectDependency.emit({
            action, owner, scene: issue.scene_id, generation: accepted.owner,
            nativeGeneration: accepted.report.generation, revision: accepted.report.revision,
        });
    }

    protected repairLabel(action: SceneDependencyAction): string {
        switch (action.kind) {
            case 'inspector': return 'Inspect Owner';
            case 'script': return 'Open Script';
            case 'prop': return 'Open in Prop';
            case 'animation': return 'Open in Animation';
            case 'animator': return 'Open in Animator';
            case 'controls_graph': return 'Open in Controls Graph';
        }
    }

    protected review(issue: SceneIssue): void {
        if (!this.canCheck() || issue.scene_id !== this.activeScene() || !issue.suggestion ||
            !this.visible().includes(issue)) return;
        this.preview.set(issue);
        this.restoreFocus('cancel', this.issueKey(issue), this.generation());
    }

    protected cancel(issue: SceneIssue): void {
        this.preview.set(null);
        this.restoreFocus('review', this.issueKey(issue), this.generation());
    }

    protected async apply(issue: SceneIssue): Promise<void> {
        const accepted = this.accepted();
        const {role, suggestion, scene_id: sceneId} = issue;
        if (!this.canCheck() || !accepted || this.pendingSuggestion() !== issue ||
            !role || !suggestion || !sceneId || !this.matches(accepted, this.editor.snapshot()) ||
            sceneId !== accepted.report.active_scene_id) return;

        this.preview.set(null);
        this.busy.set(true);
        this.error.set(null);
        let applied = false;
        const assign = async () => {
            if (!this.open() || !this.matches(accepted, this.editor.snapshot())) {
                throw new Error('The scene changed. Check Scenes and review the assignment again.');
            }
            await this.editor.execute({
                type: 'set_scene_binding',
                scene_id: sceneId,
                name: role,
                target: {kind: 'assign', key: suggestion.entity_key},
            }, accepted.owner, accepted.report.revision);
            applied = true;
        };
        try {
            if (!this.editing) await assign();
            else if (!await this.editing.withCommittedEdit(assign)) {
                throw new Error(this.editing.error() ?? 'Finish the current edit and try again.');
            }
        } catch (error) {
            if (this.open() && this.generation() === accepted.owner) {
                this.error.set(error instanceof Error ? error.message : String(error));
            }
        } finally {
            this.busy.set(false);
        }
        if (!this.open() || this.generation() !== accepted.owner) return;
        if (applied) await this.checkCurrentScene();
        this.restoreFocus('review', this.issueKey(issue), accepted.owner);
    }

    protected issueKey(issue: SceneIssue): string {
        return JSON.stringify([issue.scene_id, issue.role, issue.message, issue.dependency]);
    }

    private matches(accepted: AcceptedReport, snapshot: EditorSnapshot | null): boolean {
        const {report} = accepted;
        const generation = this.nativeGeneration();
        return !this.destroy.destroyed && snapshot !== null && accepted.owner === this.generation() && snapshot.mode === 'edit' &&
            snapshot.transaction === null && report.revision === snapshot.revision &&
            (generation === null || report.generation === generation) &&
            report.active_scene_id === snapshot.active_scene_id && report.scenes.length === snapshot.scenes.length &&
            snapshot.scenes.every(scene => report.scenes.some(checked => checked.id === scene.id));
    }

    private matchesRequest(owner: number, request: EditorSnapshot): boolean {
        const snapshot = this.editor.snapshot();
        return !this.destroy.destroyed && owner === this.generation() && snapshot?.revision === request.revision &&
            snapshot.active_scene_id === request.active_scene_id;
    }

    private restoreFocus(kind: string, key: string, owner: number): void {
        if (this.destroy.destroyed) return;
        afterNextRender({
            write: () => {
                const body = this.body()?.nativeElement;
                if (!this.open() || owner !== this.generation() || !body?.closest('dialog[open]')) return;
                const control = Array.from(body.querySelectorAll<HTMLElement>('[data-verification-action]'))
                    .find(item => item.dataset['verificationAction'] === kind && item.dataset['verificationKey'] === key &&
                        !item.matches(':disabled')) ?? body.querySelector<HTMLElement>('[data-verification-scope]');
                control?.focus();
            },
        }, {injector: this.injector});
    }
}

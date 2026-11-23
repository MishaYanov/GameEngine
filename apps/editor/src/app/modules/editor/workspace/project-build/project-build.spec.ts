import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorSnapshot, ProjectBuildResult} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ProjectService} from '../../../../core/project/project.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {ProjectBuild} from './project-build';

const output: ProjectBuildResult = {path: 'D:\\Exports\\Demo', asset_count: 7, asset_bytes: 1572864};
const prepared = {...output, build_id: 17};

function snapshot(): EditorSnapshot {
    return {
        active_scene_id: 'main', revision: 1, scene_revision: 1,
        project_dirty: false, dirty: false, can_undo: false, can_redo: false,
        mode: 'edit', paused: false, selection: [], transaction: null,
        scenes: [{id: 'main', asset_path: 'scenes/main.json', dirty: false, loaded: true}],
        project: {
            schema_version: 3, name: 'Build fixture', startup_scene_id: 'main',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, entities: [], lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
    };
}

async function createBuild() {
    const current = signal(snapshot());
    const busy = signal(false);
    const hasChanges = signal(false);
    const workspaceGeneration = signal(1);
    const reserved = signal(false);
    const chooseDestination = vi.fn<() => Promise<string | null>>().mockResolvedValue('D:\\Exports');
    const prepareBuild = vi.fn(async (_destination: string) => prepared);
    const finishBuild = vi.fn(async (_buildId: typeof prepared.build_id) => output);
    const runBuild = vi.fn(async (path: string) => ({path, process_id: 123}));
    const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
        reserved.set(true);
        try {
            await action();
            return true;
        } finally {
            reserved.set(false);
        }
    });
    TestBed.configureTestingModule({providers: [
            WorkspaceDraftReview,
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: current, busy, workspaceGeneration,
            playing: computed(() => current().mode === 'play'),
            dirty: computed(() => current().dirty), prepareBuild, finishBuild, runBuild,
        }},
        {provide: ProjectService, useValue: {chooseDestination}},
        {provide: SceneEditingService, useValue: {
            ready: computed(() => !reserved() && !busy() && current().mode === 'edit' && current().transaction === null),
            hasChanges, error: signal<string | null>(null), withCommittedEdit,
        }},
    ]});
    const fixture = TestBed.createComponent(ProjectBuild);
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const exportButton = () => root.querySelector<HTMLButtonElement>('button')!;
    const runButton = () => [...root.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent?.includes('Run Exported Game'))!;
    return {
        fixture,
        root,
        current,
        busy,
        hasChanges,
        drafts: TestBed.inject(WorkspaceDraftReview),
        workspaceGeneration,
        reserved,
        chooseDestination,
        prepareBuild,
        finishBuild,
        runBuild,
        withCommittedEdit,
        exportButton,
        runButton
    };
}

describe('ProjectBuild', () => {
    it('cancels destination selection without building or launching a game', async () => {
        const {
            fixture,
            root,
            drafts,
            chooseDestination,
            prepareBuild,
            finishBuild,
            runBuild,
            exportButton
        } = await createBuild();
        const review = vi.fn(() => true);
        drafts.setReviewers([review]);
        chooseDestination.mockResolvedValueOnce(null);
        exportButton().click();
        await fixture.whenStable();
        expect(chooseDestination).toHaveBeenCalledExactlyOnceWith(null);
        expect(review).not.toHaveBeenCalled();
        expect(prepareBuild).not.toHaveBeenCalled();
        expect(finishBuild).not.toHaveBeenCalled();
        expect(runBuild).not.toHaveBeenCalled();
        expect(exportButton().disabled).toBe(false);
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('requires saved scenes and committed field drafts before export', async () => {
        const {fixture, root, current, hasChanges, chooseDestination, exportButton} = await createBuild();
        current.update(value => ({...value, dirty: true}));
        await fixture.whenStable();
        expect(exportButton().disabled).toBe(true);
        expect(root.textContent).toContain('Save All before exporting');
        exportButton().click();
        expect(chooseDestination).not.toHaveBeenCalled();
        current.update(value => ({...value, dirty: false}));
        hasChanges.set(true);
        await fixture.whenStable();
        expect(exportButton().disabled).toBe(true);
    });

    it('exposes export errors for Problems and clears them when another workspace opens', async () => {
        const {fixture, root, workspaceGeneration, finishBuild, exportButton} = await createBuild();
        finishBuild.mockRejectedValueOnce(new Error('Compiler failed'));
        exportButton().click();
        await fixture.whenStable();
        expect(fixture.componentInstance.exportFailure()).toBe('Compiler failed');
        fixture.componentInstance.revealExportFailure();
        expect(document.activeElement).toBe(root.querySelector('[role="alert"]'));
        workspaceGeneration.update(value => value + 1);
        await fixture.whenStable();
        expect(fixture.componentInstance.exportFailure()).toBeNull();
    });

    it('rechecks dirty state after destination selection', async () => {
        const {fixture, root, current, chooseDestination, prepareBuild, exportButton} = await createBuild();
        let choose!: (path: string | null) => void;
        chooseDestination.mockImplementationOnce(() => new Promise(resolve => choose = resolve));
        exportButton().click();
        current.update(value => ({...value, dirty: true}));
        choose('D:\\Exports');
        await fixture.whenStable();
        expect(prepareBuild).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('Save All and stop Play');
    });

    it('waits for local drafts before staging and uses the new revision when a draft saves', async () => {
        const {fixture, current, drafts, prepareBuild, withCommittedEdit, exportButton} = await createBuild();
        let approve!: (accepted: boolean) => void;
        const review = vi.fn(() => new Promise<boolean>(resolve => approve = resolve));
        drafts.setReviewers([review]);
        exportButton().click();
        await vi.waitFor(() => expect(review).toHaveBeenCalledOnce());
        expect(withCommittedEdit).not.toHaveBeenCalled();
        current.update(value => ({...value, revision: 4}));
        approve(true);
        await fixture.whenStable();
        expect(prepareBuild).toHaveBeenCalledExactlyOnceWith('D:\\Exports', 1, 4);
    });

    it('does not stage when the active scene changes during draft review', async () => {
        const {fixture, current, drafts, prepareBuild, finishBuild, exportButton} = await createBuild();
        let approve!: (accepted: boolean) => void;
        const review = vi.fn(() => new Promise<boolean>(resolve => approve = resolve));
        drafts.setReviewers([review]);
        exportButton().click();
        await vi.waitFor(() => expect(review).toHaveBeenCalledOnce());
        current.update(value => ({...value, active_scene_id: 'other', revision: 2}));
        approve(true);
        await fixture.whenStable();
        expect(prepareBuild).not.toHaveBeenCalled();
        expect(finishBuild).not.toHaveBeenCalled();
        expect(exportButton().disabled).toBe(false);
    });

    it('shows export contents and launches only after the explicit Run action using its expected path', async () => {
        const {fixture, root, prepareBuild, finishBuild, runBuild, exportButton, runButton} = await createBuild();
        exportButton().click();
        await fixture.whenStable();
        expect(prepareBuild).toHaveBeenCalledExactlyOnceWith('D:\\Exports', 1, 1);
        expect(finishBuild).toHaveBeenCalledExactlyOnceWith(prepared.build_id);
        expect(root.textContent).toContain(output.path);
        expect(root.textContent).toContain('7 assets · 1.5 MiB');
        expect(runBuild).not.toHaveBeenCalled();
        runButton().click();
        await fixture.whenStable();
        expect(runBuild).toHaveBeenCalledExactlyOnceWith(output.path);
        expect(root.querySelector('[role="status"]')?.textContent).toContain('Standalone player started');
    });

    it('reserves edits through the prepare acknowledgment, then releases them during background compilation', async () => {
        const {fixture, root, current, reserved, prepareBuild, finishBuild, withCommittedEdit, exportButton} = await createBuild();
        let acknowledge!: (result: typeof prepared) => void;
        let finish!: (result: ProjectBuildResult) => void;
        prepareBuild.mockImplementationOnce(() => new Promise(resolve => acknowledge = resolve));
        finishBuild.mockImplementationOnce(() => new Promise(resolve => finish = resolve));
        exportButton().click();
        await vi.waitFor(() => expect(prepareBuild).toHaveBeenCalledOnce());
        expect(reserved()).toBe(true);
        expect(finishBuild).not.toHaveBeenCalled();
        acknowledge(prepared);
        await vi.waitFor(() => expect(finishBuild).toHaveBeenCalledOnce());
        expect(await withCommittedEdit.mock.results[0]!.value).toBe(true);
        expect(reserved()).toBe(false);
        fixture.detectChanges();
        expect(root.textContent).toContain('You can continue editing');
        current.update(value => ({...value, dirty: true, revision: 2}));
        finish(output);
        await fixture.whenStable();
        expect(root.textContent).toContain(output.path);
        expect(root.textContent).toContain('Export complete');
    });

    it('preserves the destination and permits retry after a failed export', async () => {
        const {fixture, root, chooseDestination, prepareBuild, finishBuild, exportButton} = await createBuild();
        prepareBuild.mockRejectedValueOnce(new Error('The output folder already exists.'));
        exportButton().click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('output folder already exists');
        expect(exportButton().disabled).toBe(false);
        exportButton().click();
        await fixture.whenStable();
        expect(chooseDestination).toHaveBeenLastCalledWith('D:\\Exports');
        expect(prepareBuild).toHaveBeenCalledTimes(2);
        expect(finishBuild).toHaveBeenCalledOnce();
        expect(root.querySelector('[role="alert"]')).toBeNull();
        expect(root.textContent).toContain('Export complete');
    });

    it('retries failed compilation using the same staged build even when live edits are now dirty', async () => {
        const {fixture, root, current, chooseDestination, prepareBuild, finishBuild, exportButton} = await createBuild();
        finishBuild.mockRejectedValueOnce(new Error('The compiler could not finish.'));
        exportButton().click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('compiler could not finish');
        current.update(value => ({...value, dirty: true, revision: 2}));
        await fixture.whenStable();
        const retry = [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(button => button.textContent?.trim() === 'Retry Export')!;
        expect(exportButton().disabled).toBe(true);
        expect(retry.disabled).toBe(false);
        retry.click();
        await fixture.whenStable();
        expect(chooseDestination).toHaveBeenCalledOnce();
        expect(prepareBuild).toHaveBeenCalledOnce();
        expect(finishBuild.mock.calls).toEqual([[prepared.build_id], [prepared.build_id]]);
        expect(root.querySelector('[role="alert"]')).toBeNull();
        expect(root.textContent).toContain('Export complete');
        expect(root.textContent).toContain(output.path);
        expect(root.textContent).not.toContain('Retry Export');
    });

    it('clears failed preparation retry when the workspace generation changes', async () => {
        const {fixture, root, workspaceGeneration, finishBuild, exportButton} = await createBuild();
        finishBuild.mockRejectedValueOnce(new Error('The compiler could not finish.'));
        exportButton().click();
        await fixture.whenStable();
        expect(root.textContent).toContain('Retry Export');
        workspaceGeneration.update(value => value + 1);
        await fixture.whenStable();
        expect(root.textContent).not.toContain('Retry Export');
        expect(root.textContent).not.toContain('Retry the staged files');
    });

    it('permits explicit launch retry without rebuilding after a launch failure', async () => {
        const {fixture, root, prepareBuild, finishBuild, runBuild, exportButton, runButton} = await createBuild();
        exportButton().click();
        await fixture.whenStable();
        runBuild.mockRejectedValueOnce(new Error('The player could not start.'));
        runButton().click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('player could not start');
        expect(runButton().disabled).toBe(false);
        runButton().click();
        await fixture.whenStable();
        expect(runBuild).toHaveBeenCalledTimes(2);
        expect(prepareBuild).toHaveBeenCalledOnce();
        expect(finishBuild).toHaveBeenCalledOnce();
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('does not export a project replaced while the destination chooser is open', async () => {
        const {fixture, current, workspaceGeneration, chooseDestination, prepareBuild, exportButton} = await createBuild();
        let choose!: (path: string | null) => void;
        chooseDestination.mockImplementationOnce(() => new Promise(resolve => choose = resolve));
        exportButton().click();
        current.update(value => ({...value, project: {...value.project, name: 'Another project'}}));
        workspaceGeneration.update(value => value + 1);
        choose('D:\\Exports');
        await fixture.whenStable();
        expect(prepareBuild).not.toHaveBeenCalled();
        expect(exportButton().disabled).toBe(false);
    });

    it('disables exporting during Play', async () => {
        const {fixture, current, chooseDestination, exportButton} = await createBuild();
        current.update(value => ({...value, mode: 'play'}));
        await fixture.whenStable();
        expect(exportButton().disabled).toBe(true);
        exportButton().click();
        expect(chooseDestination).not.toHaveBeenCalled();
    });

    it('clears a previous output when the same manifest is opened in a new workspace generation', async () => {
        const {fixture, root, workspaceGeneration, exportButton} = await createBuild();
        exportButton().click();
        await fixture.whenStable();
        expect(root.textContent).toContain(output.path);
        workspaceGeneration.update(value => value + 1);
        await fixture.whenStable();
        expect(root.querySelector('.output')).toBeNull();
        expect(root.textContent).not.toContain('Run Exported Game');
    });

    it('does not expose a previous workspace output when a background build finishes after reopening the same manifest', async () => {
        const {fixture, root, workspaceGeneration, finishBuild, exportButton} = await createBuild();
        let finish!: (result: ProjectBuildResult) => void;
        finishBuild.mockImplementationOnce(() => new Promise(resolve => finish = resolve));
        exportButton().click();
        await vi.waitFor(() => expect(finishBuild).toHaveBeenCalledOnce());
        workspaceGeneration.update(value => value + 1);
        finish(output);
        await fixture.whenStable();
        expect(root.querySelector('.output')).toBeNull();
        expect(root.textContent).not.toContain('Run Exported Game');
    });
});

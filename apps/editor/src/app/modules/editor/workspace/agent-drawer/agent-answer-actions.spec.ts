import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {EditorService} from '../../../../core/editor/editor.service';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import {PropPreviewService} from '../prop-workspace/prop-preview.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {AgentAnswerActions} from './agent-answer-actions';
import {AgentSelectionContextService, type AgentSelectionContext} from './agent-selection-context';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

describe('Agent answer navigation', () => {
    const target: AgentSelectionContext = {label: 'Gate', componentId: 'exact-clip', subject: {
        address: {kind: 'linked', scene: 'workshop', root: 'east-assembly',
            address: {kind: 'object', placement_path: ['step-2', 'gate'], source_entity: 'door'}},
        scope: {workspaceGeneration: 1, nativeGeneration: 7, revision: 10},
        contentRevision: 4, access: 'linked-overrides',
    }};
    const snapshot = signal({active_scene_id: 'workshop', revision: 12, scene_revision: 4, selection: ['west-door']});
    const canDeliver = vi.fn();
    const select = vi.fn(async (key: string) => snapshot.update(state => ({...state, selection: [key]})));
    const openScoped = vi.fn(async () => true);
    const reveal = vi.fn();
    let actions: AgentAnswerActions;

    beforeEach(() => {
        vi.clearAllMocks();
        canDeliver.mockReturnValue(true);
        snapshot.set({active_scene_id: 'workshop', revision: 12, scene_revision: 4, selection: ['west-door']});
        vi.mocked(invoke).mockResolvedValue({
            address: target.subject.address, entity_key: 'east-door', workspace_generation: 7, content_revision: 4, revision: 12,
            selected_component: {id: 'exact-clip', data: {kind: 'animation', settings: {
                source: {asset_id: 'clip', expected_sha256: 'fingerprint'}, playback: {kind: 'single_clip', clip_index: 3},
            }}},
        });
        TestBed.configureTestingModule({providers: [AgentAnswerActions,
            {provide: EditorService, useValue: {snapshot, workspaceGeneration: () => 1}},
            {provide: AgentSelectionContextService, useValue: {canDeliver}},
            {provide: WorkspaceDraftReview, useValue: {capture: () => ({}), current: () => true,
                run: async (_scope: unknown, action: () => Promise<void>) => { await action(); return true; }}},
            {provide: SceneEditingService, useValue: {select}},
            {provide: WorkspaceViewService, useValue: {selectView: vi.fn()}},
            {provide: PropWorkspaceService, useValue: {document: () => null}},
            {provide: PropPreviewService, useValue: {}},
            {provide: AnimationWorkspaceService, useValue: {openScoped}},
            {provide: ComponentWorkspaceSession, useValue: {}},
        ]});
        actions = TestBed.inject(AgentAnswerActions);
        actions.setReveal(reveal);
    });

    it('resolves the captured nested member independently of the current scene selection', async () => {
        expect(await actions.run({kind: 'open-component-editor', target, componentId: 'exact-clip'})).toBe(true);
        expect(invoke).toHaveBeenCalledWith('editor_agent_context', {request: {
            workspace_generation: 7, revision: 12, content_revision: 4, address: target.subject.address, component_id: 'exact-clip',
        }});
        expect(select).toHaveBeenCalledExactlyOnceWith('east-door');
        expect(openScoped).toHaveBeenCalledWith(expect.objectContaining({subject: expect.objectContaining({
            address: target.subject.address,
        }), componentId: 'exact-clip', clipIndex: 3}), expect.any(Function));
    });

    it('rejects another workspace and requires a refresh when authored content changed', async () => {
        canDeliver.mockReturnValueOnce(false);
        expect(await actions.run({kind: 'show-object', target})).toBe(false);
        expect(actions.notice()).toContain('earlier project');
        snapshot.update(state => ({...state, scene_revision: 5}));
        expect(await actions.run({kind: 'show-object', target})).toBe(false);
        expect(actions.notice()).toContain('Refresh Target');
        expect(invoke).not.toHaveBeenCalled();
        expect(select).not.toHaveBeenCalled();
    });
});

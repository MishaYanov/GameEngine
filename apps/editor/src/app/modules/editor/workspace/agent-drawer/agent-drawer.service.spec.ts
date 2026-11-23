import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {AgentDrawerService} from './agent-drawer.service';
import type {AgentSelectionContext} from './agent-selection-context';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

describe('AgentDrawerService', () => {
    const workspaceGeneration = signal(1);
    const viewport = signal({workspace_generation: 12});
    const snapshot = signal({revision: 2});
    let send: (frame: Record<string, unknown>) => void;
    let session: AgentDrawerService;
    const target: AgentSelectionContext = {label: 'Repeated', componentId: 'animation-1', subject: {
        address: {kind: 'scene', scene: 'main', object: 'first'}, contentRevision: 1, access: 'scene-edit',
        scope: {workspaceGeneration: 1, nativeGeneration: 12, revision: 2},
    }};

    beforeEach(() => {
        vi.clearAllMocks();
        workspaceGeneration.set(1);
        viewport.set({workspace_generation: 12});
        snapshot.set({revision: 2});
        vi.mocked(listen).mockImplementation(async (_name, callback) => {
            send = payload => callback({event: 'editor-agent-session', id: 1, payload});
            return vi.fn<() => void>();
        });
        vi.mocked(invoke).mockResolvedValue(undefined);
        TestBed.configureTestingModule({providers: [AgentDrawerService,
            {provide: EditorService, useValue: {nativeAvailable: true, workspaceGeneration, snapshot}},
            {provide: ViewportEditorService, useValue: {state: viewport}},
        ]});
        session = TestBed.inject(AgentDrawerService);
    });

    it('captures the precise attachment and streams results after a selection revision changes', async () => {
        expect(await session.start('How do I animate this?', target)).toBe(true);
        const id = session.answers()[0].id;
        expect(invoke).toHaveBeenCalledWith('editor_agent_session_start', {request: {
            id, question: 'How do I animate this?', workspace_generation: 12, revision: 2,
            selection: {address: target.subject.address, content_revision: 1, component_id: 'animation-1'},
        }});
        snapshot.set({revision: 3});
        send({type: 'delta', workspace_generation: 11, id, text: 'wrong workspace'});
        send({type: 'delta', workspace_generation: 12, id: 'wrong request', text: 'wrong request'});
        send({type: 'started', workspace_generation: 12, id});
        send({type: 'delta', workspace_generation: 12, id, text: 'Open the Animation workspace.'});
        send({type: 'finished', workspace_generation: 12, id, status: 'completed'});
        expect(session.answers()[0]).toMatchObject({selection: target,
            status: 'completed', text: 'Open the Animation workspace.'});
        expect(session.running()).toBeNull();
    });

    it('stops and rejects late events, then clears history when another project opens', async () => {
        await session.start('What can I do?', null);
        const id = session.answers()[0].id;
        session.stop();
        send({type: 'delta', workspace_generation: 12, id, text: 'late'});
        expect(invoke).toHaveBeenLastCalledWith('editor_agent_session_stop', {request: {id, workspace_generation: 12}});
        expect(session.answers()[0]).toMatchObject({status: 'cancelled', text: ''});
        workspaceGeneration.set(2);
        TestBed.tick();
        expect(session.answers()).toEqual([]);
    });

    it('preserves bounded keyword sources when the model is absent', async () => {
        await session.start('Authoring', null);
        const {id} = session.answers()[0];
        const citation = {corpus: 'engine_help', source_id: 'workspace', relative_path: 'editor-workspace.md',
            heading: 'Authoring', ordinal: 1, source_revision: 'v1', content_hash: 'hash'};
        send({type: 'sources', workspace_generation: 12, id, citations: [
            {citation, excerpt: 'Inspect the selected object.'},
            {citation: {...citation, corpus: 'foreign'}, excerpt: 'Invalid scope.'},
        ]});
        send({type: 'finished', workspace_generation: 12, id, status: 'retrieval_only'});
        expect(session.answers()[0]).toMatchObject({status: 'retrieval_only',
            references: [{citation, excerpt: 'Inspect the selected object.'}]});
    });
});

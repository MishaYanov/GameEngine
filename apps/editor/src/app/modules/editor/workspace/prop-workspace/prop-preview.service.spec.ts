import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {PropPreviewService} from './prop-preview.service';
import {PropWorkspaceService} from './prop-workspace.service';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

function reply(requestId: number, owner = 'native-token') {
    return {
        token: owner, workspace_generation: 7, revision: 8,
        request_id: requestId, selection: 'root',
    };
}

function setup() {
    const activeViewKind = signal<WorkspaceViewKind>('prop-workspace');
    const requests = new Map<number, (value: unknown) => void>();
    const selectObject = vi.fn();
    vi.mocked(listen).mockReset().mockResolvedValue(vi.fn());
    vi.mocked(invoke).mockReset().mockImplementation((command, fields) => {
        if (command !== 'editor_prop_preview_open') return Promise.resolve(undefined as never);
        const args = fields as {requestId: number};
        return new Promise(resolve => requests.set(args.requestId, resolve));
    });
    TestBed.configureTestingModule({
        providers: [
            PropPreviewService,
            {provide: EditorService, useValue: {
                nativeAvailable: true, playing: signal(false), snapshot: signal({
                    revision: 8, scene_revision: 4, active_scene_id: 'arena',
                }),
            }},
            {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 7})}},
            {provide: WorkspaceViewService, useValue: {activeViewKind}},
            {provide: PropWorkspaceService, useValue: {
                document: signal({kind: 'instance', root: 'root'}), selectObject,
            }},
        ],
    });
    const service = TestBed.inject(PropPreviewService);
    TestBed.tick();
    return {service, activeViewKind, requests, selectObject};
}

it('frames and selects using the current native token, then releases the preview on tab exit', async () => {
    const {service, requests, activeViewKind, selectObject} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_open', {
        target: {kind: 'instance', scene_id: 'arena', root_key: 'root'},
        expectedGeneration: 7, expectedRevision: 8, requestId,
    });
    resolve(reply(requestId));
    await Promise.resolve();
    expect(service.ready()).toBe(true);
    await service.frame();
    await service.select('child');
    expect(selectObject).toHaveBeenLastCalledWith('child');
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_frame', {token: 'native-token'});
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_select', {token: 'native-token', key: 'child'});
    activeViewKind.set('scene-viewport');
    TestBed.tick();
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'native-token'});
    expect(service.ready()).toBe(false);
});

it('closes a delayed old preview without applying selection after the tab changes', async () => {
    const {service, requests, activeViewKind, selectObject} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    activeViewKind.set('scene-viewport');
    TestBed.tick();
    resolve(reply(requestId, 'retired-token'));
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'retired-token'});
    expect(service.ready()).toBe(false);
    expect(selectObject).not.toHaveBeenCalled();
});

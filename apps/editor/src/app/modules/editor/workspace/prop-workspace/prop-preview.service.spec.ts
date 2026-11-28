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
    const audioOrigin = signal<'scene' | 'prop'>('scene');
    const document = signal({kind: 'instance', root: 'root'});
    const requests = new Map<number, (value: unknown) => void>();
    const selectObject = vi.fn();
    const selectedPlacement = signal<{ id: string } | null>(null);
    const selectedObject = signal<{ key: string; components: readonly { data: { kind: string } }[] } | null>(null);
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
                    modelAssets: signal([]),
                capabilities: () => ({commands: [{name: 'set_scene_environment'}]}),
            }},
            {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 7})}},
            {provide: WorkspaceViewService, useValue: {activeViewKind, audioOrigin}},
            {provide: PropWorkspaceService, useValue: {
                    document, selectObject, selectedPlacement, selectedObject,
            }},
        ],
    });
    const service = TestBed.inject(PropPreviewService);
    TestBed.tick();
    return {service, activeViewKind, audioOrigin, document, requests, selectObject, selectedPlacement, selectedObject};
}

it('clears marker selection when a nested placement takes focus', async () => {
    const {service, requests, selectObject, selectedPlacement} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    vi.mocked(invoke).mockResolvedValueOnce({...reply(requestId), selection: null});

    selectedPlacement.set({id: 'nested-placement'});
    TestBed.tick();
    await Promise.resolve();
    await Promise.resolve();

    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_select', {token: 'native-token', key: null});
    expect(service.currentState()?.selection).toBeNull();
    expect(selectObject).toHaveBeenCalledTimes(1);
});

it('clears a nested placement selection after an earlier object request finishes', async () => {
    const {service, requests, selectedPlacement} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    let finishObjectSelection!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolveSelection => {
        finishObjectSelection = resolveSelection;
    }));
    vi.mocked(invoke).mockResolvedValueOnce({...reply(requestId), selection: null});
    const pendingSelection = service.select('child');
    await Promise.resolve();

    selectedPlacement.set({id: 'nested-placement'});
    TestBed.tick();
    expect(invoke).not.toHaveBeenCalledWith('editor_prop_preview_select', {token: 'native-token', key: null});
    finishObjectSelection({...reply(requestId), selection: 'child'});
    await pendingSelection;
    await Promise.resolve();

    expect(invoke).toHaveBeenLastCalledWith('editor_prop_preview_select', {token: 'native-token', key: null});
    expect(service.currentState()?.selection).toBeNull();
});

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

it('preserves a Prop audition token only while its Audio workspace is active', async () => {
    const {service, requests, activeViewKind, audioOrigin} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    audioOrigin.set('prop');
    activeViewKind.set('audio-workspace');
    TestBed.tick();
    expect(service.currentState()?.token).toBe('native-token');
    expect(invoke).not.toHaveBeenCalledWith('editor_prop_preview_close', {token: 'native-token'});
    audioOrigin.set('scene');
    TestBed.tick();
    expect(service.ready()).toBe(false);
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'native-token'});
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

it('changes only the live Prop token environment and restores Neutral when entering another preview', async () => {
    const {service, requests, activeViewKind} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    expect(service.environment()).toBe('neutral');
    await service.setEnvironment('scene');
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_environment', {token: 'native-token', environment: 'scene'});
    expect(service.environment()).toBe('scene');
    activeViewKind.set('scene-viewport');
    TestBed.tick();
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'native-token'});
    expect(service.environment()).toBe('neutral');
    activeViewKind.set('prop-workspace');
    TestBed.tick();
    const [newRequest, resolveNext] = Array.from(requests).at(-1)!;
    resolveNext(reply(newRequest, 'next-token'));
    await Promise.resolve();
    expect(service.ready()).toBe(true);
    expect(service.environment()).toBe('neutral');
});

it('discards a pending preview environment response after leaving the Prop tab', async () => {
    const {service, requests, activeViewKind} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    let finish: (value: never) => void = () => {};
    vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const pending = service.setEnvironment('scene');
    expect(service.environmentPending()).toBe(true);
    activeViewKind.set('scene-viewport');
    TestBed.tick();
    finish(undefined as never);
    await pending;
    expect(service.environmentPending()).toBe(false);
    expect(service.environment()).toBe('neutral');
    expect(service.ready()).toBe(false);
});

it('restores shading and vertices but resets normal detail on a new Prop preview', async () => {
    const {service, requests, activeViewKind, document} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    expect(service.display()).toEqual({shading: 'lit', showVertices: false, showNormalDetail: true});
    expect(invoke).not.toHaveBeenCalledWith('editor_prop_preview_display', expect.anything());

    const display = {shading: 'wireframe', showVertices: true, showNormalDetail: false} as const;
    await service.setDisplay(display);
    expect(service.display()).toEqual(display);
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_display', {token: 'native-token', ...display});

    activeViewKind.set('scene-viewport');
    TestBed.tick();
    document.set({kind: 'instance', root: 'different-prop'});
    activeViewKind.set('prop-workspace');
    TestBed.tick();
    const [nextRequest, resolveNext] = Array.from(requests).at(-1)!;
    resolveNext(reply(nextRequest, 'next-token'));
    await Promise.resolve();
    expect(service.ready()).toBe(false);
    const nextDisplay = {...display, showNormalDetail: true};
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_display', {token: 'next-token', ...nextDisplay});
    await Promise.resolve();
    expect(service.ready()).toBe(true);
    expect(service.display()).toEqual(nextDisplay);
});

it('keeps the accepted display when the native Prop change fails', async () => {
    const {service, requests} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    vi.mocked(invoke).mockRejectedValueOnce(new Error('Preview replaced'));
    await service.setDisplay({shading: 'unlit', showVertices: true, showNormalDetail: true});
    expect(service.display()).toEqual({shading: 'lit', showVertices: false, showNormalDetail: true});
    expect(service.displayError()).toBe('Preview replaced');
    expect(service.ready()).toBe(true);
});

it('releases a Prop preview when its saved display is still being restored', async () => {
    const {service, requests, activeViewKind} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();
    const display = {shading: 'unlit', showVertices: true, showNormalDetail: true} as const;
    await service.setDisplay(display);

    activeViewKind.set('scene-viewport');
    TestBed.tick();
    activeViewKind.set('prop-workspace');
    TestBed.tick();
    const [nextRequest, resolveNext] = Array.from(requests).at(-1)!;
    let finish: (value: never) => void = () => {
    };
    vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => {
        finish = resolve;
    }));
    resolveNext(reply(nextRequest, 'pending-token'));
    await Promise.resolve();
    expect(service.ready()).toBe(false);

    activeViewKind.set('scene-viewport');
    TestBed.tick();
    expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'pending-token'});
    finish(undefined as never);
    await Promise.resolve();
    expect(service.ready()).toBe(false);
    expect(service.display()).toEqual(display);
});

it('keeps material diagnostics scoped to the latest selected object', async () => {
    const {service, requests, selectedObject, activeViewKind} = setup();
    const [requestId, resolve] = Array.from(requests)[0];
    resolve(reply(requestId));
    await Promise.resolve();

    let finishFirst!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementationOnce(() => new Promise(finish => {
        finishFirst = finish;
    }));
    const components = [{data: {kind: 'mesh_renderer'}}];
    selectedObject.set({key: 'root', components});
    TestBed.tick();
    await Promise.resolve();
    expect(invoke).toHaveBeenLastCalledWith('editor_prop_preview_materials', {token: 'native-token', key: 'root'});

    const latest = {
        model_path: 'models/current.glb', materials: [
            {
                index: 0, name: 'Stone', normal: {image: 2, texture: 4, tex_coord: 1, scale: 0.625},
                primitives: [{index: 0, detail: 'Imported tangents from the model.'}]
            },
        ]
    };
    vi.mocked(invoke).mockImplementationOnce(async () => latest as never);
    selectedObject.set({key: 'mesh', components});
    TestBed.tick();
    expect(service.materials()).toBeNull();
    finishFirst({model_path: 'models/stale.glb', materials: []});
    await vi.waitFor(() => expect(service.materials()).toEqual(latest));
    expect(invoke).toHaveBeenLastCalledWith('editor_prop_preview_materials', {token: 'native-token', key: 'mesh'});

    activeViewKind.set('scene-viewport');
    TestBed.tick();
    expect(service.materials()).toBeNull();
    expect(service.materialsPending()).toBe(false);
});

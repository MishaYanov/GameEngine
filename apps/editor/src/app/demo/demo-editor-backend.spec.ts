import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {invoke} from '@tauri-apps/api/core';
import {EDITOR_BACKEND} from '../core/editor/editor-backend';
import {EditorService} from '../core/editor/editor.service';
import type {EditorCapabilities, EditorCommand, EditorSnapshot,} from '../core/editor/editor.models';
import {DemoEditorBackend} from './demo-editor-backend';

vi.mock('@tauri-apps/api/core', () => ({isTauri: () => false, invoke: vi.fn()}));

describe('Browser demo project', () => {
    let backend: DemoEditorBackend;
    let editor: EditorService;
    beforeEach(async () => {
        vi.mocked(invoke).mockClear();
        backend = new DemoEditorBackend();
        TestBed.configureTestingModule({
            providers: [EditorService, {provide: EDITOR_BACKEND, useValue: backend}],
        });
        editor = TestBed.inject(EditorService);
        await editor.openProject('demo://project.json');
    });

    it('uses the shared editor service with no Tauri IPC and reports no native viewport', async () => {
        expect(editor.nativeAvailable).toBe(false);
        expect(editor.snapshot()?.project.name).toBe('Editor Playground');
        expect((await editor.assets()).length).toBe(4);
        expect((await editor.loadCapabilities()).embedded_viewport_supported).toBe(false);
        await expect(
            editor.attachViewport({x: 0, y: 0, width: 800, height: 600, visible: true}),
        ).rejects.toMatchObject({kind: 'unavailable'});
        expect(invoke).not.toHaveBeenCalled();
    });

    it('serializes queued edits, supports undo/redo, and tracks a session save point', async () => {
        await Promise.all([
            editor.execute({type: 'rename', key: 'player', name: 'Hero'}),
            editor.execute({type: 'rename', key: 'player', name: 'Explorer'}),
        ]);
        expect(editor.snapshot()?.revision).toBe(2);
        await editor.save();
        expect(editor.dirty()).toBe(false);
        await editor.execute({type: 'undo'});
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Hero');
        expect(editor.dirty()).toBe(true);
        await editor.execute({type: 'redo'});
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Explorer');
        expect(editor.dirty()).toBe(false);
    });

    it('keeps independent scene drafts and histories across scene switches', async () => {
        await editor.execute({type: 'rename', key: 'player', name: 'Hero'});
        await editor.switchScene('lighting');
        expect(editor.selection()).toEqual([]);
        expect(editor.snapshot()?.can_undo).toBe(false);
        expect(editor.dirty()).toBe(true);
        await editor.execute({type: 'rename', key: 'tree', name: 'Oak'});
        await editor.save();
        expect(editor.dirty()).toBe(true); // The other scene remains dirty.
        await editor.switchScene('playground');
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Hero');
        await editor.execute({type: 'undo'});
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Player');
        expect(editor.dirty()).toBe(false);
        await editor.switchScene('lighting');
        expect(editor.document()?.entities.find((e) => e.key === 'tree')?.name).toBe('Oak');
    });

    it('deletes descendants and bindings, and can undo deletion', async () => {
        await editor.execute({type: 'select', keys: ['crate', 'player']});
        await editor.execute({type: 'delete_selection'});
        expect(editor.document()?.entities.map((e) => e.key)).toEqual(['ground']);
        expect(editor.document()?.bindings).toEqual({});
        await editor.execute({type: 'undo'});
        expect(editor.document()?.entities).toHaveLength(4);
        expect(editor.document()?.bindings['player']).toBe('player');
    });

    it('isolates returned snapshots, resets fixtures, and rejects unsupported commands', async () => {
        const snapshot = await editor.refresh();
        Object.assign(snapshot.document.entities[0], {name: 'Mutated outside the backend'});
        expect((await editor.refresh()).document.entities[0].name).toBe('Ground');
        await editor.execute({type: 'rename', key: 'player', name: 'Hero'});
        await expect(editor.execute({type: 'orbit_camera', yaw: 1, pitch: 0})).rejects.toThrow(
            'does not simulate',
        );
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Hero');
        await editor.openProject('demo://project.json');
        expect(editor.document()?.entities.find((e) => e.key === 'player')?.name).toBe('Player');
        expect(editor.snapshot()?.revision).toBe(0);
        expect(editor.dirty()).toBe(false);
    });

    it('supports every advertised command and rejects edit/save while previewing play', async () => {
        const capabilities = await backend.request<EditorCapabilities>('editor_capabilities');
        const commands: EditorCommand[] = [
            {type: 'select', keys: ['player']},
            {type: 'switch_scene', scene_id: 'playground'},
            {type: 'rename', key: 'player', name: 'Hero'},
            {type: 'spawn', entity: {name: 'Cube', model_path: 'models/cube.glb'}},
            {type: 'set_transforms', edits: [], space: 'local', snap: false},
            {type: 'delete_selection'},
            {type: 'duplicate_selection', offset: [1, 0, 0]},
            {type: 'undo'},
            {type: 'redo'},
            {
                type: 'set_snap',
                settings: {enabled: true, translation: 1, rotation_radians: 1, scale: 1},
            },
            {type: 'start_play'},
            {type: 'set_paused', paused: true},
            {type: 'stop_play'},
        ];
        expect(new Set(commands.map((c) => c.type))).toEqual(
            new Set(capabilities.commands.map((c) => c.name)),
        );
        for (const command of commands) await editor.execute(command);
        await editor.execute({type: 'start_play'});
        await expect(
            editor.execute({type: 'rename', key: 'player', name: 'Blocked'}),
        ).rejects.toThrow('Stop the play preview');
        await expect(editor.save()).rejects.toThrow('Stop the play preview');
        await editor.execute({type: 'stop_play'});
        await editor.save();
        expect(invoke).not.toHaveBeenCalled();
    });

    it('rejects stale revision requests without changing the project', async () => {
        await editor.execute({type: 'select', keys: ['ground']});
        await expect(
            backend.request<EditorSnapshot>('editor_execute', {
                request: {
                    expected_revision: 0,
                    command: {type: 'rename', key: 'ground', name: 'Wrong'},
                },
            }),
        ).rejects.toThrow('Revision conflict');
        expect((await editor.refresh()).document.entities[0].name).toBe('Ground');
    });
});

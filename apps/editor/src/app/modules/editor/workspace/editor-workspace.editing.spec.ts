import {Component, input, output} from '@angular/core';
import {type ComponentFixture, TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import type {EditorActionId} from '../../../core/editor/editor-actions';
import {EditorService} from '../../../core/editor/editor.service';
import type {
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    SceneDocument,
    ViewportSceneLoad,
    ViewportStatus
} from '../../../core/editor/editor.models';
import {EditorWorkspace} from './editor-workspace';
import {SceneViewport} from './scene-viewport/scene-viewport';
import {SceneEditingService} from './scene-editing.service';
import {SceneEntitiesPanel} from './scene-entities-panel/scene-entities-panel';
import {SceneSwitchService} from './scene-actions/scene-switch.service';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));

// Exercise the real UI, draft controller, revision queue and command bridge.
// GPU attachment is outside this DOM test.
@Component({selector: 'scene-viewport', template: ''})
class ViewportStub {
    readonly enabled = input(false);
    readonly label = input('');
    readonly editorActionRequested = output<EditorActionId>();
}

function initialSnapshot(): EditorSnapshot {
    return {
        active_scene_id: 'main', revision: 1, scene_revision: 1,
        project_dirty: false, dirty: false, mode: 'edit', paused: false,
        transaction: null, selection: [], can_undo: false, can_redo: false,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
        scenes: [{id: 'main', asset_path: 'scenes/main.json', loaded: true, dirty: false}],
        project: {
            schema_version: 3, name: 'Editing fixture', startup_scene_id: 'main',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
            entities: ['robot', 'crate'].map(key => ({
                key, name: key, model_path: `props/${key}.glb`, renderable: true,
                transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
            })),
        },
    };
}

describe('Workspace live editing flow', () => {
    let fixture: ComponentFixture<EditorWorkspace>;
    let editor: EditorService;
    let editing: SceneEditingService;
    let nativeState: EditorSnapshot;
    let savedDocument: SceneDocument;
    let transactionBefore: SceneDocument | null;
    let undo: SceneDocument[];
    let redo: SceneDocument[];
    let commands: EditorCommand[];
    let viewportStatus: ViewportStatus;

    beforeEach(async () => {
        localStorage.clear();
        nativeState = initialSnapshot();
        savedDocument = structuredClone(nativeState.document);
        transactionBefore = null;
        undo = [];
        redo = [];
        commands = [];
        viewportStatus = {
            attached: true, visible: true, width: 800, height: 500,
            draw_calls: 0, instances: 0, triangles: 0, gpu_time_ms: null,
            gpu_timestamps_supported: false, error: null,
        };
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset().mockImplementation(async (operation, args) => {
            if (operation === 'editor_capabilities') return {
                protocol_version: 2, scene_schema_version: 3, incremental_commands: true,
                commands: [], embedded_viewport_supported: true,
                max_selection: 4096, max_history_entries: 128, history_budget_bytes: 16 * 1024 * 1024,
            };
            if (operation === 'editor_open_project') {
                nativeState = {...initialSnapshot(), revision: nativeState.revision + 1, document: structuredClone(savedDocument)};
                return structuredClone(nativeState);
            }
            if (operation === 'editor_save') {
                expect(nativeState.transaction).toBeNull();
                savedDocument = structuredClone(nativeState.document);
                nativeState = {...nativeState, revision: nativeState.revision + 1, dirty: false};
                return structuredClone(nativeState);
            }
            if (operation === 'editor_snapshot') return structuredClone(nativeState);
            if (operation === 'editor_assets') return [];
            if (operation === 'editor_viewport_bounds') return viewportStatus;
            if (operation !== 'editor_execute_incremental') throw new Error(`Unexpected operation: ${operation}`);
            const {request} = args as {request: EditorCommandRequest};
            expect(request.expected_revision).toBe(nativeState.revision);
            const command = request.command;
            commands.push(command);
            switch (command.type) {
                case 'select': nativeState = {...nativeState, selection: command.keys}; break;
                case 'begin_transaction':
                    expect(nativeState.transaction).toBeNull();
                    transactionBefore = structuredClone(nativeState.document);
                    nativeState = {...nativeState, transaction: command.label};
                    break;
                case 'set_transforms':
                    nativeState = {...nativeState, dirty: true, document: {...nativeState.document,
                        entities: nativeState.document.entities.map(entity => {
                            const edit = command.edits.find(edit => edit.key === entity.key);
                            return edit ? {...entity, transform: edit.transform} : entity;
                        })}};
                    break;
                case 'rename':
                    nativeState = {...nativeState, dirty: true, document: {...nativeState.document,
                        entities: nativeState.document.entities.map(entity => entity.key === command.key ? {...entity, name: command.name} : entity)}};
                    break;
                case 'reparent':
                    expect(nativeState.transaction).toBeNull();
                    nativeState = {...nativeState, dirty: true, document: {...nativeState.document,
                        entities: nativeState.document.entities.map(entity => entity.key === command.key ? {...entity, parent: command.parent} : entity)}};
                    break;
                case 'commit_transaction':
                    expect(transactionBefore).not.toBeNull();
                    undo.push(transactionBefore!);
                    transactionBefore = null;
                    nativeState = {...nativeState, transaction: null, can_undo: true, can_redo: false};
                    break;
                case 'cancel_transaction':
                    nativeState = {...nativeState, transaction: null, document: transactionBefore!, dirty: false};
                    transactionBefore = null;
                    break;
                case 'undo':
                    expect(nativeState.transaction).toBeNull();
                    redo.push(nativeState.document);
                    nativeState = {...nativeState, document: undo.pop()!, can_undo: undo.length > 0, can_redo: true};
                    break;
                case 'redo':
                    undo.push(nativeState.document);
                    nativeState = {...nativeState, document: redo.pop()!, can_undo: true, can_redo: redo.length > 0};
                    break;
                case 'start_play':
                    expect(nativeState.transaction).toBeNull();
                    nativeState = {...nativeState, mode: 'play'};
                    break;
                case 'stop_play': nativeState = {...nativeState, mode: 'edit'}; break;
                case 'switch_scene':
                    nativeState = {...nativeState, active_scene_id: command.scene_id, selection: []};
                    break;
                default: throw new Error(`Unexpected command: ${command.type}`);
            }
            nativeState = {...nativeState, revision: nativeState.revision + 1, scene_revision: nativeState.scene_revision + 1};
            return {
                kind: 'snapshot', snapshot: structuredClone(nativeState),
                receipt: {
                    revision: nativeState.revision, scene_revision: nativeState.scene_revision,
                    change_cursor: nativeState.revision, active_scene_id: nativeState.active_scene_id,
                    affected_scenes: [nativeState.active_scene_id],
                    state_changed: true, project_changed: false,
                    changes: {changed_entities: [], removed_entities: [], camera_changed: false,
                        lights_changed: false, topology_changed: false, full_resync: false},
                },
            };
        });
        await TestBed.configureTestingModule({imports: [EditorWorkspace]})
            .overrideComponent(EditorWorkspace, {remove: {imports: [SceneViewport]}, add: {imports: [ViewportStub]}})
            .compileComponents();
        editor = TestBed.inject(EditorService);
        await editor.openProject('fixture/project.json');
        fixture = TestBed.createComponent(EditorWorkspace);
        editing = fixture.debugElement.injector.get(SceneEditingService);
        await fixture.whenStable();
    });

    function action(label: string): HTMLButtonElement {
        const button = Array.from(fixture.nativeElement.querySelectorAll('.workspace-actions button') as NodeListOf<HTMLButtonElement>)
            .find(button => button.textContent?.trim() === label);
        expect(button, `Missing ${label} button`).toBeTruthy();
        return button!;
    }

    async function selectRobot(): Promise<void> {
        const row = fixture.nativeElement.querySelector('.entity-list button') as HTMLButtonElement;
        row.click();
        await vi.waitFor(() => expect(editor.selection()).toEqual(['robot']));
        await fixture.whenStable();
        expect(row.getAttribute('aria-selected')).toBe('true');
    }

    function input(label: string): HTMLInputElement {
        const field = fixture.nativeElement.querySelector(`input[aria-label="${label}"]`) as HTMLInputElement;
        expect(field, `Missing ${label} field`).toBeTruthy();
        return field;
    }

    async function change(field: HTMLInputElement, value: string): Promise<void> {
        field.focus();
        field.value = value;
        field.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
    }

    it('selects, edits local transforms, saves, and displays persisted values after reopening', async () => {
        await selectRobot();
        const name = fixture.nativeElement.querySelector('inspector-panel input[type="text"]') as HTMLInputElement;
        await change(name, 'Player robot');
        name.blur();
        await editing.commit();
        await change(input('Position X'), '4.5');
        input('Position X').blur();
        await editing.commit();
        await change(input('Rotation Y (degrees)'), '90');
        input('Rotation Y (degrees)').blur();
        await editing.commit();
        await change(input('Scale Z'), '-2');
        input('Scale Z').blur();
        await editing.commit();
        await fixture.whenStable();
        const robot = editor.document()!.entities[0];
        expect(robot.name).toBe('Player robot');
        expect(robot.transform.position).toEqual([4.5, 0, 0]);
        expect(robot.transform.rotation[1]).toBeCloseTo(Math.PI / 2);
        expect(robot.transform.scale).toEqual([1, 1, -2]);
        expect(commands.filter(command => command.type === 'set_transforms').every(command => command.space === 'local')).toBe(true);
        expect(action('Save Scene').disabled).toBe(false);
        action('Save Scene').click();
        await vi.waitFor(() => expect(editor.dirty()).toBe(false));
        await editor.openProject('fixture/project.json');
        await selectRobot();
        expect(input('Position X').valueAsNumber).toBe(4.5);
        expect(input('Rotation Y (degrees)').valueAsNumber).toBeCloseTo(90);
        expect(input('Scale Z').valueAsNumber).toBe(-2);
    });

    it('cancels with Escape and routes undo/redo through the shared history', async () => {
        await selectRobot();
        const x = input('Position X');
        await change(x, '8');
        await vi.waitFor(() => expect(editor.document()!.entities[0].transform.position[0]).toBe(8));
        x.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}));
        await editing.commit();
        await fixture.whenStable();
        expect(input('Position X').valueAsNumber).toBe(0);
        expect(commands.filter(command => command.type === 'commit_transaction')).toHaveLength(0);
        await change(input('Position X'), '6');
        input('Position X').blur();
        await editing.commit();
        await fixture.whenStable();
        action('Undo').click();
        await vi.waitFor(() => expect(editor.document()!.entities[0].transform.position[0]).toBe(0));
        await fixture.whenStable();
        action('Redo').click();
        await vi.waitFor(() => expect(editor.document()!.entities[0].transform.position[0]).toBe(6));
    });

    it('commits an active field before Play, makes authoring read-only, and restores Scene on Stop', async () => {
        await selectRobot();
        await change(input('Position X'), '3');
        await fixture.whenStable();
        // Dispatching click without moving focus also exercises the action barrier.
        action('Play').click();
        await vi.waitFor(() => expect(editor.playing()).toBe(true));
        await fixture.whenStable();
        const types = commands.map(command => command.type);
        expect(types.indexOf('commit_transaction')).toBeLessThan(types.indexOf('start_play'));
        expect(input('Position X').matches(':disabled')).toBe(true);
        expect(fixture.nativeElement.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Game');
        action('Stop').click();
        await vi.waitFor(() => expect(editor.playing()).toBe(false));
        await fixture.whenStable();
        expect(input('Position X').matches(':disabled')).toBe(false);
        expect(input('Position X').valueAsNumber).toBe(3);
        expect(fixture.nativeElement.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Scene');
    });

    it('dispatches hierarchy shortcuts while leaving text and Game keys to their owners', async () => {
        await selectRobot();
        const position = input('Position X');
        await change(position, '6');
        position.blur();
        await editing.commit();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const press = (target: Element, code: string, key: string) => {
            const event = new KeyboardEvent('keydown', {code, key, ctrlKey: true, bubbles: true, cancelable: true});
            target.dispatchEvent(event);
            return event.defaultPrevented;
        };
        expect(press(position, 'KeyZ', 'z')).toBe(false);
        expect(editor.document()!.entities[0].transform.position[0]).toBe(6);
        const tree = root.querySelector('[role="tree"]')!;
        expect(press(tree, 'KeyZ', 'z')).toBe(true);
        await vi.waitFor(() => expect(editor.document()!.entities[0].transform.position[0]).toBe(0));
        await fixture.whenStable();
        expect(press(tree, 'KeyA', 'a')).toBe(true);
        await vi.waitFor(() => expect(editor.selection()).toEqual(['robot', 'crate']));
        const gameTab = Array.from(root.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
            .find(tab => tab.textContent?.trim() === 'Game')!;
        gameTab.click();
        await fixture.whenStable();
        expect(press(root.querySelector('[role="tabpanel"]')!, 'KeyY', 'y')).toBe(false);
        expect(commands.some(command => command.type === 'redo')).toBe(false);
    });

    it('commits the current inspector draft before reparenting and preserves world-space intent', async () => {
        await selectRobot();
        await change(input('Position X'), '5');
        const panel = fixture.debugElement.query(By.directive(SceneEntitiesPanel)).componentInstance as SceneEntitiesPanel;
        panel.reparentRequested.emit({key: 'robot', parent: 'crate'});
        await vi.waitFor(() => expect(editor.document()!.entities[0].parent).toBe('crate'));
        await fixture.whenStable();
        const types = commands.map(command => command.type);
        expect(types.indexOf('commit_transaction')).toBeLessThan(types.indexOf('reparent'));
        expect(commands.find(command => command.type === 'reparent')).toEqual({type: 'reparent', key: 'robot', parent: 'crate', keep_world: true});
        expect(editor.document()!.entities[0].transform.position).toEqual([5, 0, 0]);
        expect(fixture.nativeElement.querySelector('[data-key="robot"]').getAttribute('aria-level')).toBe('2');

        panel.reparentRequested.emit({key: 'robot', parent: null});
        await vi.waitFor(() => expect(editor.document()!.entities[0].parent).toBeNull());
        expect(commands.filter(command => command.type === 'reparent').at(-1)).toEqual({type: 'reparent', key: 'robot', parent: null, keep_world: true});
    });

    it('reports a rejected parent change and clears its error when another project opens', async () => {
        await selectRobot();
        const execute = vi.spyOn(editor, 'execute').mockRejectedValue(new Error('Parent pose is not representable.'));
        const panel = fixture.debugElement.query(By.directive(SceneEntitiesPanel)).componentInstance as SceneEntitiesPanel;
        panel.reparentRequested.emit({key: 'robot', parent: 'crate'});
        await vi.waitFor(() => expect(fixture.nativeElement.querySelector('scene-entities-panel [role="alert"]')?.textContent).toContain('Parent pose is not representable.'));
        expect(editor.document()!.entities[0].parent).toBeUndefined();
        execute.mockRestore();
        await editor.openProject('another/project.json');
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelector('scene-entities-panel [role="alert"]')).toBeNull();
    });

    it('shows measured scene progress until a frame for the accepted scene revision is presented', async () => {
        const target = {id: 'courtyard', asset_path: 'scenes/courtyard.json'};
        nativeState = {
            ...nativeState,
            scenes: [...nativeState.scenes, {...target, dirty: false, loaded: true}],
            project: {...nativeState.project, scenes: [...nativeState.project.scenes, target]},
        };
        await editor.refresh();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const progress = () => root.querySelector('progress');
        const status = () => root.querySelector('.scene-load [role="status"]')?.textContent;
        const update = async (load: ViewportSceneLoad) => {
            viewportStatus = {...viewportStatus, scene_load: load};
            await editor.updateViewportBounds({x: 0, y: 0, width: 800, height: 500, visible: true});
            await fixture.whenStable();
        };
        const previousFrame: ViewportSceneLoad = {
            workspace_generation: 1, scene_id: target.id, scene_revision: 1, stage: 'presented',
        };
        await update(previousFrame);
        const switcher = fixture.debugElement.injector.get(SceneSwitchService);
        expect(await switcher.request(target.id)).toBe(true);
        await fixture.whenStable();
        expect(progress()?.hasAttribute('value')).toBe(false);

        const current = {...previousFrame, scene_revision: editor.snapshot()!.scene_revision};
        await update({...current, stage: 'preparing', completed: 2, total: 4});
        expect(progress()?.value).toBe(2);
        expect(progress()?.max).toBe(4);
        await update({...current, stage: 'rendering'});
        expect(progress()?.hasAttribute('value')).toBe(false);
        expect(status()).toContain('Presenting');
        await update(current);
        expect(progress()).toBeNull();
    });
});

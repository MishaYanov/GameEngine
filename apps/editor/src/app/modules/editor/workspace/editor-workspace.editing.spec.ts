import {Component, input, output} from '@angular/core';
import {type ComponentFixture, TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {type Event as TauriEvent, listen} from '@tauri-apps/api/event';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {EditorActionId} from '../../../core/editor/editor-actions';
import {EditorService} from '../../../core/editor/editor.service';
import type {
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    ModelAsset,
    ProjectData,
    SceneDocument,
    ViewportSceneLoad,
    ViewportStatus
} from '../../../core/editor/editor.models';
import type {LinkedPrefabStatus, PrefabSourceDraft} from '../../../core/editor/linked-prefabs.models';
import {EditorWorkspace} from './editor-workspace';
import {SceneViewport} from './scene-viewport/scene-viewport';
import {SceneEditingService} from './scene-editing.service';
import {SceneEntitiesPanel} from './scene-entities-panel/scene-entities-panel';
import {SceneSwitchService} from './scene-actions/scene-switch.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {
    DEFAULT_VIEWPORT_SETTINGS,
    type PlayStatus,
    type ViewportAction,
    type ViewportEditorState
} from '../../../core/editor/viewport-editor.models';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

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

const gameplayModule = {
    id: 'fixture_game',
    label: 'Fixture Game',
    description: 'A test game with a player role.',
    default_roles: [{name: 'player', label: 'Player', requirement_hint: 'Choose the player.'}],
    scene_roles: {},
};

const folderCapabilities = (['create_folder', 'rename_folder', 'move_folder', 'move_roots_to_folder', 'remove_folder'] as const)
    .map(name => ({name, label: name, category: 'hierarchy', undoable: true, available_in_play: false}));

const propAsset = {kind: 'prefab', asset_id: 'arch-id', path: 'arch.prefab.json', name: 'Arch'} as const;

function propSource(revision: number): PrefabSourceDraft {
    const transform = {position: [0, 0, 0] as const, rotation: [0, 0, 0] as const, scale: [1, 1, 1] as const};
    return {
        draft_id: 'arch-draft', workspace_generation: 1, revision,
        path: propAsset.path, asset_id: propAsset.asset_id, fingerprint: 'arch-fingerprint',
        prefab: {schema_version: 3, gameplay: null, reference_slots: {actor: 'external-actor'}, entities: [
            {key: 'arch', name: 'Arch root', transform, components: []},
            {key: 'stone', name: 'Stone', parent: 'arch', transform, components: []},
        ]},
    };
}

describe('Workspace live editing flow', () => {
    const dialogPrototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(dialogPrototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(dialogPrototype, 'close');

    beforeAll(() => {
        Object.defineProperty(dialogPrototype, 'showModal', {
            configurable: true,
            value(this: HTMLDialogElement) { this.setAttribute('open', ''); },
        });
        Object.defineProperty(dialogPrototype, 'close', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            },
        });
    });

    afterAll(() => {
        if (originalShow) Object.defineProperty(dialogPrototype, 'showModal', originalShow);
        else Reflect.deleteProperty(dialogPrototype, 'showModal');
        if (originalClose) Object.defineProperty(dialogPrototype, 'close', originalClose);
        else Reflect.deleteProperty(dialogPrototype, 'close');
    });

    let fixture: ComponentFixture<EditorWorkspace>;
    let editor: EditorService;
    let editing: SceneEditingService;
    let nativeState: EditorSnapshot;
    let savedDocument: SceneDocument;
    let savedProject: ProjectData;
    let transactionBefore: SceneDocument | null;
    let undo: SceneDocument[];
    let redo: SceneDocument[];
    let commands: EditorCommand[];
    let viewportStatus: ViewportStatus;
    let viewportEditorState: ViewportEditorState;
    let viewportStateListener: ((event: TauriEvent<ViewportEditorState>) => void) | undefined;
    let restartReply: (() => Promise<ViewportEditorState>) | undefined;
    let assets: readonly ModelAsset[];
    let linkedStatus: LinkedPrefabStatus | null;
    let openPropSource: (revision: number) => Promise<PrefabSourceDraft>;

    beforeEach(async () => {
        localStorage.clear();
        nativeState = initialSnapshot();
        savedDocument = structuredClone(nativeState.document);
        savedProject = structuredClone(nativeState.project);
        transactionBefore = null;
        undo = [];
        redo = [];
        commands = [];
        viewportStatus = {
            attached: true, visible: true, width: 800, height: 500,
            draw_calls: 0, instances: 0, triangles: 0, gpu_time_ms: null,
            gpu_timestamps_supported: false, error: null,
        };
        viewportEditorState = {
            settings: DEFAULT_VIEWPORT_SETTINGS, camera_position: [0, 0, 3], camera_target: [0, 0, 0],
            camera_distance: 3, revision: 1, workspace_generation: 1, gesture: null, placement: null,
            focused: false, error: null, game_status: null,
        };
        restartReply = undefined;
        viewportStateListener = undefined;
        assets = [];
        linkedStatus = null;
        openPropSource = async revision => propSource(revision);
        vi.mocked(listen).mockReset().mockImplementation(async (name, handler) => {
            if (name === 'editor://viewport-editor-state') {
                viewportStateListener = handler as (event: TauriEvent<ViewportEditorState>) => void;
            }
            return () => undefined;
        });
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset().mockImplementation(async (operation, args) => {
            if (operation === 'editor_capabilities') return {
                protocol_version: 2, scene_schema_version: 3, incremental_commands: true,
                commands: folderCapabilities, embedded_viewport_supported: true,
                max_selection: 4096, max_history_entries: 128, history_budget_bytes: 16 * 1024 * 1024,
                gameplay_modules: [gameplayModule],
            };
            if (operation === 'editor_open_project') {
                nativeState = {...initialSnapshot(), revision: nativeState.revision + 1,
                    project: structuredClone(savedProject), document: structuredClone(savedDocument)};
                return structuredClone(nativeState);
            }
            if (operation === 'editor_save') {
                expect(nativeState.transaction).toBeNull();
                savedDocument = structuredClone(nativeState.document);
                nativeState = {...nativeState, revision: nativeState.revision + 1, dirty: false};
                return structuredClone(nativeState);
            }
            if (operation === 'editor_save_all') {
                expect(nativeState.transaction).toBeNull();
                savedDocument = structuredClone(nativeState.document);
                savedProject = structuredClone(nativeState.project);
                nativeState = {...nativeState, revision: nativeState.revision + 1, dirty: false, project_dirty: false};
                return structuredClone(nativeState);
            }
            if (operation === 'editor_snapshot') return structuredClone(nativeState);
            if (operation === 'editor_assets') return assets;
            if (operation === 'editor_prefab_source_open') {
                return openPropSource((args as {expectedRevision: number}).expectedRevision);
            }
            if (operation === 'editor_prefab_source_close') return undefined;
            if (operation === 'editor_prop_preview_open') {
                const {requestId, expectedGeneration, expectedRevision} =
                    args as {requestId: number; expectedGeneration: number; expectedRevision: number};
                return {token: 'workspace-prop-preview', workspace_generation: expectedGeneration,
                    revision: expectedRevision, request_id: requestId, selection: null};
            }
            if (operation === 'editor_prop_preview_close' || operation === 'editor_prop_preview_select') return undefined;
            if (operation === 'editor_linked_prefab_status') return linkedStatus;
            if (operation === 'editor_viewport_bounds') return viewportStatus;
            if (operation === 'editor_viewport_editor_state') return viewportEditorState;
            if (operation === 'editor_viewport_action') {
                const {action} = args as { action: ViewportAction };
                if (action.type === 'set_view') {
                    viewportEditorState = {
                        ...viewportEditorState, revision: viewportEditorState.revision + 1,
                        settings: {...viewportEditorState.settings, view: action.view},
                    };
                    return viewportEditorState;
                }
                if (action.type === 'restart_run' && restartReply) return restartReply();
            }
            if (operation !== 'editor_execute_incremental') throw new Error(`Unexpected operation: ${operation}`);
            const {request} = args as {request: EditorCommandRequest};
            expect(request.expected_revision).toBe(nativeState.revision);
            const command = request.command;
            commands.push(command);
            switch (command.type) {
                case 'select': nativeState = {...nativeState, selection: command.keys}; break;
                case 'set_project':
                    expect(nativeState.transaction).toBeNull();
                    nativeState = {...nativeState, project: command.project, project_dirty: true, dirty: true};
                    break;
                case 'set_camera':
                    nativeState = {...nativeState, dirty: true, document: {...nativeState.document, camera: command.camera}};
                    break;
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
                case 'set_collider':
                    if (nativeState.transaction === null) {
                        undo.push(nativeState.document);
                        redo = [];
                    }
                    nativeState = {...nativeState, dirty: true, can_undo: undo.length > 0, document: {...nativeState.document,
                        entities: nativeState.document.entities.map(entity =>
                            entity.key === command.key ? {...entity, collider: command.collider} : entity)}};
                    break;
                case 'reparent':
                    expect(nativeState.transaction).toBeNull();
                    nativeState = {...nativeState, dirty: true, document: {...nativeState.document,
                        entities: nativeState.document.entities.map(entity => entity.key === command.key ? {...entity, parent: command.parent} : entity)}};
                    break;
                case 'move_roots_to_folder': {
                    expect(nativeState.transaction).toBeNull();
                    const organization = nativeState.document.organization!;
                    const membership = {...organization.root_membership};
                    for (const key of command.roots) {
                        if (command.folder === null) delete membership[key];
                        else membership[key] = command.folder;
                    }
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, organization: {...organization, root_membership: membership}}};
                    break;
                }
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
                    nativeState = {...nativeState, mode: 'play', play_session_id: `play-${nativeState.revision + 1}`};
                    break;
                case 'stop_play':
                    nativeState = {...nativeState, mode: 'edit', play_session_id: null};
                    break;
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

    it('reveals the source of a clicked entity usage and reports a broken reference after the scene changes', async () => {
        const setParent = async (parent: string) => {
            nativeState = {
                ...nativeState,
                revision: nativeState.revision + 1,
                scene_revision: nativeState.scene_revision + 1,
                document: {
                    ...nativeState.document, entities: nativeState.document.entities.map(entity =>
                        entity.key === 'crate' ? {...entity, parent} : entity)
                },
            };
            await editor.refresh();
            await fixture.whenStable();
        };
        await setParent('robot');
        const hierarchy = fixture.debugElement.query(By.directive(SceneEntitiesPanel)).injector.get(SceneEntitiesPanel);
        hierarchy.usagesRequested.emit('robot');
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const usage = root.querySelector<HTMLElement>('editor-find-usages [role="treeitem"][aria-level="2"]')!;
        expect(usage.textContent).toContain('crate · Parent');
        usage.click();
        await vi.waitFor(() => expect(editor.selection()).toEqual(['crate']));
        await fixture.whenStable();
        expect(root.querySelector('editor-find-usages dialog[open]')).toBeNull();

        await setParent('removed');
        const button = root.querySelector<HTMLButtonElement>('.problems-button')!;
        button.click();
        await fixture.whenStable();
        const problem = root.querySelector<HTMLButtonElement>('editor-problems-panel dialog[open] li button')!;
        expect(problem.textContent).toContain('Entity “removed” does not exist in this scene.');
        problem.click();
        await fixture.whenStable();
        expect(root.querySelector('editor-problems-panel dialog[open]')).toBeNull();
        expect(document.activeElement).toBe(root.querySelector('.entity-list'));
        await setParent('robot');
        button.click();
        await fixture.whenStable();
        expect(root.querySelector('editor-problems-panel dialog[open]')?.textContent).toContain('No reference problems');
    });

    it('opens Scene Information and focuses a gameplay role from Find Usages or Problems', async () => {
        await editor.loadCapabilities();
        const root: HTMLElement = fixture.nativeElement;
        const setPlayer = async (key: string) => {
            nativeState = {
                ...nativeState,
                revision: nativeState.revision + 1,
                scene_revision: nativeState.scene_revision + 1,
                project: {...nativeState.project, gameplay_module: gameplayModule.id},
                document: {...nativeState.document, bindings: {player: key}},
            };
            await editor.refresh();
            await fixture.whenStable();
        };
        const sceneDialog = () => root.querySelector<HTMLDialogElement>('editor-scene-actions dialog[open]');
        const focusedRole = () => expect(document.activeElement)
            .toBe(sceneDialog()?.querySelector('[data-scene-binding="player"]'));

        await setPlayer('robot');
        const hierarchy = fixture.debugElement.query(By.directive(SceneEntitiesPanel)).injector.get(SceneEntitiesPanel);
        hierarchy.usagesRequested.emit('robot');
        await fixture.whenStable();
        const usage = root.querySelector<HTMLElement>('editor-find-usages [role="treeitem"][aria-level="2"]')!;
        expect(usage.textContent).toContain('Scene · player');
        usage.click();
        await fixture.whenStable();
        expect(root.querySelector('editor-find-usages dialog[open]')).toBeNull();
        expect(sceneDialog()?.querySelector('h2')?.textContent?.trim()).toBe('Scene Information');
        focusedRole();

        sceneDialog()!.close();
        await setPlayer('missing');
        root.querySelector<HTMLButtonElement>('.problems-button')!.click();
        await fixture.whenStable();
        const problem = root.querySelector<HTMLButtonElement>('editor-problems-panel dialog[open] li button')!;
        expect(problem.textContent).toContain('Entity “missing” does not exist in this scene.');
        problem.click();
        await fixture.whenStable();
        expect(root.querySelector('editor-problems-panel dialog[open]')).toBeNull();
        focusedRole();
        expect(commands).toEqual([]);
    });

    it('opens the palette from the hierarchy shortcut and dispatches Save Scene through the existing toolbar', async () => {
        await selectRobot();
        const name = fixture.nativeElement.querySelector('inspector-panel input[type="text"]') as HTMLInputElement;
        await change(name, 'Robot renamed');
        await editing.commit();
        const root: HTMLElement = fixture.nativeElement;
        const tree = root.querySelector<HTMLElement>('.entity-list')!;
        tree.focus();
        const shortcut = new KeyboardEvent('keydown', {
            key: 'P', code: 'KeyP', ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true,
        });
        tree.dispatchEvent(shortcut);
        await fixture.whenStable();
        expect(shortcut.defaultPrevented).toBe(true);
        const palette = root.querySelector<HTMLDialogElement>('editor-command-palette dialog[open]')!;
        const search = palette.querySelector<HTMLInputElement>('input[role="combobox"]')!;
        search.value = 'save scene';
        search.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        palette.querySelector<HTMLElement>('[role="option"]')!.click();
        await vi.waitFor(() => expect(editor.dirty()).toBe(false));
        expect(savedDocument.entities.find(entity => entity.key === 'robot')?.name).toBe('Robot renamed');
        expect(palette.open).toBe(false);
    });

    it('applies game bindings, saves the project and reopens with the authored profile before Play', async () => {
        const inputManagerButton = () => fixture.nativeElement.querySelector('editor-game-input > button') as HTMLButtonElement;
        inputManagerButton().click();
        await fixture.whenStable();
        const forward: HTMLSelectElement = fixture.nativeElement.querySelectorAll('editor-game-input .movement-keys select')[3];
        forward.value = 'arrow_up';
        forward.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        const apply: HTMLButtonElement = [...fixture.nativeElement.querySelectorAll('editor-game-input button') as NodeListOf<HTMLButtonElement>]
            .find(button => button.textContent?.trim() === 'Apply Bindings')!;
        apply.click();
        await fixture.whenStable();
        expect(editor.snapshot()?.project.input_profile?.movement.positive_y).toBe('arrow_up');
        expect(editor.snapshot()?.project_dirty).toBe(true);
        const dialog: HTMLDialogElement = fixture.nativeElement.querySelector('editor-game-input dialog');
        dialog.close();
        await fixture.whenStable();
        action('Save All').click();
        await fixture.whenStable();
        expect(editor.dirty()).toBe(false);
        await editor.openProject('fixture/project.json');
        await fixture.whenStable();
        expect(editor.snapshot()?.project.input_profile?.movement.positive_y).toBe('arrow_up');
        inputManagerButton().click();
        await fixture.whenStable();
        expect((fixture.nativeElement.querySelectorAll('editor-game-input .movement-keys select')[3] as HTMLSelectElement).value).toBe('arrow_up');
        (fixture.nativeElement.querySelector('editor-game-input dialog') as HTMLDialogElement).close();
        await fixture.whenStable();
        action('Play').click();
        await fixture.whenStable();
        expect(editor.playing()).toBe(true);
        expect(editor.snapshot()?.project.input_profile?.movement.positive_y).toBe('arrow_up');
    });

    it('authors and persists follow camera settings through the real editor command queue', async () => {
        const open = () => (fixture.nativeElement.querySelector('editor-camera-settings > button') as HTMLButtonElement).click();
        open();
        await fixture.whenStable();
        const mode: HTMLSelectElement = fixture.nativeElement.querySelector('editor-camera-settings select');
        mode.value = 'follow';
        mode.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        const apply = [...fixture.nativeElement.querySelectorAll('editor-camera-settings button') as NodeListOf<HTMLButtonElement>]
            .find(button => button.textContent.trim() === 'Apply Game Camera')!;
        apply.click();
        await fixture.whenStable();
        expect(editor.document()?.camera.follow).toEqual({
            target: 'robot', offset: [0, 2, 5], look_offset: [0, 0, 0], smoothing_seconds: 0.15,
        });
        (fixture.nativeElement.querySelector('editor-camera-settings dialog') as HTMLDialogElement).close();
        await fixture.whenStable();
        action('Save Scene').click();
        await fixture.whenStable();
        await editor.openProject('fixture/project.json');
        await fixture.whenStable();
        expect(editor.document()?.camera.follow?.target).toBe('robot');
        open();
        await fixture.whenStable();
        expect((fixture.nativeElement.querySelector('editor-camera-settings select') as HTMLSelectElement).value).toBe('follow');
    });

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

    it('authors collider shapes and values through history, saving and reopening', async () => {
        await selectRobot();
        const shape = () => fixture.nativeElement.querySelector('editor-collider-inspector select') as HTMLSelectElement;
        const choose = async (kind: string) => {
            shape().value = kind;
            shape().dispatchEvent(new Event('change'));
            await fixture.whenStable();
        };
        await choose('box');
        expect(editor.document()!.entities[0].collider?.kind).toBe('box');
        await change(input('Collider Offset X'), '1.1');
        input('Collider Offset X').blur();
        await editing.commit();
        await fixture.whenStable();
        expect(input('Collider Offset X').value).toBe('1.1');
        action('Undo').click();
        await fixture.whenStable();
        expect(input('Collider Offset X').value).toBe('0');
        action('Redo').click();
        await fixture.whenStable();
        expect(input('Collider Offset X').value).toBe('1.1');
        await choose('sphere');
        expect(editor.document()!.entities[0].collider).toEqual({kind: 'sphere', radius: 0.5, offset: [1.1, 0, 0]});
        await choose('none');
        expect(editor.document()!.entities[0].collider).toBeNull();
        action('Save Scene').click();
        await fixture.whenStable();
        await editor.openProject('fixture/project.json');
        await selectRobot();
        expect(shape().value).toBe('none');
        expect(fixture.nativeElement.querySelector('[aria-label="Collider Radius"]')).toBeNull();
    });

    it('discards the whole collider gesture when its final numeric draft is incomplete', async () => {
        await selectRobot();
        await editing.replaceCollider('sphere');
        await fixture.whenStable();
        await change(input('Collider Radius'), '2');
        await change(input('Collider Radius'), '1e-');
        expect(input('Collider Radius').getAttribute('aria-invalid')).toBe('true');
        input('Collider Radius').blur();
        await editing.commit();
        await fixture.whenStable();
        expect(editor.document()!.entities[0].collider).toEqual({kind: 'sphere', radius: 0.5, offset: [0, 0, 0]});
        expect(input('Collider Radius').value).toBe('0.5');
        expect(commands.filter(command => command.type === 'commit_transaction')).toHaveLength(0);
        expect(commands.filter(command => command.type === 'cancel_transaction')).toHaveLength(1);
        expect(fixture.nativeElement.querySelector('.edit-error')).toBeNull();
    });

    it('commits the previous field before atomically changing the collider shape', async () => {
        await selectRobot();
        await editing.replaceCollider('sphere');
        await fixture.whenStable();
        const start = commands.length;
        await change(input('Collider Radius'), '2');
        const shape: HTMLSelectElement = fixture.nativeElement.querySelector('editor-collider-inspector select');
        shape.value = 'box';
        shape.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(commands.slice(start).map(command => command.type)).toEqual([
            'begin_transaction', 'set_collider', 'commit_transaction', 'set_collider',
        ]);
        expect(editor.document()!.entities[0].collider?.kind).toBe('box');
    });

    it('edits sensor mode and collision layers through Undo, saving and reopening', async () => {
        await selectRobot();
        await editing.replaceCollider('sphere');
        await fixture.whenStable();
        const mode = () => fixture.nativeElement.querySelectorAll('editor-collider-inspector select')[1] as HTMLSelectElement;
        const layer = () => fixture.nativeElement.querySelector('[aria-label="Belongs to layer 32"]') as HTMLInputElement;
        mode().value = 'sensor';
        mode().dispatchEvent(new Event('change'));
        await fixture.whenStable();
        layer().click();
        await fixture.whenStable();
        expect(editor.document()!.entities[0].collider?.interaction).toEqual({
            sensor: true, layers: 0x80000001, mask: 0xffffffff,
        });
        action('Undo').click();
        await fixture.whenStable();
        expect(layer().checked).toBe(false);
        expect(mode().value).toBe('sensor');
        action('Redo').click();
        await fixture.whenStable();
        expect(layer().checked).toBe(true);
        action('Save Scene').click();
        await fixture.whenStable();
        await editor.openProject('fixture/project.json');
        await selectRobot();
        expect(mode().value).toBe('sensor');
        expect(layer().checked).toBe(true);
    });

    it('clears a whole detection mask with one undoable UI action', async () => {
        await selectRobot();
        await editing.replaceCollider('sphere');
        await editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        await fixture.whenStable();
        const original = editor.document()!.entities[0].collider;
        const before = commands.length;
        const clear: HTMLButtonElement = fixture.nativeElement.querySelector('[aria-label="Detect no layers"]');
        clear.click();
        await fixture.whenStable();
        expect(commands.slice(before).map(command => command.type)).toEqual(['set_collider']);
        expect(editor.document()!.entities[0].collider?.interaction?.mask).toBe(0);
        expect(fixture.nativeElement.textContent).toContain('Collision and overlap detection are disabled');
        action('Undo').click();
        await fixture.whenStable();
        expect(editor.document()!.entities[0].collider).toEqual(original);
        expect(fixture.nativeElement.textContent).not.toContain('Collision and overlap detection are disabled');
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
        expect(fixture.nativeElement.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Game');
        expect(fixture.nativeElement.querySelector('editor-runtime-inspector')).not.toBeNull();
        expect(fixture.nativeElement.querySelector('input[aria-label="Position X"]')).toBeNull();
        const workspaceRoot: HTMLElement = fixture.nativeElement;
        const sceneTab = Array.from(workspaceRoot.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
            .find(tab => tab.textContent?.trim() === 'Scene')!;
        sceneTab.click();
        await fixture.whenStable();
        expect(input('Position X').matches(':disabled')).toBe(true);
        action('Stop').click();
        await vi.waitFor(() => expect(editor.playing()).toBe(false));
        await fixture.whenStable();
        expect(input('Position X').matches(':disabled')).toBe(false);
        expect(input('Position X').valueAsNumber).toBe(3);
        expect(fixture.nativeElement.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Scene');
    });

    it('displays status for the current Play session and restarts only after native confirmation', async () => {
        const viewport = fixture.debugElement.injector.get(ViewportEditorService);
        await viewport.start();
        await editor.updateViewportBounds({x: 0, y: 0, width: 800, height: 500, visible: true});
        action('Play').click();
        await fixture.whenStable();
        const snapshot = editor.snapshot()!;
        const root: HTMLElement = fixture.nativeElement;
        const panel = root.querySelector<HTMLElement>('.game-status')!;
        const restart = panel.querySelector<HTMLButtonElement>('button')!;
        expect(restart.textContent).toContain('Restart Run');
        expect(restart.disabled).toBe(true);

        const sendStatus = async (status: PlayStatus) => {
            viewportEditorState = {
                ...viewportEditorState,
                revision: viewportEditorState.revision + 1,
                game_status: status
            };
            viewportStateListener!({
                event: 'editor://viewport-editor-state', id: 1, payload: viewportEditorState,
            });
            await fixture.whenStable();
        };
        const completed: PlayStatus = {
            play_session_id: snapshot.play_session_id!,
            run_id: 'run-1',
            scene_generation: 1,
            scene_id: 'finish',
            paused: false,
            pending_scene_id: null,
            gameplay: {objective: 'Run complete', detail: 'Finish reached', score: 1, completed: true},
        };
        await sendStatus({...completed, play_session_id: 'old-session'});
        expect(panel.textContent).not.toContain('Run complete');
        expect(restart.disabled).toBe(true);
        await sendStatus(completed);
        expect(panel.textContent).toContain('Runtime scene: finish');
        expect(panel.textContent).toContain('Completed');
        expect(panel.querySelector('[role="status"]')?.textContent).toContain('Run complete');
        expect(panel.textContent).toContain('Finish reached');
        expect(panel.textContent).toContain('Score: 1');
        expect(restart.disabled).toBe(false);
        const accessibility = await axe.run(panel, {rules: {'color-contrast': {enabled: false}}});
        expect(accessibility.violations).toEqual([]);

        restartReply = async () => {
            throw new Error('Startup scene could not be loaded.');
        };
        restart.click();
        await fixture.whenStable();
        expect(root.querySelector('.game-error')?.textContent).toContain('Startup scene could not be loaded');
        expect(panel.textContent).toContain('Completed');
        expect(restart.disabled).toBe(false);

        let acceptRestart!: () => void;
        const restarted: PlayStatus = {
            ...completed, run_id: 'run-2', scene_id: 'courtyard', scene_generation: 0,
            gameplay: {objective: 'Find the key', detail: null, score: 0, completed: false},
        };
        restartReply = () => new Promise(resolve => {
            acceptRestart = () => {
                viewportEditorState = {
                    ...viewportEditorState,
                    revision: viewportEditorState.revision + 1,
                    game_status: restarted
                };
                resolve(viewportEditorState);
            };
        });
        restart.click();
        await vi.waitFor(() => expect(acceptRestart).toBeDefined());
        expect(invoke).toHaveBeenLastCalledWith('editor_viewport_action', {
            action: {type: 'restart_run', run_id: 'run-1', expected_revision: snapshot.revision},
        });
        await fixture.whenStable();
        expect(restart.disabled).toBe(true);
        expect(panel.textContent).toContain('Completed');
        acceptRestart();
        await vi.waitFor(() => expect(viewport.gameStatus()?.run_id).toBe('run-2'));
        await fixture.whenStable();
        expect(panel.textContent).toContain('Runtime scene: courtyard');
        expect(panel.textContent).toContain('Running');
        expect(panel.textContent).toContain('Score: 0');
        expect(root.querySelector('.game-error')).toBeNull();
        expect(editor.snapshot()).toBe(snapshot);
        expect(invoke).not.toHaveBeenCalledWith('editor_snapshot');

        await sendStatus({...restarted, paused: true});
        expect(panel.textContent).toContain('Paused');
        action('Stop').click();
        await fixture.whenStable();
        expect(root.querySelector('.game-status')).toBeNull();
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

    it('organizes accepted root selections through the live hierarchy controls and restores placement on undo', async () => {
        nativeState = {...nativeState, document: {...nativeState.document, schema_version: 6, organization: {
            folders: [{id: 'floor', name: 'Floor'}], root_membership: {},
        }}};
        await editor.loadCapabilities();
        await editor.refresh();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const robot = root.querySelector<HTMLButtonElement>('[data-key="robot"]')!;
        const tree = root.querySelector<HTMLElement>('[role="tree"]')!;
        robot.click();
        await vi.waitFor(() => expect(editor.selection()).toEqual(['robot']));
        tree.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowDown', shiftKey: true, bubbles: true, cancelable: true}));
        await vi.waitFor(() => expect(editor.selection()).toEqual(['robot', 'crate']));
        const move = [...root.querySelectorAll<HTMLButtonElement>('.hierarchy-actions button')]
            .find(button => button.textContent?.trim() === 'Move to Folder…')!;
        move.click();
        await fixture.whenStable();
        root.querySelector('hierarchy-folder-editor form')!
            .dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        await vi.waitFor(() => expect(editor.document()?.organization?.root_membership).toEqual({robot: 'floor', crate: 'floor'}));
        await fixture.whenStable();
        expect(commands.at(-1)).toEqual({type: 'move_roots_to_folder', roots: ['robot', 'crate'], folder: 'floor'});
        expect(robot.getAttribute('aria-level')).toBe('2');
        expect(editor.document()?.entities[0].parent).toBeUndefined();
        await editor.execute({type: 'undo'});
        await fixture.whenStable();
        expect(robot.getAttribute('aria-level')).toBe('1');
        expect(editor.selection()).toEqual(['robot', 'crate']);
    });

    it('opens a source Prop from Assets, keeps the same viewport mounted, and restores the source of focus', async () => {
        await selectRobot();
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const viewport = root.querySelector('scene-viewport');
        const asset = root.querySelector<HTMLButtonElement>('[data-asset-id="arch-id"]')!;
        asset.focus();
        asset.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(root.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Prop'));
        await fixture.whenStable();
        const panel = root.querySelector<HTMLElement>('editor-prop-workspace')!;
        expect(root.querySelector('scene-viewport')).toBe(viewport);
        expect(viewport?.hasAttribute('hidden')).toBe(false);
        expect(fixture.debugElement.query(By.directive(ViewportStub)).componentInstance.label())
            .toBe('Isolated Prop viewport');
        expect(panel.textContent).toContain('arch.prefab.json');
        panel.querySelector<HTMLButtonElement>('[data-prop-object="stone"]')!.click();
        await fixture.whenStable();
        expect(panel.querySelector('[aria-label="Prop Inspector"]')?.textContent).toContain('Stone');
        expect(editor.selection()).toEqual(['robot']);
        Array.from(panel.querySelectorAll('button'))
            .find(button => button.textContent?.trim() === 'Close Prop')!.click();
        await fixture.whenStable();
        expect(root.querySelector('editor-prop-workspace')).toBeNull();
        expect(root.querySelector('scene-viewport')).toBe(viewport);
        expect(document.activeElement).toBe(asset);
        expect(invoke).toHaveBeenCalledWith('editor_prefab_source_close', {draftId: 'arch-draft'});
        expect(invoke).toHaveBeenCalledWith('editor_prop_preview_close', {token: 'workspace-prop-preview'});
    });

    it('opens a pinned instance with unavailable source and restores its read-only tab after Play', async () => {
        const prefab = {source: {source_id: 'missing-id', path: 'missing.json'}, bindings: {actor: 'crate'}};
        nativeState = {...nativeState, document: {...nativeState.document, entities: nativeState.document.entities.map(entity =>
            entity.key === 'robot' ? {...entity, linked_prefab: prefab, component_views: []} : {...entity, parent: 'robot'})}};
        linkedStatus = {root_key: 'robot', source_id: 'missing-id', source_path: 'missing.json', source_fingerprint: 'stored',
            source_changed: false, source_error: 'Source file missing', bindings: prefab.bindings, overrides: []};
        await editor.refresh();
        await fixture.whenStable();
        await selectRobot();
        await vi.waitFor(() => expect(fixture.nativeElement.textContent).toContain('Source file missing'));
        const root: HTMLElement = fixture.nativeElement;
        const open = [...root.querySelectorAll<HTMLButtonElement>('editor-linked-prefab-inspector button')]
            .find(button => button.textContent?.trim() === 'Open Prop Workspace')!;
        open.click();
        await vi.waitFor(() => expect(root.querySelector('editor-prop-workspace')?.textContent).toContain('Pinned Prop instance'));
        const propText = () => root.querySelector('editor-prop-workspace')?.textContent;
        expect(propText()).toContain('missing.json');
        expect(propText()).toContain('crate');
        await editing.select('crate');
        await fixture.whenStable();
        expect(propText()).toContain('Pinned Prop instance');
        action('Play').click();
        await vi.waitFor(() => expect(root.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Game'));
        expect(root.querySelector('editor-prop-workspace')).toBeNull();
        action('Stop').click();
        await vi.waitFor(() => expect(propText()).toContain('Pinned Prop instance'));
        expect(editor.selection()).toEqual(['crate']);
        expect(invoke).not.toHaveBeenCalledWith('editor_prefab_source_open', expect.anything());
    });

    it('closes a late source reply when opening another project without mounting its Prop tab', async () => {
        let resolveDraft!: (draft: PrefabSourceDraft) => void;
        openPropSource = () => new Promise(resolve => (resolveDraft = resolve));
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(resolveDraft).toBeTypeOf('function'));
        fixture.componentRef.setInput('opening', true);
        fixture.detectChanges();
        const nextProject = editor.openProject('other/project.json');
        resolveDraft(propSource(nativeState.revision));
        await nextProject;
        fixture.componentRef.setInput('opening', false);
        await fixture.whenStable();
        expect(invoke).toHaveBeenCalledWith('editor_prefab_source_close', {draftId: 'arch-draft'});
        expect(root.querySelector('editor-prop-workspace')).toBeNull();
        expect(root.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Scene');
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

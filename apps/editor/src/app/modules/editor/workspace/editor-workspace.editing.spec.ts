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
    CameraComponentSettings,
    Collider,
    ModelAsset,
    ProjectData,
    SceneDocument,
    ViewportSceneLoad,
    ViewportStatus
} from '../../../core/editor/editor.models';
import type {ComponentRecord} from '../../../core/editor/editor-components';
import type {SceneIssue} from '../../../core/editor/scene-verification.models';
import type {CharacterControllerSettings} from '../../../core/editor/character-controller.models';
import type {PlayerControllerSettings} from '../../../core/editor/player-controller.models';
import type {InputActionMap, InputActionsAsset, InputActionsCapture, InputActionsDocument} from '../../../core/game-input/input-actions.models';
import type {AnimatorAsset, AnimatorAssetCapture, AnimatorDocument, AnimatorGraph} from '../../../core/editor/animator.models';
import type {ClipAssetDocument} from '../../../core/editor/authored-clip.models';
import type {LinkedPrefabStatus, NestedPropPlacementRequest, PrefabSourceDraft, PropDraftEdit} from '../../../core/editor/linked-prefabs.models';
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
import {ContextMenuService} from '../../../shared/components/context-menu/context-menu.service';
import {AnimationWorkspaceService} from './animation/animation-workspace.service';

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
const cameraCapabilities = (['create_camera', 'align_camera_to_scene_view', 'set_default_game_camera'] as const)
    .map(name => ({name, label: name, category: 'camera', undoable: true, available_in_play: false}));
const cameraSettings: CameraComponentSettings = {
    lens: {vertical_fov: Math.fround(Math.PI / 3), near: 0.1, far: 100},
    rig: {mode: 'fixed'},
};
const playerCapabilities = (['create_player', 'set_local_player'] as const)
    .map(name => ({name, label: name, category: 'player', undoable: true, available_in_play: false}));
const playerSettings: PlayerControllerSettings = {input: {kind: 'project_default'}, space: 'camera_relative', local: false};
const playerCollider: Collider = {kind: 'box', half_extents: [0.35, 0.9, 0.35], offset: [0, 0, 0]};
const playerMotor: CharacterControllerSettings = {
    walk_speed: 4, ground_acceleration: 30, air_acceleration: 10, gravity: 20, max_fall_speed: 30,
    jump_speed: 7, ground_probe: 0.02, step_height: 0.25, coyote_ticks: 6, jump_buffer_ticks: 6, jump_action: 'jump',
};
const controlsAssetId = '00000000-0000-0000-0000-000000000321';
const animatorAssetId = '00000000-0000-0000-0000-000000000914';
const clipAssetId = '00000000-0000-0000-0000-000000000922';
const rigPropId = '00000000-0000-0000-0000-000000000925';
const clipFingerprint = 'c'.repeat(64);
const authoredClip: ClipAssetDocument = {
    workspace_generation: 1, asset: {asset_id: clipAssetId, path: 'blade.animation.json', fingerprint: clipFingerprint, revision: 1},
    clip: {schema_version: 1, name: 'Blade', owner: {kind: 'prop', asset_id: rigPropId}, duration_ticks: 500, tracks: [{
        target: {kind: 'prop', address: {kind: 'object', placement_path: [], source_entity: 'blade'}},
        track: {kind: 'local_position', interpolation: 'linear', keys: [{at: 0, value: [0, 0, 0]}]},
    }]},
};
const animatorClip = {kind: 'authored', asset_id: clipAssetId, expected_sha256: clipFingerprint} as const;
const walkGraph: AnimatorGraph = {
    schema_version: 1, entry: 'idle', states: [
        {id: 'idle', name: 'Idle', clip: animatorClip, looping: true},
        {id: 'walk', name: 'Walk', clip: animatorClip, looping: true},
    ],
    parameters: [{id: 'moving', name: 'Is moving', source: {kind: 'moving', minimum_mm_per_second: 100}}],
    transitions: [
        {id: 'to_walk', from: 'idle', to: 'walk', condition: {kind: 'bool', parameter: 'moving', value: true}, blend_ticks: 6},
        {id: 'to_idle', from: 'walk', to: 'idle', condition: {kind: 'bool', parameter: 'moving', value: false}, blend_ticks: 6},
    ],
};

const propAsset = {kind: 'prefab', asset_id: 'arch-id', path: 'arch.prefab.json', name: 'Arch'} as const;
const nestedAsset = {kind: 'prefab', asset_id: '00000000-0000-0000-0000-000000000001',
    path: 'grate.prefab.json', name: 'Grate', fingerprint: 'a'.repeat(64)} as const;

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

function nestedSourceFixture(revision: number, path: string): PrefabSourceDraft {
    const draft = {...propSource(revision), draft_revision: 1, dirty: false, can_undo: false, can_redo: false};
    if (path === nestedAsset.path) {
        return {...draft, path, draft_id: 'grate-draft', asset_id: nestedAsset.asset_id, fingerprint: nestedAsset.fingerprint};
    }
    const target = {asset_id: nestedAsset.asset_id, fingerprint: 'b'.repeat(64)};
    const local = draft.prefab.entities[0].transform;
    return {...draft, prefab: {...draft.prefab, schema_version: 4, composition: {
        pins: [{version: target, path: 'old-grate.prefab.json', entities: draft.prefab.entities}],
        placements: ['left', 'right'].map(id => ({id, name: id, parent: 'arch', target, local})),
    }}};
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
    let openPropSource: (revision: number, path: string) => Promise<PrefabSourceDraft>;
    let propDraft: PrefabSourceDraft | null;
    let propDrafts: Map<string, PrefabSourceDraft>;
    let initialPropDrafts: Map<string, PrefabSourceDraft>;
    let propDraftListener: ((event: TauriEvent<PrefabSourceDraft>) => void) | undefined;
    let previousProp: PrefabSourceDraft['prefab'] | null;
    let nextProp: PrefabSourceDraft['prefab'] | null;
    let savePropFailure: string | null;
    let nestedPropFailure: string | null;
    let inputActionsSupported: boolean;
    let inputActions: Map<string, InputActionsDocument>;
    let animatorSupported: boolean;
    let animators: Map<string, AnimatorDocument>;
    let verificationIssues: readonly SceneIssue[] | null;
    let incrementalSelection: boolean;

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
        propDraft = null;
        propDrafts = new Map();
        initialPropDrafts = new Map();
        propDraftListener = undefined;
        previousProp = null;
        nextProp = null;
        savePropFailure = null;
        nestedPropFailure = null;
        inputActionsSupported = false;
        inputActions = new Map();
        animatorSupported = false;
        animators = new Map();
        verificationIssues = null;
        incrementalSelection = false;
        vi.mocked(listen).mockReset().mockImplementation(async (name, handler) => {
            if (name === 'editor://viewport-editor-state') {
                viewportStateListener = handler as (event: TauriEvent<ViewportEditorState>) => void;
            }
            if (name === 'editor://prop-draft-changed') {
                propDraftListener = handler as (event: TauriEvent<PrefabSourceDraft>) => void;
            }
            return () => undefined;
        });
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset().mockImplementation(async (operation, args) => {
            if (operation === 'editor_capabilities') return {
                protocol_version: 2, scene_schema_version: 3, incremental_commands: true,
                commands: [...folderCapabilities, ...cameraCapabilities, ...playerCapabilities], embedded_viewport_supported: true,
                component_types: [
                    {type_id: 'engine.camera', kind: 'camera', label: 'Camera', description: 'Authored camera', max_per_object: 1},
                    {type_id: 'engine.player_controller', kind: 'player_controller', label: 'Player Controller', description: 'Local input', max_per_object: 1},
                    {type_id: 'engine.character_controller', kind: 'character_controller', label: 'Character Controller', description: 'Physics motor', max_per_object: 1},
                    ...(animatorSupported ? [{type_id: 'engine.animator', kind: 'animator', label: 'Animator',
                        description: 'Graph playback', max_per_object: 1}] : []),
                ],
                max_selection: 4096, max_history_entries: 128, history_budget_bytes: 16 * 1024 * 1024,
                gameplay_modules: [gameplayModule],
                input_action_assets: inputActionsSupported,
                animator_assets: animatorSupported,
                scene_verification_supported: verificationIssues !== null,
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
            if (operation === 'editor_check_scenes') return {
                generation: viewportEditorState.workspace_generation,
                revision: nativeState.revision,
                active_scene_id: nativeState.active_scene_id,
                scenes: nativeState.scenes.map(scene => ({
                    id: scene.id, content_stamp: '0000000000000000-1111111111111111', checked: true,
                })),
                issues: verificationIssues ?? [],
            };
            if (operation === 'editor_assets') return assets;
            if (operation === 'editor_animation_clips') return [authoredClip.asset];
            if (operation === 'editor_open_authored_clip') return authoredClip;
            if (operation === 'editor_list_animators' || operation === 'editor_open_animator' ||
                operation === 'editor_save_animator' || operation === 'editor_validate_animator_actor') {
                const request = args as {expectedGeneration: number; expectedRevision: number;
                    assetId?: string; destination?: string; capture?: AnimatorAssetCapture | null; graph?: AnimatorGraph; entityKey?: string};
                expect(request.expectedGeneration).toBe(viewportEditorState.workspace_generation);
                expect(request.expectedRevision).toBe(nativeState.revision);
                if (operation === 'editor_list_animators') return [...animators.values()].map(document => document.asset);
                if (operation === 'editor_open_animator') return animators.get(request.assetId!);
                if (operation === 'editor_validate_animator_actor') {
                    const attachment = nativeState.document.entities.find(entity => entity.key === request.entityKey)?.animator;
                    const graph = animators.get(attachment!.settings.asset_id)!.graph;
                    return {entry: graph.entry, states: graph.states.length, parameters: graph.parameters.length,
                        runtime_digest: 'd'.repeat(64)};
                }
                const old = request.capture ? animators.get(request.capture.id) : undefined;
                expect(old?.asset.fingerprint ?? null).toBe(request.capture?.fingerprint ?? null);
                const revision = (old?.asset.revision ?? 0) + 1;
                const asset: AnimatorAsset = {id: old?.asset.id ?? animatorAssetId, path: request.destination!, kind: 'animator_graph',
                    revision, fingerprint: revision.toString(16).repeat(64), bytes: 1024};
                const document = {asset, graph: request.graph!};
                animators.set(asset.id, document);
                return document;
            }
            if (operation === 'editor_list_input_actions' || operation === 'editor_open_input_actions' ||
                operation === 'editor_save_input_actions' || operation === 'editor_move_input_actions') {
                const request = args as {
                    expectedGeneration: number; expectedRevision: number; assetId?: string;
                    destination?: string; capture?: InputActionsCapture | null; map?: InputActionMap;
                };
                expect(request.expectedGeneration).toBe(viewportEditorState.workspace_generation);
                expect(request.expectedRevision).toBe(nativeState.revision);
                if (operation === 'editor_list_input_actions') return [...inputActions.values()].map(document => document.asset);
                if (operation === 'editor_open_input_actions') return inputActions.get(request.assetId!);
                const old = request.capture ? inputActions.get(request.capture.id) : undefined;
                expect(old?.asset.fingerprint ?? null).toBe(request.capture?.fingerprint ?? null);
                const revision = (old?.asset.revision ?? 0) + 1;
                const asset: InputActionsAsset = {id: old?.asset.id ?? controlsAssetId, path: request.destination!,
                    revision, kind: 'input_actions', fingerprint: operation === 'editor_move_input_actions' ? old!.asset.fingerprint :
                        revision.toString(16).repeat(64), bytes: 1024};
                const document = {asset, map: request.map ?? old!.map};
                inputActions.set(asset.id, document);
                return operation === 'editor_move_input_actions' ? asset : document;
            }
            if (operation === 'editor_prefab_source_open') {
                const {path, expectedRevision} = args as {path: string; expectedRevision: number};
                propDraft = await openPropSource(expectedRevision, path);
                propDrafts.set(propDraft.draft_id, propDraft);
                initialPropDrafts.set(propDraft.draft_id, propDraft);
                return propDraft;
            }
            if (operation === 'editor_prop_draft_edit') {
                const {draftId, expectedDraftRevision, edit} = args as {draftId: string; expectedDraftRevision: number; edit: PropDraftEdit};
                const draft = propDrafts.get(draftId)!;
                expect(draft.draft_id).toBe(draftId);
                expect(draft.draft_revision).toBe(expectedDraftRevision);
                let prefab = draft.prefab;
                switch (edit.type) {
                    case 'apply':
                        previousProp = prefab;
                        nextProp = null;
                        prefab = edit.prefab;
                        break;
                    case 'undo':
                        nextProp = prefab;
                        prefab = previousProp!;
                        previousProp = null;
                        break;
                    case 'redo':
                        previousProp = prefab;
                        prefab = nextProp!;
                        nextProp = null;
                        break;
                    case 'discard':
                        prefab = initialPropDrafts.get(draftId)!.prefab;
                        previousProp = null;
                        nextProp = null;
                        break;
                }
                propDraft = {...draft, prefab, draft_revision: expectedDraftRevision + 1,
                    dirty: edit.type !== 'discard', can_undo: previousProp !== null, can_redo: nextProp !== null};
                propDrafts.set(draftId, propDraft);
                return propDraft;
            }
            if (operation === 'editor_prop_draft_save') {
                const {draftId, expectedDraftRevision} = args as {draftId: string; expectedDraftRevision: number};
                const draft = propDrafts.get(draftId)!;
                expect(draft.draft_revision).toBe(expectedDraftRevision);
                if (savePropFailure) throw new Error(savePropFailure);
                propDraft = {...draft, dirty: false, draft_revision: expectedDraftRevision + 1, fingerprint: 'new saved fingerprint'};
                propDrafts.set(draftId, propDraft);
                initialPropDrafts.set(draftId, propDraft);
                return {draft: propDraft, snapshot: nativeState};
            }
            if (operation === 'editor_prop_nested_source') {
                expect(args).toEqual({assetId: nestedAsset.asset_id, fingerprint: nestedAsset.fingerprint, expectedGeneration: 1});
                return {asset_id: nestedAsset.asset_id, fingerprint: nestedAsset.fingerprint, slots: ['actor']};
            }
            if (operation === 'editor_prop_draft_add_nested') {
                const {draftId, expectedDraftRevision, placement} =
                    args as {draftId: string; expectedDraftRevision: number; placement: NestedPropPlacementRequest};
                const draft = propDraft!;
                expect(draftId).toBe(draft.draft_id);
                expect(expectedDraftRevision).toBe(draft.draft_revision);
                if (nestedPropFailure) throw new Error(nestedPropFailure);
                const target = {asset_id: placement.asset_id, fingerprint: placement.fingerprint};
                const existing = draft.prefab.composition?.placements ?? [];
                previousProp = draft.prefab;
                propDraft = {...draft, draft_revision: expectedDraftRevision + 1, dirty: true, can_undo: true, prefab: {
                    ...draft.prefab, schema_version: 4, composition: {
                        pins: [{version: target, path: nestedAsset.path, entities: draft.prefab.entities}],
                        placements: [...existing, {id: 'grate-' + (existing.length + 1), target,
                            name: placement.name, parent: placement.parent, local: placement.local, bindings: placement.bindings}],
                    },
                }};
                propDrafts.set(draftId, propDraft);
                return propDraft;
            }
            if (operation === 'editor_prop_draft_refresh_nested') {
                const {draftId, expectedDraftRevision, placementId, fingerprint} =
                    args as {draftId: string; expectedDraftRevision: number; placementId: string; fingerprint: string};
                const draft = propDrafts.get(draftId)!;
                expect(draft.draft_revision).toBe(expectedDraftRevision);
                const composition = draft.prefab.composition!;
                const target = {asset_id: nestedAsset.asset_id, fingerprint};
                propDraft = {...draft, draft_revision: expectedDraftRevision + 1, dirty: true, prefab: {
                    ...draft.prefab, composition: {
                        pins: [...composition.pins, {version: target, path: nestedAsset.path, entities: draft.prefab.entities}],
                        placements: composition.placements.map(placement => placement.id === placementId ? {...placement, target} : placement),
                    },
                }};
                propDrafts.set(draftId, propDraft);
                return propDraft;
            }
            if (operation === 'editor_prefab_source_close') {
                const {draftId} = args as {draftId: string};
                if (propDrafts.get(draftId)?.dirty) throw new Error('Save or discard this draft first.');
                propDrafts.delete(draftId);
                return undefined;
            }
            if (operation === 'editor_prop_preview_open') {
                const {requestId, expectedGeneration, expectedRevision} =
                    args as {requestId: number; expectedGeneration: number; expectedRevision: number};
                return {token: 'workspace-prop-preview', workspace_generation: expectedGeneration,
                    revision: expectedRevision, request_id: requestId, selection: null};
            }
            if (operation === 'editor_prop_preview_close' || operation === 'editor_prop_preview_select') return undefined;
            if (operation === 'editor_camera_preview_open') {
                const {cameraKey, expectedRevision, expectedGeneration, sceneId} =
                    args as {cameraKey: string; expectedRevision: number; expectedGeneration: number; sceneId: string};
                return {mode: 'fixed', camera_key: cameraKey, token: 'selected-camera-preview', revision: expectedRevision,
                    workspace_generation: expectedGeneration, scene_id: sceneId, sample: 0, effective_distance: 0,
                    required_probe_radius: 0, target_lost: false, blocker: null};
            }
            if (operation === 'editor_camera_preview_close') return undefined;
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
                if (action.type === 'align_camera_to_scene_view') {
                    expect(action.expected_revision).toBe(nativeState.revision);
                    expect(action.expected_generation).toBe(viewportEditorState.workspace_generation);
                    expect(action.scene_id).toBe(nativeState.active_scene_id);
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, dirty: true, can_undo: true, can_redo: false,
                        revision: nativeState.revision + 1, scene_revision: nativeState.scene_revision + 1,
                        document: {...nativeState.document, entities: nativeState.document.entities.map(entity =>
                            entity.key === action.key ? {...entity, transform: {...entity.transform, position: [2, 3, 5]}} : entity)}};
                    viewportEditorState = {...viewportEditorState, revision: viewportEditorState.revision + 1};
                    return viewportEditorState;
                }
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
                case 'create_camera': {
                    expect(nativeState.transaction).toBeNull();
                    const key = `camera-${nativeState.document.entities.filter(entity => !!entity.camera).length + 1}`;
                    const details = {id: `lens-${key}`, enabled: true, settings: cameraSettings};
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, selection: [key], dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, schema_version: 7,
                            default_game_camera: nativeState.document.default_game_camera ?? key,
                            entities: [...nativeState.document.entities, {
                                key, name: 'Camera', model_path: '__no_mesh__', renderable: false,
                                transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
                                camera: details, component_views: [{schema_version: 1, id: details.id, enabled: true,
                                    data: {kind: 'camera', settings: cameraSettings}}],
                            }]}};
                    break;
                }
                case 'set_default_game_camera':
                    expect(nativeState.document.entities.find(entity => entity.key === command.key)?.camera?.enabled).toBe(true);
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, default_game_camera: command.key}};
                    break;
                case 'create_player': {
                    expect(nativeState.transaction).toBeNull();
                    const key = `player-${nativeState.document.entities.filter(entity => entity.player_controller).length + 1}`;
                    const controller = {id: 'player', enabled: true, settings: playerSettings};
                    const motor = {id: 'character', enabled: true, settings: playerMotor};
                    const component_views: ComponentRecord[] = [
                        {schema_version: 1, id: 'body', enabled: true, data: {kind: 'collider', collider: playerCollider}},
                        {schema_version: 1, id: 'player', enabled: true, data: {kind: 'player_controller', settings: playerSettings}},
                        {schema_version: 1, id: 'character', enabled: true, data: {kind: 'character_controller', settings: playerMotor}},
                    ];
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, selection: [key], dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, schema_version: 7, entities: [...nativeState.document.entities, {
                            key, name: 'Player', model_path: '__no_mesh__', renderable: false, collider: playerCollider,
                            transform: {position: [0, 0.9, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
                            player_controller: controller, character_controller: motor, component_views,
                        }]}};
                    break;
                }
                case 'set_local_player': {
                    const {key} = command;
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, entities: nativeState.document.entities.map(entity => {
                            if (!entity.player_controller) return entity;
                            const settings = {...entity.player_controller.settings, local: entity.key === key};
                            return {...entity, player_controller: {...entity.player_controller, settings},
                                component_views: entity.component_views?.map(component => component.data.kind === 'player_controller'
                                    ? {...component, data: {...component.data, settings}} : component)};
                        })}};
                    break;
                }
                case 'add_component':
                case 'replace_component': {
                    const {component, key} = command;
                    const data = component.data;
                    undo.push(nativeState.document);
                    redo = [];
                    nativeState = {...nativeState, dirty: true, can_undo: true, can_redo: false,
                        document: {...nativeState.document, entities: nativeState.document.entities.map(entity => {
                            if (entity.key !== key) return entity;
                            const component_views = command.type === 'add_component' ? [...(entity.component_views ?? []), component] :
                                entity.component_views?.map(existing => existing.id === component.id ? component : existing);
                            const details = {id: component.id, enabled: component.enabled};
                            if (data.kind === 'camera') return {...entity, camera: {...details, settings: data.settings}, component_views};
                            if (data.kind === 'player_controller') return {...entity, player_controller: {...details, settings: data.settings}, component_views};
                            if (data.kind === 'animator') return {...entity, animator: {...details, settings: data.settings}, component_views};
                            throw new Error('Unexpected fixture component');
                        })}};
                    break;
                }
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
            nativeState = {...nativeState, revision: nativeState.revision + 1,
                scene_revision: nativeState.scene_revision + (command.type === 'select' ? 0 : 1)};
            const receipt = {
                revision: nativeState.revision, scene_revision: nativeState.scene_revision,
                change_cursor: nativeState.revision, active_scene_id: nativeState.active_scene_id,
                affected_scenes: [nativeState.active_scene_id],
                state_changed: true, project_changed: false,
                changes: {changed_entities: [], removed_entities: [], camera_changed: false,
                    lights_changed: false, topology_changed: false, full_resync: false},
            };
            if (command.type === 'select' && incrementalSelection) {
                const {document, project, ...state} = structuredClone(nativeState);
                return {kind: 'delta', state, receipt, entities: [], camera: null, lights: null};
            }
            return {
                kind: 'snapshot', snapshot: structuredClone(nativeState),
                receipt,
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

    it('keeps the hierarchy model stable for selection deltas and updates it for entity changes', async () => {
        incrementalSelection = true;
        const hierarchy = fixture.debugElement.query(By.directive(SceneEntitiesPanel)).componentInstance as SceneEntitiesPanel;
        const scene = hierarchy.scene();
        const revision = editor.snapshot()!.revision;
        await selectRobot();
        expect(editor.snapshot()!.revision).toBeGreaterThan(revision);
        expect(hierarchy.scene()).toBe(scene);

        await editor.execute({type: 'select', keys: []});
        await fixture.whenStable();
        expect(hierarchy.scene()).toBe(scene);
        expect(hierarchy.selectedKeys()).toEqual([]);

        await editor.execute({type: 'rename', key: 'robot', name: 'Scout'});
        await fixture.whenStable();
        expect(hierarchy.scene()).not.toBe(scene);
        expect(hierarchy.scene().entities.find(entity => entity.key === 'robot')?.name).toBe('Scout');
    });

    it('creates and selects Camera objects from the Hierarchy and commits lens and alignment as separate Undo steps', async () => {
        await editor.loadCapabilities();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const menu = TestBed.inject(ContextMenuService);
        const create = async (key: string) => {
            button('Create').click();
            const camera = menu.request()?.items.find(item => item.kind === 'action' && item.label === 'Camera');
            expect(camera).toBeDefined();
            menu.execute(camera!);
            await vi.waitFor(() => expect(editor.selection()).toEqual([key]));
            await fixture.whenStable();
        };
        await create('camera-1');
        await create('camera-2');
        expect(editor.document()?.default_game_camera).toBe('camera-1');
        expect(commands.filter(item => item.type === 'create_camera')).toEqual([{type: 'create_camera'}, {type: 'create_camera'}]);
        expect(root.querySelector('[data-key="camera-2"]')?.getAttribute('aria-selected')).toBe('true');
        root.querySelector<HTMLButtonElement>('button[aria-label="Edit Camera"]')!.click();
        await fixture.whenStable();
        const field = (label: string) => [...root.querySelectorAll('editor-component-editor label')]
            .find(item => item.firstChild?.textContent?.trim() === label)!.querySelector('input')!;
        await change(field('Near clip'), '200');
        expect(field('Far clip').getAttribute('aria-invalid')).toBe('true');
        expect(button('Apply Component').disabled).toBe(true);
        await change(field('Near clip'), '0.25');
        await change(field('Vertical field of view (degrees)'), '90');
        button('Apply Component').click();
        const camera = () => editor.document()?.entities.find(entity => entity.key === 'camera-2');
        await vi.waitFor(() => expect(camera()?.camera?.settings.lens.vertical_fov).toBeCloseTo(Math.PI / 2));
        const viewport = fixture.debugElement.injector.get(ViewportEditorService);
        await viewport.start();
        await fixture.whenStable();
        expect(button('Align Camera to Scene View').disabled).toBe(false);
        button('Align Camera to Scene View').click();
        await vi.waitFor(() => expect(camera()?.transform.position).toEqual([2, 3, 5]));
        action('Undo').click();
        await vi.waitFor(() => expect(camera()?.transform.position).toEqual([0, 0, 0]));
        expect(camera()?.camera?.settings.lens.vertical_fov).toBeCloseTo(Math.PI / 2);
        action('Undo').click();
        await vi.waitFor(() => expect(camera()?.camera?.settings.lens.vertical_fov).toBeCloseTo(Math.PI / 3));
    });

    it('previews a selected Camera over the stopped Game view and assigns the default in a separate edit', async () => {
        await editor.execute({type: 'create_camera'});
        await editor.execute({type: 'create_camera'});
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        nativeState = {...nativeState, revision: nativeState.revision + 1,
            document: {...nativeState.document, default_game_camera: null}};
        await editor.refresh();
        const viewport = fixture.debugElement.injector.get(ViewportEditorService);
        await viewport.start();
        await fixture.whenStable();
        const gameTab = [...root.querySelectorAll<HTMLButtonElement>('[role="tab"]')]
            .find(tab => tab.textContent?.trim() === 'Game')!;
        gameTab.click();
        await fixture.whenStable();
        expect(root.textContent).toContain('Select an enabled Camera in the Hierarchy');
        const revision = editor.snapshot()?.revision;
        const preview = button('Preview Selected Camera');
        await vi.waitFor(() => expect(preview.disabled).toBe(false));
        preview.click();
        await vi.waitFor(() => expect(root.textContent).toContain('Fixed camera view'));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_camera_preview_open', {
            cameraKey: 'camera-2', expectedRevision: revision, expectedGeneration: 1, sceneId: 'main',
        });
        expect(button('Turn left')).toBeUndefined();
        expect(editor.document()?.default_game_camera).toBeNull();
        expect(editor.snapshot()?.revision).toBe(revision);
        button('Close Camera Preview').click();
        await fixture.whenStable();
        expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_camera_preview_close', {token: 'selected-camera-preview'});
        expect(gameTab.getAttribute('aria-selected')).toBe('true');
        expect(root.textContent).toContain('Game preview is stopped');
        expect(document.activeElement).toBe(preview);

        button('Set as Game Camera').click();
        await vi.waitFor(() => expect(editor.document()?.default_game_camera).toBe('camera-2'));
        expect(root.textContent).toContain('Default Game Camera');
        expect(root.textContent).not.toContain('Select an enabled Camera in the Hierarchy');
        action('Undo').click();
        await vi.waitFor(() => expect(editor.document()?.default_game_camera).toBeNull());
    });

    it('creates a complete Player, routes local input explicitly, and restores prior ownership through Undo', async () => {
        await editor.loadCapabilities();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const menu = TestBed.inject(ContextMenuService);
        const create = async (key: string) => {
            button('Create').click();
            const player = menu.request()?.items.find(item => item.kind === 'action' && item.label === 'Player');
            expect(player).toBeDefined();
            menu.execute(player!);
            await vi.waitFor(() => expect(editor.selection()).toEqual([key]));
            await fixture.whenStable();
        };
        const entity = (key: string) => editor.document()?.entities.find(item => item.key === key);
        const localPlayer = () => editor.document()?.entities.find(item => item.player_controller?.settings.local)?.key;
        await create('player-1');
        expect(entity('player-1')?.character_controller?.enabled).toBe(true);
        expect(entity('player-1')?.collider?.kind).toBe('box');
        expect(root.textContent).toContain('Input source: Project Input');
        expect(localPlayer()).toBeUndefined();
        button('Set Local Player').click();
        await vi.waitFor(() => expect(localPlayer()).toBe('player-1'));
        await create('player-2');
        expect(localPlayer()).toBe('player-1');
        expect(root.querySelector('[data-key="player-2"]')?.getAttribute('aria-selected')).toBe('true');
        button('Set Local Player').click();
        await vi.waitFor(() => expect(localPlayer()).toBe('player-2'));
        expect(entity('player-1')?.player_controller?.settings.local).toBe(false);

        root.querySelector<HTMLButtonElement>('button[aria-label="Edit Player Controller"]')!.click();
        await fixture.whenStable();
        const label = [...root.querySelectorAll('editor-component-editor label')].find(
            item => item.firstChild?.textContent?.trim() === 'Movement space',
        )!;
        const space = label.querySelector('select')!;
        space.value = 'world';
        space.dispatchEvent(new Event('change', {bubbles: true}));
        space.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        button('Apply Component').click();
        await vi.waitFor(() => expect(entity('player-2')?.player_controller?.settings.space).toBe('world'));
        expect(localPlayer()).toBe('player-2');
        button('Clear Local Player').click();
        await vi.waitFor(() => expect(localPlayer()).toBeUndefined());
        action('Undo').click();
        await vi.waitFor(() => expect(localPlayer()).toBe('player-2'));
        action('Undo').click();
        await vi.waitFor(() => expect(entity('player-2')?.player_controller?.settings.space).toBe('camera_relative'));
        action('Undo').click();
        await vi.waitFor(() => expect(localPlayer()).toBe('player-1'));
    });

    it('edits typed Input Actions, monitors deliberate key input, and assigns the same asset ID after a move', async () => {
        inputActionsSupported = true;
        await editor.loadCapabilities();
        await editor.execute({type: 'create_player'});
        const viewport = fixture.debugElement.injector.get(ViewportEditorService);
        await viewport.start();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string, within: ParentNode = root) => [...within.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const field = (label: string, within: ParentNode) => [...within.querySelectorAll<HTMLLabelElement>('label')]
            .find(item => item.firstChild?.textContent?.trim() === label)!;
        await vi.waitFor(() => expect(fixture.componentInstance.canOpenControls()).toBe(true));
        fixture.componentInstance.openControls();
        await vi.waitFor(() => expect(button('Input Actions Assets')).toBeDefined());
        button('Input Actions Assets').click();
        await fixture.whenStable();
        const panel = root.querySelector('editor-input-actions-editor')!;
        await vi.waitFor(() => expect(button('New Asset', panel).disabled).toBe(false));
        button('New Asset', panel).click();
        await fixture.whenStable();
        const jump = panel.querySelector<HTMLElement>('[data-action-id="jump"]')!;
        jump.querySelector('button')!.click();
        await fixture.whenStable();
        const capture = jump.querySelector<HTMLButtonElement>('button[aria-label="Capture Keyboard key"]')!;
        const key = (code: string, kind = 'keydown') => new KeyboardEvent(kind, {code, bubbles: true, cancelable: true});
        capture.click();
        await fixture.whenStable();
        expect(capture.getAttribute('aria-pressed')).toBe('true');
        capture.dispatchEvent(key('Escape'));
        await fixture.whenStable();
        expect(capture.getAttribute('aria-pressed')).toBe('false');
        expect(field('Keyboard key', jump).querySelector('select')?.value).toBe('space');
        capture.click();
        capture.dispatchEvent(key('KeyQ'));
        await fixture.whenStable();
        expect(field('Keyboard key', jump).querySelector('select')?.value).toBe('q');

        const display = field('Display name', jump).querySelector('input')!;
        await change(display, 'Leap');
        const monitor = panel.querySelector<HTMLElement>('[aria-label="Monitor Input Actions"]')!;
        monitor.focus();
        monitor.dispatchEvent(key('KeyQ'));
        await fixture.whenStable();
        const result = () => [...monitor.querySelectorAll('dt')].find(item => item.textContent === 'Leap')?.nextElementSibling?.textContent;
        expect(result()).toBe('Pressed');
        display.focus();
        await fixture.whenStable();
        expect(result()).toBe('Released');

        const valueType = field('Value type', jump).querySelector('select')!;
        valueType.value = 'vector2';
        valueType.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        expect(panel.textContent).toContain('jump needs the button value type');
        expect(button('Save Input Actions', panel).disabled).toBe(true);
        valueType.value = 'button';
        valueType.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        button('Add Input Action', panel).click();
        await fixture.whenStable();
        const custom = panel.querySelector<HTMLElement>('[data-action-id="action_5"]')!;
        button('Add Binding', custom).click();
        await fixture.whenStable();
        const customType = field('Value type', custom).querySelector('select')!;
        customType.value = 'vector2';
        customType.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        expect(panel.textContent).toContain('action_5 has a binding with the wrong value type');
        expect(button('Save Input Actions', panel).disabled).toBe(true);
        button('Remove Action', custom).click();
        await fixture.whenStable();
        button('Save Input Actions', panel).click();
        await vi.waitFor(() => expect(inputActions.get(controlsAssetId)?.map.actions.find(action => action.id === 'jump'))
            .toMatchObject({id: 'jump', label: 'Leap', bindings: [{kind: 'key', key: 'q'}, {kind: 'gamepad_button', button: 'a'}]}));
        await fixture.whenStable();
        button('Move or Rename Asset…', panel).click();
        await fixture.whenStable();
        await change(field('New asset path', panel).querySelector('input')!, 'keyboard.input-actions.json');
        button('Move Asset', panel).click();
        await vi.waitFor(() => expect(inputActions.get(controlsAssetId)?.asset.path).toBe('keyboard.input-actions.json'));
        await fixture.whenStable();
        button('Open Asset', panel).click();
        await vi.waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_open_input_actions',
            expect.objectContaining({assetId: controlsAssetId, expectedGeneration: 1})));
        await fixture.whenStable();
        root.querySelector<HTMLButtonElement>('button[aria-label="Close Controls"]')!.click();
        await fixture.whenStable();

        const edit = root.querySelector<HTMLButtonElement>('button[aria-label="Edit Player Controller"]')!;
        edit.click();
        await fixture.whenStable();
        const inspector = root.querySelector('editor-component-editor')!;
        const inputSource = field('Input source', inspector).querySelector('select')!;
        inputSource.value = controlsAssetId;
        inputSource.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        button('Apply Component', inspector).click();
        await vi.waitFor(() => expect(editor.document()?.entities.find(entity => entity.key === 'player-1')
            ?.player_controller?.settings.input).toEqual({kind: 'asset', asset_id: controlsAssetId}));
        expect(commands.filter(command => command.type === 'replace_component')).toHaveLength(1);

        fixture.componentInstance.openControls();
        await fixture.whenStable();
        const controls = root.querySelector('editor-input-actions-editor:not([hidden])')!;
        expect(button('Open Asset', controls)).toBeDefined();
        const preset = field('Layout preset', controls).querySelector('select')!;
        preset.value = 'airborne';
        button('Add Layout', controls).click();
        await fixture.whenStable();
        const layoutId = field('Layout name', controls).querySelector('input')!;
        await change(layoutId, 'Air controls');
        expect(controls.querySelector('[data-action-id="jump"]')?.textContent).toContain('Unbound');
        expect(controls.querySelector('[data-action-id="attack"]')?.textContent).toContain('Override');
        button('Duplicate Layout', controls).click();
        await fixture.whenStable();
        button('Move Up', controls).click();
        await fixture.whenStable();
        expect(controls.textContent).toContain('Active layout Default');
        expect(controls.textContent).toContain('Unavailable');
        const motion = field('Preview motion', controls).querySelector('select')!;
        motion.value = 'airborne';
        motion.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        expect(controls.textContent).toContain('Active layout Air controls');
        expect(controls.textContent).toContain('Shadowed');
        const preview = controls.querySelector<HTMLElement>('[aria-label="Monitor Input Actions"]')!;
        preview.focus();
        preview.dispatchEvent(key('KeyQ'));
        await fixture.whenStable();
        expect([...preview.querySelectorAll('dt')].find(item => item.textContent === 'Attack')?.nextElementSibling?.textContent)
            .toBe('Pressed');
        motion.focus();
        button('Save Input Actions', controls).click();
        await vi.waitFor(() => expect(inputActions.get(controlsAssetId)?.map.schema_version).toBe(2));
        const layouts = inputActions.get(controlsAssetId)?.map.layouts ?? [];
        expect(layouts.map(layout => layout.label)).toEqual(['Air controls', 'Air controls']);
        expect(new Set(layouts.map(layout => layout.id)).size).toBe(2);
        expect(layouts[0].overrides).toContainEqual({action_id: 'jump', bindings: []});
    });

    it('reviews a registered Animator definition and binds the same graph to two independent visual actors', async () => {
        animatorSupported = true;
        const entity = initialSnapshot().document.entities[0];
        nativeState = {...nativeState, revision: nativeState.revision + 1, document: {...nativeState.document,
            entities: ['a', 'b'].flatMap(id => [{
                ...entity, key: 'actor-' + id, name: 'Actor ' + id, model_path: '__no_mesh__', renderable: false,
                component_views: [],
            }, {
                ...entity, key: 'blade-' + id, name: 'Blade ' + id, parent: 'actor-' + id,
            }])}};
        await editor.refresh();
        await editor.loadCapabilities();
        await fixture.debugElement.injector.get(ViewportEditorService).start();
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (name: string, within: ParentNode = root) => [...within.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === name)!;
        const field = (name: string, within: ParentNode) => [...within.querySelectorAll<HTMLLabelElement>('label')]
            .find(item => item.firstChild?.textContent?.trim() === name)!;
        const inspector = () => root.querySelector<HTMLElement>('editor-animator-inspector')!;
        const choose = (select: HTMLSelectElement, value: string) => {
            select.value = value;
            select.dispatchEvent(new Event('change', {bubbles: true}));
        };
        const selectedActor = async (key: string) => {
            root.querySelector<HTMLButtonElement>(`[data-key="${key}"]`)!.click();
            await vi.waitFor(() => expect(editor.selection()).toEqual([key]));
            await vi.waitFor(() => expect(button('Animator Assets…', inspector())?.disabled).toBe(false));
        };
        const bind = async (childKey: string) => {
            await vi.waitFor(() => expect(inspector().textContent).toContain('Entry state: Idle'));
            await fixture.whenStable();
            const attach = button('Attach Animator', inspector());
            expect(attach.disabled).toBe(true);
            expect(inspector().textContent).toContain('Choose a renderable child');
            const target = field('blade', inspector()).querySelector('select')!;
            expect([...target.options].map(option => option.value)).toEqual(['', childKey]);
            choose(target, childKey);
            await fixture.whenStable();
            attach.click();
            await vi.waitFor(() => expect(inspector().textContent).toContain('Animator binding validated'));
        };

        await selectedActor('actor-a');
        button('Animator Assets…', inspector()).click();
        await fixture.whenStable();
        const dialog = inspector().querySelector<HTMLDialogElement>('dialog[open]')!;
        const text = field('Animator graph JSON', dialog).querySelector('textarea')!;
        text.value = JSON.stringify({...walkGraph, entry: 'unknown'});
        text.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        button('Review Definition', dialog).click();
        await fixture.whenStable();
        expect(dialog.textContent).toContain('Entry "unknown" must name an existing state');
        expect(button('Save Animator Asset', dialog).disabled).toBe(true);
        text.value = JSON.stringify(walkGraph);
        text.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        button('Review Definition', dialog).click();
        await fixture.whenStable();
        button('Save Animator Asset', dialog).click();
        await vi.waitFor(() => expect(animators.get(animatorAssetId)?.graph.transitions).toHaveLength(2));
        await vi.waitFor(() => expect(dialog.textContent).toContain('Animator asset saved'));
        await vi.waitFor(() => expect(dialog.querySelector<HTMLButtonElement>('button[aria-label="Close Animator Assets"]')?.disabled).toBe(false));
        dialog.querySelector<HTMLButtonElement>('button[aria-label="Close Animator Assets"]')!.click();
        await fixture.whenStable();
        await bind('blade-a');

        await selectedActor('actor-b');
        const graphAsset = field('Animator asset', inspector()).querySelector('select')!;
        await vi.waitFor(() => expect([...graphAsset.options].some(option => option.value === animatorAssetId)).toBe(true));
        choose(graphAsset, animatorAssetId);
        await bind('blade-b');
        const settings = editor.document()?.entities.filter(item => item.animator).map(item => item.animator?.settings);
        expect(settings).toEqual(['blade-a', 'blade-b'].map(entity_key => ({
            asset_id: animatorAssetId,
            binding: {kind: 'visual', targets: [{channel: authoredClip.clip.tracks[0].target, entity_key}]},
        })));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_validate_animator_actor',
            expect.objectContaining({entityKey: 'actor-b', expectedGeneration: 1}));
    });

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

    it('reveals a current dependency owner in the Inspector from Check Scenes', async () => {
        verificationIssues = [{
            scene_id: 'main', scope: 'scene', role: null, suggestion: null, message: 'The panorama is unavailable.',
            dependency: {
                code: 'sky_panorama_missing', severity: 'error', owner: {key: 'robot', name: 'Robot'},
                target: {path: 'sky/overcast.png'}, open: {kind: 'inspector'},
            },
        }];
        await editor.loadCapabilities();
        expect(fixture.componentInstance.openSceneInformation()).toBe(true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const modal = () => root.querySelector('editor-scene-actions dialog[open]');
        const check = Array.from(modal()?.querySelectorAll('button') ?? []).find(button =>
            button.textContent?.trim() === 'Check Scenes');
        expect(check).toBeDefined();
        check?.click();
        await fixture.whenStable();
        const inspect = modal()?.querySelector<HTMLButtonElement>('button[aria-label="Inspect owner Robot"]');
        expect(inspect).toBeDefined();
        inspect?.click();
        await vi.waitFor(() => expect(editor.selection()).toEqual(['robot']));
        await fixture.whenStable();
        expect(modal()).toBeNull();
        expect(document.activeElement).toBe(root.querySelector('[data-reference-entity]'));
    });

    it('opens the Authored Clips library for an audio marker dependency without an imported Animation', async () => {
        verificationIssues = [{
            scene_id: 'main', scope: 'scene', role: null, suggestion: null, message: 'The contact marker is missing.',
            dependency: {
                code: 'audio_marker_missing', severity: 'error', owner: {key: 'robot', name: 'Robot'},
                target: {path: 'animations/bell.clip.json'}, open: {kind: 'animation'},
            },
        }];
        await editor.loadCapabilities();
        expect(fixture.componentInstance.openSceneInformation()).toBe(true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = (label: string) => Array.from(root.querySelectorAll('button')).find(item => item.textContent?.trim() === label);
        expect(button('Check Scenes')).toBeDefined();
        button('Check Scenes')?.click();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(button('Open in Animation')?.disabled).toBe(false);
        });
        button('Open in Animation')?.click();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(editor.selection()).toEqual(['robot']);
            expect(button('Authored Clips')?.getAttribute('aria-pressed')).toBe('true');
        });
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

    it('requires Controls and Physics Layers drafts to be resolved before leaving the project', async () => {
        const workspace = fixture.componentInstance;
        const root: HTMLElement = fixture.nativeElement;
        const click = async (selector: string, label: string) => {
            const button = Array.from(root.querySelectorAll<HTMLButtonElement>(selector + ' button'))
                .find(item => item.textContent?.trim() === label);
            expect(button).toBeDefined();
            button!.click();
            await fixture.whenStable();
        };

        workspace.openControls();
        await fixture.whenStable();
        await click('editor-game-input', 'Add Action');
        expect(await workspace.reviewProjectNavigation()).toBe(false);
        await fixture.whenStable();
        await click('editor-game-input', 'Discard and Close');
        expect(await workspace.reviewProjectNavigation()).toBe(true);

        expect(workspace.openPhysicsLayers()).toBe(true);
        await fixture.whenStable();
        await click('editor-physics-layers', 'Preview Standard World');
        await click('editor-physics-layers', 'Use preset');
        expect(await workspace.reviewProjectNavigation()).toBe(false);
        await fixture.whenStable();
        expect(root.querySelector('editor-physics-layers [role="alert"]')?.textContent)
            .toContain('Apply your Physics Layers changes');
        await click('editor-physics-layers', 'Cancel');
        expect(await workspace.reviewProjectNavigation()).toBe(true);
    });

    it('applies game bindings, saves the project and reopens with the authored profile before Play', async () => {
        fixture.componentInstance.openControls();
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
        fixture.componentInstance.openControls();
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

    it('keeps workspace focus and panel labels valid through keyboard navigation, Play, and project replacement', async () => {
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const tabs = () => [...root.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
        const tab = (name: string) => tabs().find(button => button.textContent?.trim() === name);
        const expectActive = (name: string) => {
            const selected = tab(name);
            const panel = root.querySelector<HTMLElement>('[role="tabpanel"]')!;
            expect(selected).toBeDefined();
            expect(tabs().filter(button => button.getAttribute('aria-selected') === 'true')).toEqual([selected]);
            expect(panel.getAttribute('aria-labelledby')).toBe(selected?.id);
            expect(selected?.getAttribute('aria-controls')).toBe(panel.id);
        };
        const key = (target: HTMLElement, value: string) =>
            target.dispatchEvent(new KeyboardEvent('keydown', {key: value, bubbles: true, cancelable: true}));

        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await fixture.whenStable();
        expectActive('Prop');
        const prop = tab('Prop')!;
        prop.focus();
        key(prop, 'ArrowRight');
        await fixture.whenStable();
        expectActive('Prop');
        expect(document.activeElement).toBe(tab('Animation'));
        expect(root.querySelector('editor-animation-workspace')).toBeNull();

        key(tab('Animation')!, 'Enter');
        await fixture.whenStable();
        expectActive('Animation');
        const importedClips = root.querySelector<HTMLButtonElement>('editor-animation-workspace .clip-kind button')!;
        const tools = root.querySelector('editor-animation-workspace')!;
        const sharedViewport = root.querySelector('scene-viewport')!;
        expect(tools.nextElementSibling).toBe(sharedViewport);
        expect(tools.parentElement?.classList.contains('animation-layout')).toBe(true);
        expect(root.querySelector('scene-entities-panel')?.hasAttribute('hidden')).toBe(true);
        expect(root.querySelector('editor-assets-panel')?.hasAttribute('hidden')).toBe(true);
        root.querySelector<HTMLButtonElement>('editor-animation-workspace .clip-kind button:nth-child(2)')!.click();
        await fixture.whenStable();
        const animation = fixture.debugElement.injector.get(AnimationWorkspaceService);
        const review = vi.fn(async () => false);
        const release = animation.registerTimelineReview(review);
        importedClips.click();
        await fixture.whenStable();
        expect(animation.library()).toBe('authored');
        tab('Scene')!.click();
        await fixture.whenStable();
        expectActive('Animation');
        expect(review).toHaveBeenCalledTimes(2);
        expect(await fixture.componentInstance.reviewProjectNavigation()).toBe(false);
        action('Play').click();
        await fixture.whenStable();
        expect(editor.playing()).toBe(false);
        expect(review).toHaveBeenCalledTimes(4);
        release();
        importedClips.focus();
        action('Play').click();
        await vi.waitFor(() => expect(editor.playing()).toBe(true));
        await fixture.whenStable();
        expectActive('Game');
        expect(tab('Animation')).toBeUndefined();
        expect(document.activeElement).toBe(tab('Game'));

        action('Stop').click();
        await vi.waitFor(() => expect(editor.playing()).toBe(false));
        await fixture.whenStable();
        expectActive('Animation');
        expect(document.activeElement).toBe(tab('Game'));

        const currentProp = tab('Prop')!;
        currentProp.focus();
        currentProp.click();
        await fixture.whenStable();
        expectActive('Prop');
        await editor.openProject('replacement/project.json');
        await fixture.whenStable();
        expect(tab('Prop')).toBeUndefined();
        expectActive('Scene');
        expect(document.activeElement).toBe(tab('Scene'));
        expect(tabs().filter(button => button.tabIndex === 0)).toEqual([tab('Scene')]);
    });

    it('opens the existing asset filters and creates a camera through the Objects actions', async () => {
        await editor.loadCapabilities();
        await fixture.whenStable();
        const workspace = fixture.componentInstance;
        const root = fixture.nativeElement as HTMLElement;
        expect(workspace.availableObjectActions()).toEqual(expect.arrayContaining(['browse_props', 'browse_models', 'create_camera']));
        const filter = root.querySelector<HTMLSelectElement>('.asset-filter select')!;
        await workspace.runObjectAction('browse_props');
        await fixture.whenStable();
        expect(filter.value).toBe('prefab');
        expect(document.activeElement).toBe(root.querySelector('input[aria-label="Search Assets"]'));
        await workspace.runObjectAction('browse_models');
        await fixture.whenStable();
        expect(filter.value).toBe('model');

        await workspace.runObjectAction('create_camera');
        await fixture.whenStable();
        expect(commands).toContainEqual({type: 'create_camera'});
        expect(editor.selection()).toEqual(['camera-1']);
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

    it('applies and undoes typed Prop fields, keeps the preview owner, and guards a changed source file on close', async () => {
        openPropSource = async revision => ({...propSource(revision), draft_revision: 1, dirty: false, can_undo: false, can_redo: false});
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(root.querySelector('prop-object-editor')).not.toBeNull());
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const position = () => root.querySelector<HTMLInputElement>('input[aria-label="Prop local position X"]')!;
        position().focus();
        position().value = '3';
        position().dispatchEvent(new Event('input', {bubbles: true}));
        position().dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
        await vi.waitFor(() => expect(propDraft?.prefab.entities[0].transform.position[0]).toBe(3));
        await fixture.whenStable();
        expect(root.querySelector('editor-prop-workspace')?.textContent).toContain('Unsaved Prop changes');
        const previewOpens = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === 'editor_prop_preview_open').length;
        const leaseCount = previewOpens();
        button('Undo Prop').click();
        await vi.waitFor(() => expect(position().value).toBe('0'));
        button('Redo Prop').click();
        await vi.waitFor(() => expect(position().value).toBe('3'));
        expect(editor.dirty()).toBe(false);
        expect(commands).toEqual([]);
        expect(previewOpens()).toBe(leaseCount);

        savePropFailure = 'The source changed on disk.';
        button('Close Prop').click();
        await vi.waitFor(() => expect(root.querySelector('dialog[open]')?.textContent).toContain('Unsaved Prop changes'));
        button('Save Prop and Continue').click();
        await vi.waitFor(() => expect(root.querySelector('dialog[open]')?.textContent).toContain('changed on disk'));
        expect(propDraft?.dirty).toBe(true);
        expect(root.querySelector('editor-prop-workspace')).not.toBeNull();

        savePropFailure = null;
        button('Save Prop and Continue').click();
        await vi.waitFor(() => expect(root.querySelector('editor-prop-workspace')).toBeNull());
        expect(propDraft?.dirty).toBe(false);
        expect(editor.dirty()).toBe(false);
    });

    it('adopts only advancing native Prop draft events for the current source', async () => {
        openPropSource = async revision => ({...propSource(revision), draft_revision: 1, dirty: false, can_undo: false, can_redo: false});
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(root.querySelector('prop-object-editor')).not.toBeNull());
        const draft = propDraft!;
        const moved = {...draft, draft_revision: 2, dirty: true, can_undo: true, prefab: {
            ...draft.prefab,
            entities: draft.prefab.entities.map((entity, index) => index === 0
                ? {...entity, transform: {...entity.transform, position: [5, 0, 0] as const}} : entity),
        }};
        const publish = async (payload: PrefabSourceDraft) => {
            propDraftListener?.({event: 'editor://prop-draft-changed', id: 1, payload});
            await fixture.whenStable();
        };
        const position = () => root.querySelector<HTMLInputElement>('input[aria-label="Prop local position X"]')?.value;
        await publish({...moved, draft_id: 'other-draft'});
        expect(position()).toBe('0');
        await publish(moved);
        expect(position()).toBe('5');
        await publish(draft);
        expect(position()).toBe('5');
        expect(editor.dirty()).toBe(false);
    });

    it('reviews a nested Prop pin and edits two placements independently with stable IDs', async () => {
        openPropSource = async revision => ({...propSource(revision), draft_revision: 1, dirty: false, can_undo: false, can_redo: false});
        assets = [propAsset, nestedAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(root.querySelector('prop-object-editor')).not.toBeNull());
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const form = () => root.querySelector<HTMLFormElement>('editor-nested-prop-editor form')!;
        const changeSelect = (select: HTMLSelectElement, value: string) => {
            select.value = value;
            select.dispatchEvent(new Event('change', {bubbles: true}));
        };
        const placements = () => propDraft?.prefab.composition?.placements ?? [];
        const add = async (x: string) => {
            root.querySelector<HTMLButtonElement>('[data-prop-object="arch"]')!.click();
            await fixture.whenStable();
            button('Add Nested Prop…').click();
            await fixture.whenStable();
            changeSelect(form().querySelector('select')!, nestedAsset.asset_id);
            await vi.waitFor(() => expect(form().querySelectorAll('select').length).toBe(2));
            const apply = form().querySelector<HTMLButtonElement>('button[type="submit"]')!;
            expect(apply.disabled).toBe(true);
            changeSelect(form().querySelectorAll('select')[1], 'object:arch');
            const coordinate = form().querySelectorAll<HTMLInputElement>('input')[1];
            coordinate.value = x;
            coordinate.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
            expect(apply.disabled).toBe(false);
            form().dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        };

        nestedPropFailure = 'The dependency introduces a cycle.';
        await add('2');
        await vi.waitFor(() => expect(root.querySelector('editor-prop-workspace [role="alert"]')?.textContent).toContain('cycle'));
        expect(placements()).toEqual([]);
        nestedPropFailure = null;
        form().dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        await vi.waitFor(() => expect(placements()).toHaveLength(1));
        await fixture.whenStable();
        await add('5');
        await vi.waitFor(() => expect(placements()).toHaveLength(2));
        await fixture.whenStable();
        expect(placements().map(item => item.local.position[0])).toEqual([2, 5]);
        expect(root.querySelectorAll('[data-prop-placement]')).toHaveLength(2);
        root.querySelector<HTMLButtonElement>('[data-prop-placement="grate-1"]')!.click();
        await fixture.whenStable();
        expect(root.querySelector('[aria-label="Prop Inspector"]')?.textContent).toContain('grate.prefab.json');
        const name = root.querySelector<HTMLInputElement>('prop-object-editor input[type="text"]')!;
        name.value = 'Left grate';
        name.dispatchEvent(new Event('change', {bubbles: true}));
        await vi.waitFor(() => expect(placements()[0].name).toBe('Left grate'));
        await fixture.whenStable();
        button('Move Placement Down').click();
        await vi.waitFor(() => expect(placements().map(item => item.id)).toEqual(['grate-2', 'grate-1']));
        expect(placements().map(item => item.local.position[0])).toEqual([5, 2]);
        expect(propDraft?.prefab.composition?.pins).toHaveLength(1);
        expect(editor.dirty()).toBe(false);
    });

    it('retains an outer draft and its placement through nested Back and reviews all drafts on File navigation', async () => {
        openPropSource = async (revision, path) => nestedSourceFixture(revision, path);
        assets = [propAsset, nestedAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        const heading = () => root.querySelector('[aria-current="page"]')?.textContent?.trim();
        const review = () => root.querySelector('dialog[open]')?.textContent;
        const rename = (name: string) => {
            const input = root.querySelector<HTMLInputElement>('prop-object-editor input[type="text"]')!;
            input.value = name;
            input.dispatchEvent(new Event('change', {bubbles: true}));
        };
        const openAsset = (id: string) => root.querySelector(`[data-asset-id="${id}"]`)!
            .dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        openAsset(propAsset.asset_id);
        await vi.waitFor(() => expect(heading()).toBe('Arch'));
        rename('Edited Arch');
        await vi.waitFor(() => expect(propDrafts.get('arch-draft')?.dirty).toBe(true));
        const placement = () => root.querySelector<HTMLButtonElement>('[data-prop-placement="left"]')!;
        placement().click();
        await fixture.whenStable();
        button('Open Nested Source').click();
        await vi.waitFor(() => expect(heading()).toBe('Grate'));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_prefab_source_open', expect.objectContaining({path: nestedAsset.path}));
        openAsset(nestedAsset.asset_id);
        await fixture.whenStable();
        expect(vi.mocked(invoke).mock.calls.filter(([operation]) => operation === 'editor_prefab_source_open')).toHaveLength(2);
        rename('Edited Grate');
        await vi.waitFor(() => expect(propDrafts.get('grate-draft')?.dirty).toBe(true));
        button('Arch').click();
        await vi.waitFor(() => expect(review()).toContain('draft for Grate'));
        button('Keep Editing').click();
        await vi.waitFor(() => expect(review()).toBeUndefined());
        expect(heading()).toBe('Grate');
        button('Arch').click();
        await vi.waitFor(() => expect(review()).toContain('draft for Grate'));
        button('Discard Prop Changes').click();
        await vi.waitFor(() => expect(heading()).toBe('Arch'));
        expect(placement().getAttribute('aria-pressed')).toBe('true');
        expect(propDrafts.get('arch-draft')?.prefab.entities[0].name).toBe('Edited Arch');
        expect(propDrafts.has('grate-draft')).toBe(false);

        button('Open Nested Source').click();
        await vi.waitFor(() => expect(heading()).toBe('Grate'));
        rename('Edited Grate again');
        await vi.waitFor(() => expect(propDrafts.get('grate-draft')?.dirty).toBe(true));
        const leave = fixture.componentInstance.reviewPropNavigation();
        await vi.waitFor(() => expect(review()).toContain('draft for Grate'));
        button('Discard Prop Changes').click();
        await vi.waitFor(() => expect(review()).toContain('draft for Arch'));
        button('Save Prop and Continue').click();
        await expect(leave).resolves.toBe(true);
        expect(propDrafts.get('arch-draft')?.dirty).toBe(false);
        expect(propDrafts.get('arch-draft')?.prefab.entities[0].name).toBe('Edited Arch');
        expect(editor.dirty()).toBe(false);
    });

    it('keeps missing nested sources pinned and refreshes only the reviewed placement', async () => {
        openPropSource = async (revision, path) => nestedSourceFixture(revision, path);
        assets = [propAsset];
        await editor.assets(true);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(item => item.textContent?.trim() === label)!;
        root.querySelector('[data-asset-id="arch-id"]')!.dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
        await vi.waitFor(() => expect(root.querySelector('[data-prop-placement="left"]')).not.toBeNull());
        root.querySelector<HTMLButtonElement>('[data-prop-placement="left"]')!.click();
        await fixture.whenStable();
        const inspector = () => root.querySelector('[aria-label="Prop Inspector"]')?.textContent;
        expect(inspector()).toContain('Source unavailable');
        expect(inspector()).toContain('old-grate.prefab.json');
        expect(button('Open Nested Source').disabled).toBe(true);
        assets = [propAsset, nestedAsset];
        await editor.assets(true);
        await fixture.whenStable();
        expect(inspector()).toContain('A new source version is available');
        button('Review Refresh…').click();
        await vi.waitFor(() => expect(button('Refresh This Placement')?.disabled).toBe(false));
        const review = root.querySelector('[aria-label="Review nested Prop refresh"]')?.textContent;
        expect(review).toContain('b'.repeat(64));
        expect(review).toContain(nestedAsset.fingerprint);
        expect(vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_prop_draft_refresh_nested')).toBe(false);
        button('Refresh This Placement').click();
        await vi.waitFor(() => expect(inspector()).toContain('The pinned source is current'));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith('editor_prop_draft_refresh_nested', {
            draftId: 'arch-draft', expectedDraftRevision: 1, placementId: 'left', fingerprint: nestedAsset.fingerprint,
        });
        expect(propDrafts.get('arch-draft')?.prefab.composition?.placements.map(placement => placement.target.fingerprint))
            .toEqual([nestedAsset.fingerprint, 'b'.repeat(64)]);
        expect(editor.dirty()).toBe(false);
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

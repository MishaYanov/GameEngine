import type {UnlistenFn} from '@tauri-apps/api/event';
import type {EditorBackend} from '../core/editor/editor-backend';
import type {
    EditorCapabilities,
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    SceneDocument,
} from '../core/editor/editor.models';
import {DEMO_ASSETS, demoEntity, demoProject, demoScenes} from './demo-project';

const EDITS = new Set<EditorCommand['type']>([
    'rename',
    'spawn',
    'set_transforms',
    'delete_selection',
    'duplicate_selection',
]);
const COMMANDS = [
    'select',
    'switch_scene',
    ...EDITS,
    'undo',
    'redo',
    'start_play',
    'stop_play',
    'set_paused',
    'set_snap',
] satisfies EditorCommand['type'][];
const HISTORY_LIMIT = 64;

/** Deterministic UI fixture. No disk access, native renderer, or runtime simulation. */
export class DemoEditorBackend implements EditorBackend {
    readonly available = true;
    private documents = demoScenes();
    private saved = structuredClone(this.documents);
    private state = demoProject(this.documents);
    private readonly histories = new Map<
        string,
        { past: SceneDocument[]; future: SceneDocument[] }
    >();
    private nextId = 1;

    async request<T>(operation: string, args: Record<string, unknown> = {}): Promise<T> {
        // The generic cast is confined to the transport boundary, like Tauri's invoke<T>.
        return structuredClone(this.dispatch(operation, structuredClone(args))) as T;
    }

    async subscribe<T>(_event: string, _handler: (payload: T) => void): Promise<UnlistenFn> {
        return () => undefined;
    }

    private dispatch(operation: string, args: Record<string, unknown>): unknown {
        switch (operation) {
            case 'editor_capabilities':
                return {
                    protocol_version: 2,
                    scene_schema_version: 3,
                    embedded_viewport_supported: false,
                    max_selection: 4096,
                    max_history_entries: HISTORY_LIMIT,
                    history_budget_bytes: 0,
                    commands: COMMANDS.map((name) => ({
                        name,
                        label: name.replaceAll('_', ' '),
                        category: 'Demo',
                        undoable: EDITS.has(name),
                        available_in_play: ['select', 'stop_play', 'set_paused'].includes(name),
                    })),
                } satisfies EditorCapabilities;
            case 'editor_open':
            case 'editor_open_project':
                this.documents = demoScenes();
                this.saved = structuredClone(this.documents);
                this.state = demoProject(this.documents);
                this.histories.clear();
                this.nextId = 1;
                return this.snapshot();
            case 'editor_snapshot':
                return this.snapshot();
            case 'editor_assets':
                return DEMO_ASSETS;
            case 'editor_execute': {
                const request = args['request'] as EditorCommandRequest;
                this.checkRevision(request.expected_revision);
                this.execute(request.command);
                this.state = {...this.state, revision: this.state.revision + 1};
                return this.snapshot();
            }
            case 'editor_save':
                this.checkRevision(args['expectedRevision']);
                this.requireEdit();
                this.saved[this.state.active_scene_id] = structuredClone(this.state.document);
                this.state = {...this.state, revision: this.state.revision + 1};
                return this.snapshot();
            case 'editor_reload':
                this.checkRevision(args['expectedRevision']);
                this.requireEdit();
                this.state = {
                    ...this.state,
                    document: structuredClone(this.saved[this.state.active_scene_id]),
                    selection: [],
                    revision: this.state.revision + 1,
                    scene_revision: this.state.scene_revision + 1,
                };
                this.histories.delete(this.state.active_scene_id);
                return this.snapshot();
            default:
                throw new Error(
                    `Demo backend does not implement ${operation}. Use the desktop editor for native features.`,
                );
        }
    }

    private execute(command: EditorCommand): void {
        if (this.state.mode === 'play' && !['select', 'stop_play', 'set_paused'].includes(command.type))
            this.requireEdit();
        switch (command.type) {
            case 'select':
                if (command.keys.some((key) => !this.state.document.entities.some((e) => e.key === key)))
                    throw new Error('Unknown entity.');
                this.state = {...this.state, selection: [...new Set(command.keys)]};
                return;
            case 'switch_scene': {
                const document = this.documents[command.scene_id];
                if (!document) throw new Error('Unknown demo scene.');
                this.documents[this.state.active_scene_id] = this.state.document;
                this.state = {
                    ...this.state,
                    active_scene_id: command.scene_id,
                    document,
                    selection: [],
                    scene_revision: this.state.scene_revision + 1,
                };
                return;
            }
            case 'rename': {
                if (!command.name.trim()) throw new Error('Enter an entity name.');
                if (!this.state.document.entities.some((e) => e.key === command.key))
                    throw new Error('Unknown entity.');
                this.change({
                    ...this.state.document,
                    entities: this.state.document.entities.map((e) =>
                        e.key === command.key ? {...e, name: command.name.trim()} : e,
                    ),
                });
                return;
            }
            case 'spawn': {
                const key = this.key();
                if (
                    command.entity.parent &&
                    !this.state.document.entities.some((e) => e.key === command.entity.parent)
                )
                    throw new Error('Unknown parent.');
                const entity = {
                    ...demoEntity(key, command.entity.name, command.entity.model_path, [0, 0, 0]),
                    ...command.entity,
                    key,
                };
                this.change({
                    ...this.state.document,
                    entities: [...this.state.document.entities, entity],
                });
                this.state = {...this.state, selection: [key]};
                return;
            }
            case 'set_transforms':
                if (command.space !== 'local' || command.snap)
                    throw new Error('Demo transforms support local, unsnapped edits only.');
                if (
                    command.edits.some(
                        (edit) =>
                            !this.state.document.entities.some((e) => e.key === edit.key) ||
                            [
                                ...edit.transform.position,
                                ...edit.transform.rotation,
                                ...edit.transform.scale,
                            ].some((v) => !Number.isFinite(v)),
                    )
                )
                    throw new Error('Invalid transform.');
                this.change({
                    ...this.state.document,
                    entities: this.state.document.entities.map((entity) => {
                        const edit = command.edits.find((e) => e.key === entity.key);
                        return edit ? {...entity, transform: edit.transform} : entity;
                    }),
                });
                return;
            case 'duplicate_selection': {
                const entities = this.state.document.entities.filter((e) =>
                    this.state.selection.includes(e.key),
                );
                const keys = new Map(entities.map((e) => [e.key, this.key()]));
                const copies = entities.map((e) => ({
                    ...e,
                    key: keys.get(e.key)!,
                    name: `${e.name} copy`,
                    parent: e.parent ? (keys.get(e.parent) ?? e.parent) : null,
                    transform: {
                        ...e.transform,
                        position: e.transform.position.map((v, i) => v + command.offset[i]) as [
                            number,
                            number,
                            number,
                        ],
                    },
                }));
                this.change({
                    ...this.state.document,
                    entities: [...this.state.document.entities, ...copies],
                });
                this.state = {...this.state, selection: copies.map((e) => e.key)};
                return;
            }
            case 'delete_selection': {
                const removed = new Set(this.state.selection);
                let previousSize: number;
                do {
                    previousSize = removed.size;
                    for (const entity of this.state.document.entities)
                        if (entity.parent && removed.has(entity.parent)) removed.add(entity.key);
                } while (removed.size !== previousSize);
                this.change({
                    ...this.state.document,
                    entities: this.state.document.entities.filter((e) => !removed.has(e.key)),
                    bindings: Object.fromEntries(
                        Object.entries(this.state.document.bindings).filter(([, key]) => !removed.has(key)),
                    ),
                });
                this.state = {...this.state, selection: []};
                return;
            }
            case 'undo':
            case 'redo': {
                const history = this.history();
                const from = command.type === 'undo' ? history.past : history.future;
                const to = command.type === 'undo' ? history.future : history.past;
                const document = from.pop();
                if (document) {
                    to.push(this.state.document);
                    this.state = {
                        ...this.state,
                        document,
                        scene_revision: this.state.scene_revision + 1,
                        selection: this.state.selection.filter((key) =>
                            document.entities.some((e) => e.key === key),
                        ),
                    };
                }
                return;
            }
            case 'start_play':
                this.state = {...this.state, mode: 'play', paused: false};
                return;
            case 'stop_play':
                this.state = {...this.state, mode: 'edit', paused: false};
                return;
            case 'set_paused':
                if (this.state.mode !== 'play') throw new Error('Start the play preview before pausing.');
                this.state = {...this.state, paused: command.paused};
                return;
            case 'set_snap':
                this.state = {...this.state, snap: command.settings};
                return;
            default:
                throw new Error(`The demo does not simulate ${command.type}.`);
        }
    }

    private change(document: SceneDocument): void {
        const history = this.history();
        history.past.push(this.state.document);
        if (history.past.length > HISTORY_LIMIT) history.past.shift();
        history.future = [];
        this.state = {...this.state, document, scene_revision: this.state.scene_revision + 1};
    }

    private history() {
        const id = this.state.active_scene_id;
        let history = this.histories.get(id);
        if (!history) {
            history = {past: [], future: []};
            this.histories.set(id, history);
        }
        return history;
    }

    private snapshot(): EditorSnapshot {
        this.documents[this.state.active_scene_id] = this.state.document;
        const scenes = this.state.scenes.map((scene) => ({
            ...scene,
            dirty: JSON.stringify(this.documents[scene.id]) !== JSON.stringify(this.saved[scene.id]),
        }));
        return {
            ...this.state,
            scenes,
            dirty: scenes.some((scene) => scene.dirty),
            can_undo: this.history().past.length > 0,
            can_redo: this.history().future.length > 0,
        };
    }

    private key(): string {
        let key: string;
        do {
            key = `demo-entity-${this.nextId++}`;
        } while (this.state.document.entities.some((e) => e.key === key));
        return key;
    }

    private checkRevision(expected: unknown): void {
        if (expected !== this.state.revision)
            throw new Error(`Revision conflict: current ${this.state.revision}`);
    }

    private requireEdit(): void {
        if (this.state.mode !== 'edit') throw new Error('Stop the play preview before editing.');
    }
}

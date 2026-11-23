import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {
    EditorCommand,
    EditorSnapshot,
    ParticleEmitterComponent,
    SceneEntity
} from '../../../../core/editor/editor.models';
import type {LinkedPropAddress} from '../../../../core/editor/linked-prefabs.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {ScopedPaneRequests} from '../scoped-pane-request';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {EditorService} from '../../../../core/editor/editor.service';
import {DEFAULT_PARTICLE_EMITTER} from '../../../../core/editor/particle-emitter.models';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {ParticleDesigner} from './particle-designer';
import {ParticleDesignerSession} from './particle-designer-session';
import {PARTICLE_PRESETS, particlePresetLabel} from './particle-designer.models';
import {ParticlePreview} from './particle-preview';

const emitter: ParticleEmitterComponent = {id: 'fixture-emitter', enabled: true, settings: DEFAULT_PARTICLE_EMITTER};
const entity: SceneEntity = {
    key: 'mist', name: 'Mist', model_path: '', renderable: false,
    transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    particle_emitter: emitter,
};

type DraftSnapshot = Pick<EditorSnapshot, 'mode' | 'revision' | 'active_scene_id' | 'scene_revision' | 'selection'> & {
    document: Pick<EditorSnapshot['document'], 'entities'>;
};

describe('Particle Designer', () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            },
        });
    });

    afterAll(() => {
        if (originalShow) Object.defineProperty(prototype, 'showModal', originalShow);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (originalClose) Object.defineProperty(prototype, 'close', originalClose);
        else Reflect.deleteProperty(prototype, 'close');
    });

    afterEach(() => vi.restoreAllMocks());

    async function setup(open = true) {
        vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
        const snapshot = signal<DraftSnapshot>({
            mode: 'edit', revision: 8, active_scene_id: 'yard', scene_revision: 3, selection: [entity.key],
            document: {entities: [entity]},
        });
        const workspaceGeneration = signal(2);
        const inspectLinkedPropAddress = vi.fn(async (): Promise<LinkedPropAddress | null> => null);
        const execute = vi.fn(async (command: EditorCommand) => {
            if (command.type !== 'replace_component' || command.component.data.kind !== 'particle_emitter') {
                throw new Error('Unexpected command');
            }
            const {id, enabled, data: {settings}} = command.component;
            snapshot.update(current => ({
                ...current, revision: current.revision + 1,
                document: {entities: [{...entity, particle_emitter: {id, enabled, settings}}]},
            }));
            return snapshot();
        });
        TestBed.configureTestingModule({
            providers: [
                {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: vi.fn(async () => ({})), save: vi.fn()}},
                {provide: EditorService, useValue: {snapshot, workspaceGeneration, execute, inspectLinkedPropAddress}},
                {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 17})}},
                ScopedPaneRequests, WorkspaceDraftReview,
                {
                    provide: SceneEditingService, useValue: {
                        ready: () => true, error: () => null,
                        withCommittedEdit: async (write: () => Promise<void>) => {
                            await write();
                            return true;
                        },
                    }
                },
                ParticleDesignerSession,
            ]
        });
        const fixture = TestBed.createComponent(ParticleDesigner);
        const root = fixture.nativeElement as HTMLElement;
        const session = TestBed.inject(ParticleDesignerSession);
        const opener = document.createElement('button');
        root.append(opener);
        opener.focus();
        await fixture.whenStable();
        if (open) expect(await session.openSelected(entity.key)).toBe(true);
        await fixture.whenStable();
        const button = (name: string) => [...root.querySelectorAll('button')].find(item =>
            item.textContent?.trim() === name || item.getAttribute('aria-label') === name)!;
        const click = async (name: string) => {
            button(name).click();
            await fixture.whenStable();
        };
        const changePreset = async (name: string) => {
            const select = root.querySelector('select')!;
            select.value = name;
            select.dispatchEvent(new Event('change', {bubbles: true}));
            await fixture.whenStable();
        };
        return {fixture, root, session, snapshot, workspaceGeneration, execute, button, click, changePreset, opener,
            inspectLinkedPropAddress, reviews: TestBed.inject(WorkspaceDraftReview)};
    }

    it('opens only the clicked emitter after review and rejects stale selection', async () => {
        const {session, snapshot, reviews} = await setup(false);
        const subject: InspectorSubject = {address: {kind: 'scene', scene: 'yard', object: entity.key},
            scope: {workspaceGeneration: 2, nativeGeneration: 17, revision: 8}, contentRevision: 3, access: 'scene-edit'};
        expect(await session.openScoped(subject, entity.key, 'unknown-component')).toBe(false);
        reviews.setReviewers([() => {
            snapshot.update(current => ({...current, selection: ['other']}));
            return true;
        }]);
        expect(await session.openScoped(subject, entity.key, emitter.id)).toBe(false);
        expect(session.active()).toBe(false);
    });

    it('validates exact linked member addresses and applies a supported override', async () => {
        const {session, snapshot, inspectLinkedPropAddress, execute} = await setup(false);
        const address = {kind: 'object', placement_path: ['child-placement'], source_entity: 'source-mist'} as const;
        const root: SceneEntity = {...entity, key: 'linked-root', particle_emitter: null,
            linked_prefab: {source: {source_id: 'prop', path: 'props/yard.prefab.json'}, bindings: {}}};
        snapshot.update(current => ({...current, document: {entities: [root, {...entity, parent: root.key}]}}));
        const subject: InspectorSubject = {address: {kind: 'linked', scene: 'yard', root: root.key, address},
            scope: {workspaceGeneration: 2, nativeGeneration: 17, revision: 8}, contentRevision: 3, access: 'linked-overrides'};
        const reply = {root_key: root.key, entity_key: entity.key, scene_id: 'yard', address};
        inspectLinkedPropAddress.mockResolvedValueOnce({...reply, address: {...address, source_entity: 'wrong-source'}});
        expect(await session.openScoped(subject, entity.key, emitter.id)).toBe(false);
        expect(await session.openScoped({...subject, access: 'read-only'}, entity.key, emitter.id)).toBe(false);
        inspectLinkedPropAddress.mockResolvedValue(reply);
        expect(await session.openScoped(subject, entity.key, emitter.id)).toBe(true);
        session.pickPreset('Sparks');
        await session.apply();
        expect(inspectLinkedPropAddress).toHaveBeenCalledTimes(3);
        expect(execute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
            type: 'replace_component', key: entity.key,
        }), 2, 8);
    });

    it('applies a previewed preset as one revisioned component replacement', async () => {
        const {root, session, button, click, changePreset, execute, snapshot} = await setup();
        expect(root.textContent).toContain('Emitter on Mist');
        expect(root.querySelector('canvas[aria-label="Animated particle preview"]')).not.toBeNull();
        expect(button('Apply').disabled).toBe(true);
        await changePreset('Fountain');
        expect(execute).not.toHaveBeenCalled();
        expect(session.candidate()).toEqual(PARTICLE_PRESETS[1].settings);
        await click('Pause');
        expect(button('Play')).toBeDefined();
        await click('Restart');
        await click('Apply');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'replace_component',
            key: 'mist',
            component: {
                id: 'fixture-emitter', schema_version: 1, enabled: true,
                data: {kind: 'particle_emitter', settings: PARTICLE_PRESETS[1].settings},
            },
        }, 2, 8);
        expect(snapshot().revision).toBe(9);
        expect(button('Apply').disabled).toBe(true);
    });

    it('protects an unsaved draft on project departure and restores focus when discarded', async () => {
        const {fixture, root, session, changePreset, click, execute, opener, button} = await setup();
        await changePreset('Sparks');
        button('Pause').focus();
        expect(session.canLeaveProject()).toBe(false);
        await fixture.whenStable();
        expect(root.textContent).toContain('Unsaved particle settings');
        await click('Keep Editing');
        expect(root.querySelector('dialog[open]')).not.toBeNull();
        const cancel = new Event('cancel', {cancelable: true});
        root.querySelector('dialog')!.dispatchEvent(cancel);
        await fixture.whenStable();
        await click('Discard and Close');
        expect(execute).not.toHaveBeenCalled();
        expect(root.querySelector('dialog[open]')).toBeNull();
        expect(document.activeElement).toBe(opener);
    });

    it('blocks invalid or stale drafts and leaves the user free to discard them', async () => {
        const {
            fixture,
            root,
            session,
            snapshot,
            workspaceGeneration,
            execute,
            button,
            click,
            changePreset
        } = await setup();
        const field = [...root.querySelectorAll('label')].find(label => label.textContent?.includes('Random seed'))!.querySelector('input')!;
        field.value = '4294967296';
        field.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        expect(button('Apply').disabled).toBe(true);
        expect(field.getAttribute('aria-invalid')).toBe('true');
        await changePreset('Fountain');
        snapshot.update(current => ({...current, document: {entities: [{...entity, particle_emitter: null}]}}));
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('emitter is no longer on this object');
        expect(button('Apply').disabled).toBe(true);
        workspaceGeneration.set(3);
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('project or scene changed');
        expect(session.canLeaveProject()).toBe(false);
        await fixture.whenStable();
        await click('Discard and Close');
        expect(session.canLeaveProject()).toBe(true);
        expect(execute).not.toHaveBeenCalled();
    });
});

describe('Particle preview step', () => {
    it('identifies preset values serialized by the native f32 runtime', () => {
        const settings = PARTICLE_PRESETS[1].settings;
        expect(particlePresetLabel({...settings, size: Math.fround(settings.size),
            color: [Math.fround(settings.color[0]), .75, 1]})).toBe('Fountain');
        expect(particlePresetLabel({...settings, size: .22})).toBe('');
    });
    it('matches fixed-step grouping, uses its seed and caps active particles', () => {
        const settings = {...PARTICLE_PRESETS[1].settings, rate_per_second: 128, lifetime_seconds: 10};
        const individual = new ParticlePreview(settings);
        const grouped = new ParticlePreview(settings);
        const reseeded = new ParticlePreview({...settings, seed: settings.seed + 1});
        for (let index = 0; index < 240; index++) {
            individual.advance(1 / 60);
            reseeded.advance(1 / 60);
        }
        for (let index = 0; index < 120; index++) grouped.advance(2 / 60);
        const particles = [...individual.positions()];
        expect(particles).toHaveLength(192);
        expect([...grouped.positions()]).toEqual(particles);
        expect([...reseeded.positions()]).not.toEqual(particles);
    });
});

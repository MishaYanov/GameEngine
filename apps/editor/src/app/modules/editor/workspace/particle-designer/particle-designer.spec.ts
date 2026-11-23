import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {
    EditorCommand,
    EditorSnapshot,
    ParticleEmitterComponent,
    SceneEntity
} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {DEFAULT_PARTICLE_EMITTER} from '../../../../core/editor/particle-emitter.models';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {ParticleDesigner} from './particle-designer';
import {ParticleDesignerSession} from './particle-designer-session';
import {PARTICLE_PRESETS} from './particle-designer.models';
import {ParticlePreview} from './particle-preview';

const emitter: ParticleEmitterComponent = {id: 'fixture-emitter', enabled: true, settings: DEFAULT_PARTICLE_EMITTER};
const entity: SceneEntity = {
    key: 'mist', name: 'Mist', model_path: '', renderable: false,
    transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    particle_emitter: emitter,
};

type DraftSnapshot = Pick<EditorSnapshot, 'mode' | 'revision' | 'active_scene_id'> & {
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

    async function setup() {
        vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
        const snapshot = signal<DraftSnapshot>({
            mode: 'edit', revision: 8, active_scene_id: 'yard', document: {entities: [entity]},
        });
        const workspaceGeneration = signal(2);
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
                {provide: EditorService, useValue: {snapshot, workspaceGeneration, execute}},
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
        expect(session.open(entity.key)).toBe(true);
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
        return {fixture, root, session, snapshot, workspaceGeneration, execute, button, click, changePreset, opener};
    }

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

import {TestBed} from '@angular/core/testing';
import {ComponentEditor, type ComponentEditIntent} from './component-editor';
import {
    validComponentRecord,
    type ComponentRecord,
    type ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';

const interactable: ComponentTypeDescriptor = {
    type_id: 'engine.interactable', kind: 'interactable', label: 'Interactable',
    description: 'Game action and reach.', max_per_object: 1,
};
const nativeBehavior: ComponentTypeDescriptor = {
    type_id: 'engine.native_behavior', kind: 'native_behavior', label: 'Native Behavior',
    description: 'Built-in behavior.', max_per_object: 8,
};

describe('Interactable Inspector', () => {
    async function setup() {
        const fixture = TestBed.createComponent(ComponentEditor);
        const host: HTMLElement = fixture.nativeElement;
        const intents: ComponentEditIntent[] = [];
        fixture.componentInstance.actionRequested.subscribe(intent => intents.push(intent));
        fixture.componentRef.setInput('types', [interactable]);
        fixture.componentRef.setInput('editable', true);
        fixture.componentRef.setInput('ownerKey', 'lever');
        fixture.componentRef.setInput('contextKey', 'main:lever');
        fixture.componentRef.setInput('gameActions', ['interact', 'inspect']);
        await fixture.whenStable();
        const button = (name: string) => Array.from(host.querySelectorAll('button'))
            .find(button => button.textContent.trim() === name)!;
        const field = (name: string) => Array.from(host.querySelectorAll('label'))
            .find(label => label.textContent.trim().startsWith(name))!.querySelector('input')!;
        const set = async (name: string, value: string) => {
            const input = field(name);
            input.value = value;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        };
        return {fixture, host, intents, button, field, set};
    }

    it('chooses Animator from the shared picker and explains an unavailable Controls Graph', async () => {
        const panel = await setup();
        const chosen: string[] = [];
        panel.fixture.componentInstance.specialistAttachmentRequested.subscribe(kind => chosen.push(kind));
        panel.fixture.componentRef.setInput('specialistAttachments', [
            {id: 'animator'}, {id: 'controls_graph', unavailableReason: 'Attach a Player Controller first.'},
        ]);
        await panel.fixture.whenStable();
        panel.host.querySelector<HTMLInputElement>('input[role="combobox"]')!.click();
        await panel.fixture.whenStable();
        const options = Array.from(panel.host.querySelectorAll<HTMLElement>('[role="option"]'));
        const controls = options.find(option => option.textContent?.includes('Controls Graph'))!;
        expect(controls.textContent).toContain('Attach a Player Controller first.');
        expect(controls.getAttribute('aria-disabled')).toBe('true');
        options.find(option => option.textContent?.includes('Animator'))!.click();
        await panel.fixture.whenStable();
        panel.button('Add Component').click();
        expect(chosen).toEqual(['animator']);
        expect(panel.intents).toEqual([]);
    });

    it('authors projectile movement and impact settings from the Inspector', async () => {
        const panel = await setup();
        await openNativeBehavior(panel, 'projectile');
        await panel.set('Speed', '6');
        await panel.set('Lifetime (ticks)', '18');
        await panel.set('Particle count', '5');
        panel.button('Apply Component').click();
        expect(panel.intents).toMatchObject([{
            kind: 'add', component: {
                data: {
                    kind: 'native_behavior', behavior: {
                        kind: 'projectile', settings: {
                            speed: 6, lifetime_ticks: 18, collision_mask: 0xffff_ffff, impact: {count: 5},
                        }
                    },
                }
            }
        }]);
    });

    it('binds a projectile Prop to a semantic input in the Inspector', async () => {
        const panel = await setup();
        await openNativeBehavior(panel, 'projectile_launcher');
        await panel.set('Launch Input Action', 'throw');
        await panel.set('Projectile Prop path', 'props/orb.json');
        await panel.set('Maximum alive', '2');
        panel.button('Apply Component').click();
        expect(panel.intents).toMatchObject([{
            kind: 'add', component: {
                data: {
                    kind: 'native_behavior', behavior: {
                        kind: 'projectile_launcher', settings: {
                            action: 'throw', prefab_path: 'props/orb.json', max_alive: 2, spawn_offset: [0, 0, -0.5],
                        }
                    },
                }
            }
        }]);
    });

    async function openNativeBehavior(panel: Awaited<ReturnType<typeof setup>>, kind: string): Promise<void> {
        panel.fixture.componentRef.setInput('types', [nativeBehavior]);
        await panel.fixture.whenStable();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        const select = panel.host.querySelector<HTMLSelectElement>('.component-form select')!;
        select.value = kind;
        select.dispatchEvent(new Event('input', {bubbles: true}));
        select.dispatchEvent(new Event('change', {bubbles: true}));
        await panel.fixture.whenStable();
    }

    it('focuses the named draft and returns to Add Component on discard', async () => {
        const panel = await setup();
        const add = panel.button('Add Component');
        add.click();
        await panel.fixture.whenStable();

        const form = panel.host.querySelector<HTMLElement>('fieldset.component-form')!;
        expect(form.querySelector('legend')?.textContent).toBe('Add Interactable');
        expect(panel.host.querySelector<HTMLElement>('.component-list')?.hidden).toBe(true);
        expect(document.activeElement).toBe(form);

        panel.button('Discard Component Draft').click();
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('fieldset.component-form')).toBeNull();
        expect(panel.host.querySelector<HTMLElement>('.component-list')?.hidden).toBe(false);
        expect(document.activeElement).toBe(add);
        expect(panel.intents).toEqual([]);
    });

    it('waits for the applied edit to settle before restoring focus to the opener', async () => {
        const panel = await setup();
        const add = panel.button('Add Component');
        add.click();
        await panel.fixture.whenStable();
        panel.button('Apply Component').click();

        panel.fixture.componentRef.setInput('pending', true);
        panel.fixture.componentRef.setInput('acknowledged', 1);
        await panel.fixture.whenStable();
        expect(add.disabled).toBe(true);
        expect(panel.host.querySelector('fieldset.component-form')).not.toBeNull();

        panel.fixture.componentRef.setInput('editable', false);
        panel.fixture.componentRef.setInput('pending', false);
        await panel.fixture.whenStable();
        expect(add.disabled).toBe(true);
        expect(panel.host.querySelector('fieldset.component-form')).not.toBeNull();

        panel.fixture.componentRef.setInput('editable', true);
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('fieldset.component-form')).toBeNull();
        expect(document.activeElement).toBe(add);
    });

    it('returns to the Components heading after an edit if its opener was removed', async () => {
        const panel = await setup();
        const component: ComponentRecord = {
            id: 'interact', schema_version: 1, enabled: true,
            data: {kind: 'interactable', settings: {action: 'interact', reach: 1}}
        };
        panel.fixture.componentRef.setInput('components', [component]);
        await panel.fixture.whenStable();

        panel.host.querySelector<HTMLButtonElement>('button[aria-label="Edit Interactable"]')!.click();
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('fieldset.component-form legend')?.textContent).toBe('Edit Interactable');
        panel.button('Apply Component').click();
        panel.fixture.componentRef.setInput('components', []);
        panel.fixture.componentRef.setInput('acknowledged', 1);
        await panel.fixture.whenStable();

        expect(panel.host.querySelector('fieldset.component-form')).toBeNull();
        expect(document.activeElement).toBe(panel.host.querySelector('.components h3'));
        expect(panel.intents).toHaveLength(1);
    });

    it('submits one typed record with the authored action and reach', async () => {
        const panel = await setup();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        expect(Array.from(panel.host.querySelectorAll('datalist option'), option => option.getAttribute('value')))
            .toEqual(['interact', 'inspect']);
        await panel.set('Game action ID', 'inspect');
        await panel.set('Reach (metres)', '1.25');
        panel.button('Apply Component').click();
        await panel.fixture.whenStable();
        expect(panel.intents).toEqual([expect.objectContaining({
            kind: 'add', expected: null, component: expect.objectContaining({
                schema_version: 1,
                enabled: true,
                data: {kind: 'interactable', settings: {action: 'inspect', reach: 1.25}},
            }),
        })]);
    });

    it('replaces a reviewed model with its catalog identity and preserves material', async () => {
        const panel = await setup();
        const material = {base_color_factor: [0.4, 0.5, 0.6, 1] as const};
        const mesh: ComponentRecord = {id: 'mesh', schema_version: 1, enabled: true,
            data: {kind: 'mesh_renderer', model_path: 'missing.glb', mesh_index: 3, material}};
        panel.fixture.componentRef.setInput('components', [mesh]);
        panel.fixture.componentRef.setInput('models', [
            {name: 'Replacement Cube', path: 'models/cube.glb', asset_id: 'cube-id', kind: 'model'},
            {name: 'Bell', path: 'audio/bell.wav', asset_id: 'bell-id', kind: 'audio_clip'},
        ]);
        await panel.fixture.whenStable();
        panel.host.querySelector<HTMLButtonElement>('button[aria-label="Edit Mesh Renderer"]')!.click();
        await panel.fixture.whenStable();
        expect(panel.host.textContent).toContain('Missing target: missing.glb');
        panel.host.querySelector<HTMLInputElement>('.component-form input[role="combobox"]')!.click();
        await panel.fixture.whenStable();
        const options = Array.from(panel.host.querySelectorAll<HTMLElement>('[role="option"]'));
        expect(options.some(option => option.textContent?.includes('Bell'))).toBe(false);
        options.find(option => option.textContent?.includes('Replacement Cube'))!.click();
        await panel.fixture.whenStable();
        panel.button('Apply Component').click();
        expect(panel.intents).toEqual([{kind: 'replace', expected: JSON.stringify(mesh), component: {
            ...mesh, data: {
                kind: 'mesh_renderer', model_path: 'models/cube.glb', asset_id: 'cube-id', mesh_index: null, material,
            },
        }}]);
    });

    it('keeps an existing mesh index until an author explicitly selects a stable mesh', async () => {
        const panel = await setup();
        const meshId = '41111111-4111-4111-8111-411111111111';
        const existing: ComponentRecord = {id: 'mesh', schema_version: 1, enabled: true, data: {
            kind: 'mesh_renderer', model_path: 'models/tower.glb', asset_id: 'tower-id', mesh_index: 3,
        }};
        panel.fixture.componentRef.setInput('components', [existing]);
        panel.fixture.componentRef.setInput('models', [{
            name: 'Tower', path: 'models/tower.glb', asset_id: 'tower-id',
            meshes: [{id: meshId, index: 8, name: 'Roof'}],
        }]);
        await panel.fixture.whenStable();
        panel.host.querySelector<HTMLButtonElement>('button[aria-label="Edit Mesh Renderer"]')!.click();
        await panel.fixture.whenStable();
        expect(panel.field('Mesh index').value).toBe('3');
        panel.button('Apply Component').click();
        const legacyEdit = panel.intents[0];
        const legacy = legacyEdit?.kind === 'replace' ? legacyEdit.component : undefined;
        expect(legacy?.data).toEqual({...existing.data, material: null});

        const picker = panel.host.querySelector<HTMLInputElement>('input[placeholder="Find a mesh…"]')!;
        expect(picker.value).toBe('Use mesh index');
        picker.click();
        await panel.fixture.whenStable();
        const roof = [...panel.host.querySelectorAll<HTMLElement>('[aria-label="Mesh results"] [role="option"]')]
            .find(option => option.textContent.trim() === 'Roof (8)')!;
        roof.click();
        await panel.fixture.whenStable();
        expect(picker.value).toBe('Roof (8)');
        panel.button('Apply Component').click();
        const stableEdit = panel.intents[1];
        const stable = stableEdit?.kind === 'replace' ? stableEdit.component : undefined;
        expect(stable?.data).toEqual({...existing.data, mesh_index: 8, mesh_id: meshId, material: null});
        expect(validComponentRecord(stable)).toBe(true);
    });

    it('edits Point Light settings through the same object component Inspector', async () => {
        const panel = await setup();
        const light: ComponentRecord = {id: 'lamp', schema_version: 1, enabled: true, data: {
            kind: 'light', settings: {kind: 'point', color: [1, 0.75, 0.5], intensity: 1, range: 8},
        }};
        panel.fixture.componentRef.setInput('components', [light]);
        await panel.fixture.whenStable();
        panel.host.querySelector<HTMLButtonElement>('button[aria-label="Edit Light"]')!.click();
        await panel.fixture.whenStable();
        await panel.set('Intensity', '2.5');
        await panel.set('Range (metres)', '12');
        panel.button('Apply Component').click();
        expect(panel.intents).toEqual([{kind: 'replace', expected: JSON.stringify(light), component: {
            ...light, data: {kind: 'light', settings: {kind: 'point', color: [1, 0.75, 0.5], intensity: 2.5, range: 12}},
        }}]);
    });

    it('attaches a Box from the catalog without treating a plane mesh as its collision shape', async () => {
        const panel = await setup();
        panel.fixture.componentRef.setInput('types', [{
            type_id: 'engine.collider', kind: 'collider', label: 'Collider', description: 'Collision shape',
            max_per_object: 1,
        }]);
        const box = {kind: 'box', half_extents: [0.5, 0.05, 0.5], offset: [0, -0.05, 0]};
        panel.fixture.componentRef.setInput('initialCollider', box);
        await panel.fixture.whenStable();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        expect(panel.host.querySelector<HTMLSelectElement>('.component-form select')!.value).toBe('box');
        panel.button('Apply Component').click();
        expect(panel.intents).toEqual([expect.objectContaining({kind: 'add', component: expect.objectContaining({
            data: {kind: 'collider', collider: box},
        })})]);
    });

    it('keeps invalid input in a draft until selection changes', async () => {
        const panel = await setup();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        await panel.set('Reach (metres)', '3.5');
        expect(panel.button('Apply Component').disabled).toBe(true);
        expect(panel.host.textContent).toContain('Reach must be positive');
        panel.button('Discard Component Draft').click();
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('.component-form')).toBeNull();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        panel.fixture.componentRef.setInput('contextKey', 'main:other');
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('.component-form')).toBeNull();
        expect(panel.intents).toEqual([]);
    });

    it('keeps a focused component action stable through snapshot reorder and shows its saved status', async () => {
        const panel = await setup();
        const interaction: ComponentRecord = {
            id: 'interaction', schema_version: 1, enabled: true,
            data: {kind: 'interactable', settings: {action: 'inspect', reach: 1.25}},
        };
        const mesh: ComponentRecord = {
            id: 'mesh', schema_version: 1, enabled: true,
            data: {kind: 'mesh_renderer', model_path: 'props/lever.glb'},
        };
        panel.fixture.componentRef.setInput('components', [mesh, interaction]);
        await panel.fixture.whenStable();

        const card = panel.host.querySelector<HTMLElement>('[data-reference-component="interaction"]')!;
        const edit = card.querySelector<HTMLButtonElement>('button[aria-label="Edit Interactable"]')!;
        expect(card.querySelector('.component-state')?.textContent?.trim()).toBe('Enabled');
        expect(card.querySelector('.binding')?.textContent?.trim()).toBe('inspect · 1.25 m');
        expect(panel.host.querySelector('[data-reference-component="mesh"] .binding')?.textContent).toContain('props/lever.glb');
        edit.focus();
        panel.fixture.componentRef.setInput('components', [{...interaction, enabled: false}, mesh]);
        await panel.fixture.whenStable();

        expect(document.activeElement).toBe(edit);
        expect(panel.host.querySelector('[data-reference-component="interaction"]')).toBe(card);
        expect(card.querySelector('.component-state')?.textContent?.trim()).toBe('Disabled');
        const more = card.querySelector<HTMLElement>('summary')!;
        expect(more.getAttribute('aria-label')).toBe('More actions for Interactable');
        more.focus();
        expect(document.activeElement).toBe(more);
        more.click();
        expect(card.querySelector('details')?.open).toBe(true);
        card.querySelector<HTMLButtonElement>('button[aria-label="Enable Interactable"]')!.click();
        expect(panel.intents).toEqual([{kind: 'enabled', component_id: 'interaction', enabled: true,
            expected: JSON.stringify({...interaction, enabled: false})}]);
    });

    it('shows the prefab source value and excludes native instance edit intents', async () => {
        const panel = await setup();
        const component = {
            id: 'interaction', schema_version: 1, enabled: true,
            data: {kind: 'interactable', settings: {action: 'interact', reach: 1.25}}
        };
        expect(validComponentRecord(component)).toBe(true);
        expect(validComponentRecord({
            ...component, data: {
                ...component.data,
                settings: {...component.data.settings, reach: 1e-100}
            }
        })).toBe(false);
        panel.fixture.componentRef.setInput('components', [component]);
        panel.fixture.componentRef.setInput('interactablePinned', true);
        await panel.fixture.whenStable();
        expect(panel.host.textContent).toContain('1.25 m');
        expect(panel.host.textContent).toContain('Edit the Prop source');
        expect(panel.host.querySelector('[aria-label="Edit Interactable"]')).toBeNull();
        expect(panel.button('Add Component').disabled).toBe(true);
    });
});

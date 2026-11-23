import {signal} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {type InspectorField, InspectorPanel, type InspectorTransformChange} from './inspector-panel';

const entity: SceneEntity = {
    key: 'robot',
    name: 'Robot',
    model_path: 'props/robot.glb',
    renderable: true,
    transform: {position: [1, 2, 3], rotation: [0, Math.PI / 2, 0], scale: [1, 1, 1]},
};

it('projects the active scene, native generation, and content revision from the mounted Inspector', async () => {
    const snapshot = signal({
        active_scene_id: 'left', revision: 12, scene_revision: 3, transaction: null,
        selection: [entity.key], document: {entities: [entity]}, project: {},
    });
    const native = signal({workspace_generation: 7});
    TestBed.configureTestingModule({providers: [
        {provide: EditorService, useValue: {snapshot, workspaceGeneration: signal(2)}},
        {provide: ViewportEditorService, useValue: {state: native}},
    ]});
    const fixture = TestBed.createComponent(InspectorPanel);
    fixture.componentRef.setInput('entity', entity);
    fixture.componentRef.setInput('editable', true);
    await fixture.whenStable();
    const inspector = fixture.componentInstance;
    expect(inspector.subject()).toEqual({
        address: {kind: 'scene', scene: 'left', object: entity.key},
        scope: {workspaceGeneration: 2, nativeGeneration: 7, revision: 12},
        contentRevision: 3, access: 'scene-edit',
    });
    inspector.revealReference(entity.key);
    snapshot.update(current => ({...current, active_scene_id: 'right', revision: 13, scene_revision: 1}));
    await fixture.whenStable();
    expect(inspector.subject()).toMatchObject({address: {scene: 'right'}, contentRevision: 1});
    expect(document.activeElement).not.toBe(fixture.nativeElement.querySelector('[data-reference-entity]'));
});

describe('InspectorPanel', () => {
    let fixture: ComponentFixture<InspectorPanel>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({imports: [InspectorPanel]}).compileComponents();
        fixture = TestBed.createComponent(InspectorPanel);
        await fixture.whenStable();
    });

    async function select(value = entity): Promise<void> {
        fixture.componentRef.setInput('entity', value);
        fixture.componentRef.setInput('editable', true);
        await fixture.whenStable();
    }

    function input(label: string): HTMLInputElement {
        return fixture.nativeElement.querySelector(`input[aria-label="${label}"]`);
    }

    it('shows the selection prompt until an entity is selected', () => {
        expect(fixture.nativeElement.textContent).toContain('Select an entity');
        expect(fixture.nativeElement.querySelector('input')).toBeNull();
    });

    it('shows selected metadata and local transforms, displaying radians as degrees', async () => {
        await select();
        expect(fixture.nativeElement.textContent).toContain('props/robot.glb');
        expect(fixture.nativeElement.textContent).toContain('Local space');
        expect(input('Position X').value).toBe('1');
        expect(input('Rotation Y (degrees)').value).toBe('90');
    });

    it('rejects a deferred reveal when another scene reuses the same entity key', async () => {
        await select();
        fixture.componentRef.setInput('contextKey', '1:scene-a');
        fixture.detectChanges();
        fixture.componentInstance.revealReference(entity.key);
        fixture.componentRef.setInput('contextKey', '1:scene-b');
        await fixture.whenStable();
        expect(document.activeElement).not.toBe(fixture.nativeElement.querySelector('[data-reference-entity]'));
    });

    it('emits rotation edits in radians and starts the correct transaction', async () => {
        await select();
        const started: InspectorField[] = [];
        const changed: InspectorTransformChange[] = [];
        fixture.componentInstance.editStarted.subscribe(value => started.push(value));
        fixture.componentInstance.transformChanged.subscribe(value => changed.push(value));
        const rotation = input('Rotation Y (degrees)');
        rotation.dispatchEvent(new FocusEvent('focus'));
        rotation.value = '180';
        rotation.dispatchEvent(new Event('input'));
        expect(started).toEqual(['rotation']);
        expect(changed).toEqual([{field: 'rotation', axis: 1, value: Math.PI, linked: false}]);
    });

    it('offers whole and per-channel resets that follow Inspector availability', async () => {
        await select();
        const reset = vi.fn();
        fixture.componentInstance.transformResetRequested.subscribe(reset);
        const root: HTMLElement = fixture.nativeElement;
        const buttons = [...root.querySelectorAll<HTMLButtonElement>('[aria-label="Reset local transform"] button')];
        expect(buttons.map(button => button.textContent?.trim())).toEqual([
            'Reset Transform', 'Reset Position', 'Reset Rotation', 'Reset Scale',
        ]);
        buttons.forEach(button => button.click());
        expect(reset.mock.calls).toEqual([['all'], ['position'], ['rotation'], ['scale']]);

        fixture.componentRef.setInput('editable', false);
        await fixture.whenStable();
        expect(buttons.every(button => button.disabled)).toBe(true);
    });

    it('links only the chosen transform vector and keeps that choice across selections', async () => {
        await select();
        const changed: InspectorTransformChange[] = [];
        fixture.componentInstance.transformChanged.subscribe(value => changed.push(value));
        const root: HTMLElement = fixture.nativeElement;
        const scaleLock = () => root.querySelector<HTMLButtonElement>(
            '[aria-label="Link Scale axes"]',
        )!;
        scaleLock().click();
        await fixture.whenStable();
        expect(scaleLock().getAttribute('aria-pressed')).toBe('true');

        await select({...entity, key: 'other'});
        expect(scaleLock().getAttribute('aria-pressed')).toBe('true');
        const scale = input('Scale X');
        scale.value = '3';
        scale.dispatchEvent(new Event('input'));
        const position = input('Position X');
        position.value = '2';
        position.dispatchEvent(new Event('input'));
        expect(changed).toEqual([
            {field: 'scale', axis: 0, value: 3, linked: true},
            {field: 'position', axis: 0, value: 2, linked: false},
        ]);
    });

    it('emits a name edit and cancels Escape without committing on the following blur', async () => {
        await select();
        const changed: string[] = [];
        let cancelled = 0;
        let committed = 0;
        fixture.componentInstance.nameChanged.subscribe(value => changed.push(value));
        fixture.componentInstance.editCancelled.subscribe(() => cancelled++);
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        const name: HTMLInputElement = fixture.nativeElement.querySelector('input[type="text"]');
        name.dispatchEvent(new FocusEvent('focus'));
        name.value = 'Runner';
        name.dispatchEvent(new Event('input'));
        name.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}));
        name.dispatchEvent(new FocusEvent('blur'));
        expect(changed).toEqual(['Runner']);
        expect(cancelled).toBe(1);
        expect(committed).toBe(0);
        expect(name.value).toBe('Robot');
    });

    it('replaces field drafts when another entity is selected', async () => {
        await select();
        const original = input('Position X');
        original.dispatchEvent(new FocusEvent('focus'));
        original.value = '';
        original.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        expect(original.getAttribute('aria-invalid')).toBe('true');
        await select({...entity, key: 'other', transform: {...entity.transform, position: [7, 8, 9]}});
        expect(input('Position X')).not.toBe(original);
        expect(input('Position X').value).toBe('7');
        expect(input('Position X').getAttribute('aria-invalid')).toBeNull();
    });

    it('replaces drafts across scenes even when entity keys are reused', async () => {
        await select();
        const original = input('Position X');
        original.dispatchEvent(new FocusEvent('focus'));
        original.value = '';
        original.dispatchEvent(new Event('input'));
        fixture.componentRef.setInput('contextKey', 'second-scene');
        await fixture.whenStable();
        expect(input('Position X')).not.toBe(original);
        expect(input('Position X').value).toBe('1');
    });

    it('passes invalid empty names to the edit owner and exposes cancellation even without selection', async () => {
        await select();
        const changed: string[] = [];
        let cancelled = 0;
        fixture.componentInstance.nameChanged.subscribe(value => changed.push(value));
        fixture.componentInstance.editCancelled.subscribe(() => cancelled++);
        const name: HTMLInputElement = fixture.nativeElement.querySelector('input[type="text"]');
        name.dispatchEvent(new FocusEvent('focus'));
        name.value = '';
        name.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        expect(changed).toEqual(['']);
        expect(name.getAttribute('aria-invalid')).toBe('true');
        fixture.componentRef.setInput('entity', null);
        fixture.componentRef.setInput('editError', 'Cancellation failed.');
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain('Cancellation failed');
        fixture.nativeElement.querySelector('.discard-edit').click();
        expect(cancelled).toBe(1);
    });

    it('makes all properties read-only and displays backend errors', async () => {
        await select();
        fixture.componentRef.setInput('editable', false);
        fixture.componentRef.setInput('editError', 'The scene changed; try again.');
        await fixture.whenStable();
        const fields: HTMLInputElement[] = [...fixture.nativeElement.querySelectorAll('input')];
        expect(fields.every(field => field.matches(':disabled'))).toBe(true);
        expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain('The scene changed');
    });

    it('has accessible property labels and field errors', async () => {
        await select();
        const field = input('Position X');
        field.dispatchEvent(new FocusEvent('focus'));
        field.value = '';
        field.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        const report = await axe.run(fixture.nativeElement, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});

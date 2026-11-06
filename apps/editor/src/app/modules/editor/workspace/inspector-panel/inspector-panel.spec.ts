import {ComponentFixture, TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {InspectorPanel, type InspectorField, type InspectorTransformChange} from './inspector-panel';

const entity: SceneEntity = {
    key: 'robot',
    name: 'Robot',
    model_path: 'props/robot.glb',
    renderable: true,
    transform: {position: [1, 2, 3], rotation: [0, Math.PI / 2, 0], scale: [1, 1, 1]},
};

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
        expect(changed).toEqual([{field: 'rotation', axis: 1, value: Math.PI}]);
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

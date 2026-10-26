import {Component, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {disabled, form, hidden, min} from '@angular/forms/signals';
import axe from 'axe-core';
import {EditorPane} from '../pane/pane';
import {EditorSubject} from './subject';
import type {SubjectInput} from '../../../core/subjects/subject.models';

@Component({
    imports: [EditorPane, EditorSubject],
    template: `<editor-pane title="Inspector" layout="cards">
		<editor-subject
			title="Transform"
			icon="cube"
			[form]="transformForm"
			[inputs]="inputs()"
			[(expanded)]="expanded"
			[(collapsed)]="collapsed"
			><button subject-actions type="button" (click)="actionCount.set(actionCount() + 1)">
				Action
			</button></editor-subject
		>
		<editor-subject title="Object" [form]="objectForm" />
	</editor-pane>`,
})
class SubjectHost {
    readonly inputs = signal<readonly SubjectInput[]>([]);
    readonly expanded = signal(false);
    readonly collapsed = signal(false);
    readonly actionCount = signal(0);
    readonly transform = signal({
        position: {x: 0, y: 0, z: 0},
        rotation: {x: 0, y: 0, z: 0},
        scale: {x: 1, y: 1, z: 1},
    });
    readonly rotationLocked = signal(false);
    readonly scaleHidden = signal(false);
    readonly transformHidden = signal(false);
    readonly transformForm = form(this.transform, (path) => {
        min(path.scale.x, 0.01, {message: 'Scale must be positive.'});
        disabled(path.rotation, {when: () => this.rotationLocked()});
        hidden(path.scale, {when: () => this.scaleHidden()});
        hidden(path, {when: () => this.transformHidden()});
    });
    readonly object = signal({name: 'Object', visible: true});
    readonly objectForm = form(this.object);
}

describe('Subject Signal Forms', () => {
    function setup() {
        const fixture = TestBed.createComponent(SubjectHost);
        fixture.detectChanges();
        const host = fixture.componentInstance;
        const element: HTMLElement = fixture.nativeElement;
        const field = (label: string) =>
            element.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
        return {fixture, host, element, field};
    }

    it('renders nested groups and stacks multiple cards within a pane', () => {
        const {element} = setup();
        expect(element.querySelectorAll('editor-subject')).toHaveLength(2);
        expect(element.querySelector('.content')?.classList.contains('cards')).toBe(true);
        expect([...element.querySelectorAll('legend')].map((node) => node.textContent?.trim())).toEqual(
            ['Position', 'Rotation', 'Scale'],
        );
        expect(element.querySelectorAll('input[type="number"]')).toHaveLength(9);
    });

    it('updates the source signal with numbers and reflects external model updates', () => {
        const {fixture, host, field} = setup();
        const x = field('Transform Position X');
        x.value = '2.5';
        x.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        expect(host.transform().position.x).toBe(2.5);
        expect(host.transformForm.position.x().dirty()).toBe(true);
        host.transform.update((value) => ({...value, position: {...value.position, y: 12}}));
        fixture.detectChanges();
        expect(field('Transform Position Y').value).toBe('12');
    });

    it('keeps cleared numeric inputs bound to their field', () => {
        const {fixture, field} = setup();
        const x = field('Transform Position X');
        x.value = '';
        x.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        expect(field('Transform Position X')).toBe(x);
        expect(x.type).toBe('number');
    });

    it('binds text and checkbox fields through Signal Forms', () => {
        const {fixture, host, field} = setup();
        const name = field('Object Name');
        name.value = 'Camera';
        name.dispatchEvent(new Event('input'));
        const visible = field('Object Visible');
        visible.click();
        fixture.detectChanges();
        expect(host.object()).toEqual({name: 'Camera', visible: false});
    });

    it('uses the supplied schema for disabled and hidden state', () => {
        const {fixture, host, field, element} = setup();
        host.rotationLocked.set(true);
        host.scaleHidden.set(true);
        fixture.detectChanges();
        expect(field('Transform Rotation X').disabled).toBe(true);
        expect(field('Transform Scale X')).toBeNull();
        host.transformHidden.set(true);
        fixture.detectChanges();
        expect(element.querySelector('editor-subject')?.hasAttribute('hidden')).toBe(true);
    });

    it('shows schema validation errors with an accessible field description', () => {
        const {fixture, host, field, element} = setup();
        host.transformForm.scale.x().value.set(-1);
        host.transformForm.scale.x().markAsTouched();
        fixture.detectChanges();
        const input = field('Transform Scale X');
        const errorId = input.getAttribute('aria-describedby');
        expect(errorId).toBeTruthy();
        expect(input.getAttribute('aria-invalid')).toBe('true');
        expect(
            [...element.querySelectorAll('.error')].find((node) => node.id === errorId)?.textContent,
        ).toContain('Scale must be positive.');
    });

    it('has accessible labels and grouping', async () => {
        const {element} = setup();
        const report = await axe.run(element, {rules: {'color-contrast': {enabled: false}}});
        expect(
            report.violations.map((violation) => ({
                id: violation.id,
                nodes: violation.nodes.map((node) => node.target),
            })),
        ).toEqual([]);
    });

    it('keeps primary traits visible and reveals configured groups on demand', () => {
        const {fixture, host, element, field} = setup();
        expect(element.querySelector('.traits-toggle')).toBeNull();
        host.inputs.set([
            {path: 'rotation', type: 'vector3', underExpanded: true},
            {path: 'scale', type: 'vector3', underExpanded: true},
        ]);
        fixture.detectChanges();
        expect(field('Transform Position X').closest('fieldset')?.hidden).toBe(false);
        expect(field('Transform Rotation X').closest('fieldset')?.hidden).toBe(true);
        const button = element.querySelector<HTMLButtonElement>('.traits-toggle')!;
        expect(button.parentElement?.lastElementChild).toBe(button);
        expect(button.closest('header')).toBeNull();
        expect(button.getAttribute('aria-label')).toContain(button.textContent?.trim());
        expect(button.getAttribute('aria-expanded')).toBe('false');
        button.click();
        fixture.detectChanges();
        expect(host.expanded()).toBe(true);
        expect(field('Transform Rotation X').closest('fieldset')?.hidden).toBe(false);
        expect(button.getAttribute('aria-controls')).toBe(
            element.querySelector('editor-subject-fields')?.id,
        );
    });

    it('can hide a nested trait without hiding its primary siblings', () => {
        const {fixture, host, field} = setup();
        host.inputs.set([{path: 'position.z', type: 'number', underExpanded: true}]);
        fixture.detectChanges();
        expect(field('Transform Position X').closest<HTMLElement>('.field')?.hidden).toBe(false);
        expect(field('Transform Position Z').closest<HTMLElement>('.field')?.hidden).toBe(true);
        expect(field('Transform Position X').closest('fieldset')?.hidden).toBe(false);
    });

    it('preserves mounted controls, edits, and validation while advanced traits are folded', () => {
        const {fixture, host, element, field} = setup();
        host.inputs.set([{path: 'scale', type: 'vector3', underExpanded: true}]);
        fixture.detectChanges();
        const original = field('Transform Scale X');
        host.transformForm.scale.x().value.set(-1);
        host.transformForm.scale.x().markAsTouched();
        fixture.detectChanges();
        expect(host.transformForm.scale.x().invalid()).toBe(true);
        expect(host.transformForm.scale.x().hidden()).toBe(false);
        expect(original.closest('fieldset')?.hidden).toBe(true);
        element.querySelector<HTMLButtonElement>('.traits-toggle')!.click();
        fixture.detectChanges();
        expect(field('Transform Scale X')).toBe(original);
        expect(original.value).toBe('-1');
        expect(original.getAttribute('aria-invalid')).toBe('true');
        element.querySelector<HTMLButtonElement>('.traits-toggle')!.click();
        fixture.detectChanges();
        expect(original.closest('fieldset')?.hidden).toBe(true);
        expect(host.transform().scale.x).toBe(-1);
    });

    it('minimizes to the header and restores the same form controls and values', () => {
        const {fixture, host, element, field} = setup();
        const input = field('Transform Position X');
        input.value = '12';
        input.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        const header = element.querySelector<HTMLElement>('editor-subject header')!;
        const content = element.querySelector<HTMLElement>('editor-subject .subject-content')!;
        header.click();
        fixture.detectChanges();
        expect(host.collapsed()).toBe(true);
        expect(content.hidden).toBe(true);
        expect(element.querySelector('.header-toggle')?.getAttribute('aria-expanded')).toBe('false');
        expect(field('Transform Position X')).toBe(input);
        header.click();
        fixture.detectChanges();
        expect(content.hidden).toBe(false);
        expect(input.value).toBe('12');
        expect(host.transform().position.x).toBe(12);
    });

    it('keeps header actions and advanced trait expansion independent from minimization', () => {
        const {fixture, host, element} = setup();
        host.inputs.set([
            {path: 'rotation', type: 'vector3', underExpanded: true},
            {path: 'scale', type: 'vector3', underExpanded: true},
        ]);
        fixture.detectChanges();
        element.querySelector<HTMLButtonElement>('.traits-toggle')!.click();
        element.querySelector<HTMLButtonElement>('[subject-actions]')!.click();
        fixture.detectChanges();
        expect(host.collapsed()).toBe(false);
        expect(host.expanded()).toBe(true);
        expect(host.actionCount()).toBe(1);
        const toggle = element.querySelector<HTMLButtonElement>('.header-toggle')!;
        toggle.click();
        fixture.detectChanges();
        expect(element.querySelector('.traits-toggle')).toBeNull();
        toggle.click();
        fixture.detectChanges();
        expect(element.querySelector('.traits-toggle')?.getAttribute('aria-expanded')).toBe('true');
    });

    it('has accessible controls when minimized', async () => {
        const {fixture, element, host} = setup();
        host.collapsed.set(true);
        fixture.detectChanges();
        const toggle = element.querySelector('.header-toggle')!;
        expect(toggle.getAttribute('aria-controls')).toBe(
            element.querySelector('.subject-content')?.id,
        );
        const report = await axe.run(element, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map((violation) => violation.id)).toEqual([]);
    });
});

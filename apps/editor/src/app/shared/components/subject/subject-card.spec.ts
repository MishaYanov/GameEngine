import {Component, inject} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {vi} from 'vitest';
import axe from 'axe-core';
import {defaultTransform} from '../../../core/subjects/subject.models';
import {SubjectStore} from '../../../core/subjects/subject.store';
import {LAYOUT_PREFERENCES_STORAGE} from '../../services/layout-preferences-storage';
import {Drawer} from '../drawer/drawer';
import {EditorPane} from '../pane/pane';
import {SubjectCard} from './subject-card';

@Component({
    providers: [SubjectStore],
    imports: [Drawer, EditorPane, SubjectCard],
    template: `<app-drawer
		label="Inspector"
		preferenceKey="test:inspector"
		[config]="{ width: '320px' }"
	>
		<editor-pane title="Inspector" icon="layers" layout="cards">
			@for (subject of store.subjects(); track subject.id) {
				<editor-subject-card [subjectId]="subject.id" />
			}
		</editor-pane>
	</app-drawer>`,
})
class InspectorHost {
    readonly store = inject(SubjectStore);

    constructor() {
        this.store.setScriptAssets([
            {
                assetId: 'player',
                path: 'scripts/player.rs',
                className: 'PlayerController',
                properties: [{name: 'speed', type: 'number', defaultValue: 5}],
            },
        ]);
        this.store.setSubjects([
            {id: 'object', kind: 'object', data: {name: 'Object', visible: true}},
            {id: 'transform', kind: 'transform', data: defaultTransform()},
            {id: 'script', kind: 'script', script: null, data: {enabled: true, properties: {}}},
        ]);
    }
}

describe('Typed inspector cards and drawer rail', () => {
    const load = vi.fn<() => Promise<unknown>>();
    const save = vi.fn().mockResolvedValue(undefined);
    beforeEach(() => {
        load.mockReset().mockResolvedValue(null);
        TestBed.configureTestingModule({
            providers: [{provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}}],
        });
    });

    async function setup() {
        const fixture = TestBed.createComponent(InspectorHost);
        fixture.detectChanges();
        await fixture.whenStable();
        const drawer: Drawer = fixture.debugElement.query(By.directive(Drawer)).componentInstance;
        await vi.waitFor(() => expect(drawer.configReady()).toBe(true));
        fixture.detectChanges();
        const element: HTMLElement = fixture.nativeElement;
        const input = (name: string) =>
            element.querySelector<HTMLInputElement>(`input[aria-label="${name}"]`)!;
        const button = (name: string) =>
            element.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!;
        return {fixture, element, input, button, store: fixture.componentInstance.store};
    }

    it('shows every Transform input without an extra expansion button', async () => {
        const {element} = await setup();
        const transform = element.querySelector<HTMLElement>(
            'editor-subject .subject[aria-label="Transform"]',
        )!;
        expect(transform).not.toBeNull();
        expect(transform.querySelector('.traits-toggle')).toBeNull();
        expect(transform.querySelectorAll('input[type="number"]')).toHaveLength(9);
        expect([...transform.querySelectorAll('fieldset')].every((group) => !group.hidden)).toBe(true);
    });

    it('commits Signal Forms edits to the typed store and accepts external updates', async () => {
        const {fixture, input, store} = await setup();
        input('Object Name').value = 'Camera';
        input('Object Name').dispatchEvent(new Event('input'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(store.subjects()[0].data).toEqual({name: 'Camera', visible: true});
        store.updateSubject({id: 'object', kind: 'object', data: {name: 'Light', visible: false}});
        fixture.detectChanges();
        expect(input('Object Name').value).toBe('Light');
    });

    it('minimizes from the header icon, shows every subject icon, and preserves mounted forms', async () => {
        const {fixture, element, input, button} = await setup();
        const original = input('Object Name');
        button('More Object traits').click();
        fixture.detectChanges();
        button('Minimize Inspector').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelector('app-drawer')?.classList.contains('collapsed')).toBe(true);
        expect(
            [...element.querySelectorAll('.subject-rail button')].map((button) =>
                button.getAttribute('aria-label'),
            ),
        ).toEqual(['Expand Inspector', 'Show Object', 'Show Transform', 'Show Script']);
        button('Show Object').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(input('Object Name')).toBe(original);
        expect(button('Less Object traits').getAttribute('aria-expanded')).toBe('true');
        expect(element.querySelector('app-drawer')?.classList.contains('collapsed')).toBe(false);
    });

    it('uses script property metadata to expose primary and advanced traits independently', async () => {
        const {fixture, element, input, store, button} = await setup();
        store.setScriptAssets([
            {
                assetId: 'player',
                path: 'scripts/player.rs',
                className: 'PlayerController',
                properties: [
                    {name: 'speed', type: 'number', defaultValue: 5},
                    {name: 'jump', type: 'number', defaultValue: 2, underExpanded: true},
                ],
            },
        ]);
        store.attachScript('script', 'player');
        fixture.detectChanges();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(input('PlayerController Properties Speed').closest<HTMLElement>('.field')?.hidden).toBe(
            false,
        );
        expect(input('PlayerController Properties Jump').closest<HTMLElement>('.field')?.hidden).toBe(
            true,
        );
        button('More PlayerController traits').click();
        fixture.detectChanges();
        expect(input('PlayerController Properties Jump').closest<HTMLElement>('.field')?.hidden).toBe(
            false,
        );
        expect(
            element
                .querySelector('button[aria-label="More Object traits"]')
                ?.getAttribute('aria-expanded'),
        ).toBe('false');
    });

    it('attaches and detaches a script using the Signal Forms selector', async () => {
        const {fixture, element, input, store} = await setup();
        const select = element.querySelector<HTMLSelectElement>('select')!;
        select.value = 'player';
        select.dispatchEvent(new Event('change'));
        select.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(input('PlayerController Properties Speed').value).toBe('5');
        input('PlayerController Properties Speed').value = '9';
        input('PlayerController Properties Speed').dispatchEvent(new Event('input'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(store.subjects()[2].data).toEqual({enabled: true, properties: {speed: 9}});
        select.value = '';
        select.dispatchEvent(new Event('change'));
        select.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(store.subjects()[2]).toMatchObject({script: null, data: {properties: {}}});
    });

    it('restores saved collapsed state and removes deleted subjects from the rail', async () => {
        load.mockResolvedValue({
            schema_version: 1,
            components: {'test:inspector': {collapsed: true, width: '360px'}},
        });
        const {fixture, element, store} = await setup();
        expect(element.querySelector('app-drawer')?.classList.contains('collapsed')).toBe(true);
        store.removeSubject('object');
        fixture.detectChanges();
        expect(element.querySelector('button[aria-label="Show Object"]')).toBeNull();
    });

    it('has accessible expanded and collapsed controls', async () => {
        const {fixture, element, button} = await setup();
        const options = {rules: {'color-contrast': {enabled: false}}};
        expect((await axe.run(element, options)).violations.map((v) => v.id)).toEqual([]);
        button('Minimize Inspector').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect((await axe.run(element, options)).violations.map((v) => v.id)).toEqual([]);
    });
});

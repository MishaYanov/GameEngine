import {TestBed} from '@angular/core/testing';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {FindUsages} from './find-usages';
import type {SceneReference} from './scene-references';

const target: SceneEntity = {
    key: 'player', name: 'Player', model_path: 'player.glb', renderable: true,
    transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
};
const reference: SceneReference = {
    id: 'camera-follow', group: 'Camera', field: 'Follow target', sourceName: 'Game Camera',
    targetKey: 'player', issue: null, location: {kind: 'camera', mode: 'follow'},
};

describe('Find Usages', () => {
    const prototype = HTMLDialogElement.prototype;
    const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const close = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            }
        });
        // The browser dispatches the close event after removing the modal from the top layer.
        Object.defineProperty(prototype, 'close', {
            configurable: true, value(this: HTMLDialogElement) {
                this.removeAttribute('open');
            }
        });
    });
    afterAll(() => {
        if (show) Object.defineProperty(prototype, 'showModal', show);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (close) Object.defineProperty(prototype, 'close', close);
        else Reflect.deleteProperty(prototype, 'close');
    });

    it('groups results for the target and reveals the selected source after the dialog closes', async () => {
        const fixture = TestBed.createComponent(FindUsages);
        const revealed: SceneReference[] = [];
        fixture.componentInstance.revealRequested.subscribe(reference => revealed.push(reference));
        fixture.componentRef.setInput('target', target);
        fixture.componentRef.setInput('references', [reference, {...reference, id: 'other', targetKey: 'other'}]);
        await fixture.whenStable();

        const host: HTMLElement = fixture.nativeElement;
        const dialog = host.querySelector('dialog')!;
        expect(dialog.open).toBe(true);
        const rows = [...dialog.querySelectorAll<HTMLElement>('[role="treeitem"]')];
        expect(rows.map(row => row.getAttribute('aria-label'))).toEqual(['Camera', 'Game Camera · Follow target']);
        rows[1].click();
        await fixture.whenStable();
        expect(dialog.open).toBe(false);
        expect(revealed).toEqual([]);
        dialog.dispatchEvent(new Event('close'));
        expect(revealed).toEqual([reference]);
    });

    it('drops a pending reveal if the scene context changes while the dialog closes', async () => {
        const fixture = TestBed.createComponent(FindUsages);
        const revealed: SceneReference[] = [];
        fixture.componentInstance.revealRequested.subscribe(reference => revealed.push(reference));
        fixture.componentRef.setInput('target', target);
        fixture.componentRef.setInput('references', [reference]);
        await fixture.whenStable();
        const host: HTMLElement = fixture.nativeElement;
        const dialog = host.querySelector('dialog')!;
        dialog.querySelector<HTMLElement>('[aria-label="Game Camera · Follow target"]')!.click();
        await fixture.whenStable();
        fixture.componentRef.setInput('contextKey', 'next-scene');
        fixture.componentRef.setInput('target', null);
        await fixture.whenStable();
        dialog.dispatchEvent(new Event('close'));
        expect(revealed).toEqual([]);
    });
});

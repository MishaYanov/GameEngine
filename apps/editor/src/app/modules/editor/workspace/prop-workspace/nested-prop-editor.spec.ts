import {TestBed} from '@angular/core/testing';
import type {ModelAsset} from '../../../../core/editor/editor.models';
import type {NestedPropPlacementRequest} from '../../../../core/editor/linked-prefabs.models';
import {NestedPropEditor} from './nested-prop-editor';

const path = 'prefabs/rooms/cinder-oath/southern-watchtower/architecture/upper-balcony/ancient-oak-door.prefab.json';
const object = {key: 'anchor', name: 'The unusually long balcony socket name for the northern oak drawbridge'};
const asset: ModelAsset = {asset_id: 'door', fingerprint: 'fingerprint', kind: 'prefab', name: 'Door', path};

describe('NestedPropEditor destinations', () => {
    it('opens a source by long path and a binding by name without changing stable IDs', async () => {
        const fixture = TestBed.createComponent(NestedPropEditor);
        const root = fixture.nativeElement as HTMLElement;
        const pick = async (name: string, text: string) => {
            const label = [...root.querySelectorAll('editor-search-picker label')].find(item => item.textContent?.trim() === name)!;
            const input = root.ownerDocument.getElementById(label.getAttribute('for')!) as HTMLInputElement;
            input.value = text;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
            expect(root.querySelector('[role="option"]')?.textContent).toContain(text);
            input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
            await fixture.whenStable();
        };
        const reviewed: { assetId: string; fingerprint: string }[] = [];
        const placements: NestedPropPlacementRequest[] = [];
        fixture.componentInstance.reviewRequested.subscribe(value => reviewed.push(value));
        fixture.componentInstance.placementRequested.subscribe(value => placements.push(value));
        fixture.componentRef.setInput('assets', [asset, {...asset, asset_id: 'owner'}]);
        fixture.componentRef.setInput('ownerAssetId', 'owner');
        fixture.componentRef.setInput('objects', [object]);
        fixture.componentRef.setInput('parent', object);
        fixture.componentRef.setInput('outerSlots', ['outside']);
        await fixture.whenStable();

        await pick('Source asset', 'upper-balcony/ancient-oak-door');
        expect(reviewed).toEqual([{assetId: 'door', fingerprint: 'fingerprint'}]);
        fixture.componentRef.setInput('review', {asset_id: 'door', fingerprint: 'fingerprint', slots: ['Socket']});
        await fixture.whenStable();
        const submit = root.querySelector<HTMLButtonElement>('button[type="submit"]')!;
        expect(submit.disabled).toBe(true);
        await pick('Socket', 'northern oak drawbridge');
        expect(submit.disabled).toBe(false);
        fixture.componentRef.setInput('pending', true);
        await fixture.whenStable();
        expect([...root.querySelectorAll<HTMLInputElement>('input[role="combobox"]')].every(input => input.disabled)).toBe(true);
        expect(submit.disabled).toBe(true);
        fixture.componentRef.setInput('pending', false);
        await fixture.whenStable();
        root.querySelector('form')!.dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        expect(placements).toEqual([expect.objectContaining({
            asset_id: 'door',
            fingerprint: 'fingerprint',
            parent: 'anchor',
            bindings: {Socket: {kind: 'object', key: 'anchor'}},
        })]);
    });
});

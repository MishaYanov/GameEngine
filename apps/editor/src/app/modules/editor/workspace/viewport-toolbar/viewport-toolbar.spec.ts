import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {DEFAULT_VIEWPORT_SETTINGS} from '../../../../core/editor/viewport-editor.models';
import {SceneEditingService} from '../scene-editing.service';
import {ViewportToolbar} from './viewport-toolbar';

describe('ViewportToolbar', () => {
    async function create() {
        const playing = signal(false); const gesture = signal(false); const settings = signal(DEFAULT_VIEWPORT_SETTINGS);
        const state = signal<{collider_error: string | null}>({collider_error: null});
        const action = vi.fn(async () => ({}));
        const withCommittedEdit = vi.fn(async (callback: () => Promise<void>) => {await callback(); return true;});
        TestBed.configureTestingModule({providers: [
            {provide: EditorService, useValue: {nativeAvailable: true, snapshot: signal({}), playing}},
            {provide: SceneEditingService, useValue: {withCommittedEdit}},
            {provide: ViewportEditorService, useValue: {nativeAvailable: true, settings, state, authoringGesture: gesture, error: signal(null), action}},
        ]});
        const fixture = TestBed.createComponent(ViewportToolbar); await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = (label: string) => [...root.querySelectorAll('button')].find(candidate => candidate.textContent.trim().startsWith(label))!;
        return {fixture, root, playing, gesture, settings, state, action, withCommittedEdit, button};
    }

    it('uses native editor actions rather than authored camera commands', async () => {
        const view = await create();
        view.button('Frame Selection').click();
        await view.fixture.whenStable();
        expect(view.action).toHaveBeenCalledWith({type: 'frame_selection'});
        view.button('Rotate').click();
        await view.fixture.whenStable();
        expect(view.withCommittedEdit).toHaveBeenCalledOnce();
        expect(view.action).toHaveBeenCalledWith({type: 'set_tool', tool: 'rotate'});
    });

    it('keeps navigation available but disables authored tools during Play', async () => {
        const view = await create(); view.playing.set(true); await view.fixture.whenStable();
        expect(view.button('Move').disabled).toBe(true); expect(view.button('Rotate').disabled).toBe(true);
        expect(view.button('Frame Selection').disabled).toBe(false); expect(view.button('Hand').disabled).toBe(false);
        view.button('Hand').click(); await view.fixture.whenStable();
        expect(view.action).toHaveBeenCalledWith({type: 'set_tool', tool: 'hand'});
        expect(view.withCommittedEdit).not.toHaveBeenCalled();
    });

    it('provides cancel during native gestures and preserves text-field key ownership', async () => {
        const view = await create(); view.button('Navigation').click(); await view.fixture.whenStable();
        const input = view.root.querySelector('input')!;
        input.dispatchEvent(new KeyboardEvent('keydown', {key: 'w', bubbles: true}));
        input.dispatchEvent(new KeyboardEvent('keydown', {key: 'f', bubbles: true}));
        expect(view.action).not.toHaveBeenCalled();
        view.gesture.set(true); await view.fixture.whenStable();
        expect(view.button('Move').disabled).toBe(true);
        view.button('Cancel Gesture').click(); await view.fixture.whenStable();
        expect(view.action).toHaveBeenCalledWith({type: 'cancel'});
    });

    it('toggles collider outlines from confirmed state without an authored edit', async () => {
        const view = await create();
        const button = view.button('Colliders');
        expect(button.getAttribute('aria-pressed')).toBe('false');
        button.click();
        await view.fixture.whenStable();
        expect(view.action).toHaveBeenCalledWith({type: 'set_colliders', visible: true});
        expect(view.withCommittedEdit).not.toHaveBeenCalled();
        expect(button.getAttribute('aria-pressed')).toBe('false');
        view.settings.update(settings => ({...settings, colliders: true}));
        await view.fixture.whenStable();
        expect(button.getAttribute('aria-pressed')).toBe('true');
        button.click();
        await view.fixture.whenStable();
        expect(view.action).toHaveBeenLastCalledWith({type: 'set_colliders', visible: false});
        view.settings.update(settings => ({...settings, view: 'game'}));
        await view.fixture.whenStable();
        expect(button.disabled).toBe(true);
    });

    it('shows and clears nonfatal collider diagnostics with native state', async () => {
        const view = await create();
        view.state.set({collider_error: 'Selected collider exceeds supported world bounds.'});
        await view.fixture.whenStable();
        expect(view.root.querySelector('[role="status"]')?.textContent).toContain('supported world bounds');
        expect(view.button('Colliders').disabled).toBe(false);
        view.state.set({collider_error: null});
        await view.fixture.whenStable();
        expect(view.root.querySelector('[role="status"]')).toBeNull();
    });

    it('exposes labels and pressed states without accessibility violations', async () => {
        const view = await create();
        view.button('Navigation').click(); await view.fixture.whenStable();
        const result = await axe.run(view.root, {rules: {'color-contrast': {enabled: false}}});
        expect(result.violations).toEqual([]);
    });
});

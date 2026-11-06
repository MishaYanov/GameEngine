import {Component, computed, inject, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorService} from '../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {DEFAULT_VIEWPORT_SETTINGS, type ViewportAction, type ViewportEditorState} from '../../../core/editor/viewport-editor.models';
import {SceneEditingService} from './scene-editing.service';
import {ViewportAssetDragService} from './viewport-asset-drag.service';

@Component({template: '<button type="button" (pointerdown)="drag.candidate(\'props/robot.glb\', $event)">Robot</button>'})
class DragHost {readonly drag = inject(ViewportAssetDragService);}

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => resolve = done);
    return {promise, resolve};
}

describe('Viewport asset drag', () => {
    const pointerId = 9;
    let action: ReturnType<typeof vi.fn>;
    let updatePlacement: ReturnType<typeof vi.fn>;
    let ready: ReturnType<typeof signal<boolean>>;
    let playing: ReturnType<typeof signal<boolean>>;
    let withCommittedEdit: ReturnType<typeof vi.fn>;
    let nativeState: ReturnType<typeof signal<ViewportEditorState | null>>;

    beforeEach(() => {
        action = vi.fn(async (_action: ViewportAction) => ({}));
        updatePlacement = vi.fn(async () => ({}));
        ready = signal(true); playing = signal(false);
        nativeState = signal<ViewportEditorState | null>(null);
        withCommittedEdit = vi.fn(async (callback: () => Promise<void>) => {await callback(); return true;});
        TestBed.configureTestingModule({providers: [ViewportAssetDragService,
            {provide: EditorService, useValue: {playing}},
            {provide: SceneEditingService, useValue: {ready, error: signal(null), withCommittedEdit}},
            {provide: ViewportEditorService, useValue: {
                nativeAvailable: true, settings: signal(DEFAULT_VIEWPORT_SETTINGS), state: nativeState, action, updatePlacement,
                normalizedPoint: (x: number, y: number) => x >= 100 && x <= 500 && y >= 50 && y <= 250 ? [(x - 100) / 400, (y - 50) / 200] : null,
            }},
        ]});
    });

    async function host() {
        const fixture = TestBed.createComponent(DragHost);
        await fixture.whenStable();
        const button = fixture.nativeElement.querySelector('button') as HTMLButtonElement;
        const capture = vi.fn(); const release = vi.fn();
        button.setPointerCapture = capture; button.hasPointerCapture = () => true; button.releasePointerCapture = release;
        return {fixture, button, capture, release, drag: fixture.componentInstance.drag};
    }

    function pointer(target: EventTarget, type: string, x: number, y: number) {
        const event = new MouseEvent(type, {clientX: x, clientY: y, button: 0, bubbles: true, cancelable: true});
        Object.defineProperty(event, 'pointerId', {value: pointerId});
        target.dispatchEvent(event);
    }

    it('keeps a click as asset selection without starting a native placement', async () => {
        const {fixture, button, capture, release} = await host();
        pointer(button, 'pointerdown', 10, 300);
        pointer(document, 'pointermove', 12, 302);
        pointer(document, 'pointerup', 12, 302);
        await fixture.whenStable();
        expect(capture).toHaveBeenCalledWith(pointerId);
        expect(release).toHaveBeenCalledWith(pointerId);
        expect(action).not.toHaveBeenCalled();
    });

    it('settles field edits and flushes the final normalized point before dropping', async () => {
        const {fixture, button, drag, release} = await host();
        const order: string[] = [];
        action.mockImplementation(async (command: ViewportAction) => {order.push(command.type); return {};});
        updatePlacement.mockImplementation(async () => {order.push('point'); return {};});
        pointer(button, 'pointerdown', 10, 300);
        pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'begin_placement', path: 'props/robot.glb'}));
        pointer(document, 'pointerup', 300, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'commit_placement'}));
        await fixture.whenStable();
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(updatePlacement).toHaveBeenLastCalledWith([0.5, 0.5]);
        expect(order.at(-2)).toBe('point'); expect(order.at(-1)).toBe('commit_placement');
        expect(drag.dragging()).toBe(false); expect(release).toHaveBeenCalledWith(pointerId);
    });

    it('cancels with Escape even while native placement is beginning', async () => {
        const {fixture, button, drag} = await host();
        const beginning = deferred<object>(); action.mockReturnValueOnce(beginning.promise);
        pointer(button, 'pointerdown', 10, 300);
        pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true}));
        beginning.resolve({});
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'cancel_placement'}));
        await fixture.whenStable();
        expect(action).not.toHaveBeenCalledWith({type: 'commit_placement'});
        expect(drag.dragging()).toBe(false);
    });

    it('cancels drops outside the aperture and releases capture after failure', async () => {
        const {fixture, button, release} = await host();
        pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        pointer(document, 'pointerup', 800, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'cancel_placement'}));
        await fixture.whenStable(); expect(release).toHaveBeenCalledWith(pointerId);
        expect(action).not.toHaveBeenCalledWith({type: 'commit_placement'});
    });

    it('cancels on application blur and does not intercept ordinary text Escape', async () => {
        const {fixture, button} = await host();
        pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        window.dispatchEvent(new Event('blur'));
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'cancel_placement'}));
        await fixture.whenStable();
        const event = new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true});
        document.dispatchEvent(event); expect(event.defaultPrevented).toBe(false);
    });

    it('blocks dragging during Play and when authoring is unavailable', async () => {
        const {fixture, button, capture} = await host();
        playing.set(true); pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        playing.set(false); ready.set(false); pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        await fixture.whenStable();
        expect(capture).not.toHaveBeenCalled(); expect(action).not.toHaveBeenCalled();
    });

    it('reports loading until native placement ends without claiming an unconfirmed outcome', async () => {
        const {fixture, button, drag} = await host();
        const loading: ViewportEditorState = {
            settings: DEFAULT_VIEWPORT_SETTINGS, camera_position: [0, 5, 10], camera_target: [0, 0, 0],
            camera_distance: 10, revision: 2, workspace_generation: 1, gesture: 'asset_loading',
            placement: {path: 'props/robot.glb', position: [0, 0, 0]}, focused: false, error: null,
        };
        action.mockImplementation(async (command: ViewportAction) => {
            if (command.type === 'commit_placement') nativeState.set(loading);
            return nativeState() ?? loading;
        });
        pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        pointer(document, 'pointerup', 300, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'commit_placement'}));
        await fixture.whenStable();
        expect(drag.message()).toBe('Loading and placing model…');
        // Focusing the HWND for camera navigation blurs its sibling WebView.
        window.dispatchEvent(new Event('blur'));
        await fixture.whenStable();
        expect(action).not.toHaveBeenCalledWith({type: 'cancel_placement'});
        nativeState.set({...loading, revision: 3, placement: null, gesture: null});
        await fixture.whenStable();
        // This same state can follow adoption or cancellation from the native child.
        expect(drag.message()).toBe('Model placement finished.');
    });

    it('allows Escape to cancel the background import after releasing the pointer', async () => {
        const {fixture, button, drag} = await host();
        const loading: ViewportEditorState = {
            settings: DEFAULT_VIEWPORT_SETTINGS, camera_position: [0, 5, 10], camera_target: [0, 0, 0],
            camera_distance: 10, revision: 2, workspace_generation: 1, gesture: 'asset_loading',
            placement: {path: 'props/robot.glb', position: [0, 0, 0]}, focused: false, error: null,
        };
        nativeState.set(loading);
        action.mockResolvedValue(loading);
        pointer(button, 'pointerdown', 10, 300); pointer(document, 'pointermove', 200, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        pointer(document, 'pointerup', 300, 150);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'commit_placement'}));
        await fixture.whenStable();
        const escape = new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true});
        document.dispatchEvent(escape);
        await vi.waitFor(() => expect(action).toHaveBeenCalledWith({type: 'cancel_placement'}));
        expect(escape.defaultPrevented).toBe(true);
        expect(drag.message()).toBe('Placement cancellation requested.');
    });
});

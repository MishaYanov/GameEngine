import {Directive, input, output, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {vi} from 'vitest';
import {EngineViewportDirective} from '../../../core/editor/engine-viewport.directive';
import {EditorClientError, EditorService} from '../../../core/editor/editor.service';
import type {ViewportInput, ViewportStatus} from '../../../core/editor/editor.models';
import {EnginePortal} from './portal';

@Directive({selector: '[engineViewport]', exportAs: 'engineViewport'})
class ViewportStub {
    readonly engineViewport = input(true);
    readonly engineViewportInput = output<ViewportInput>();
    readonly engineViewportStatus = output<ViewportStatus>();
    readonly engineViewportError = output<EditorClientError>();
    readonly refreshBounds = vi.fn();
}

describe('Vulkan portal', () => {
    it('shows a browser fallback without attempting a native attachment', async () => {
        TestBed.configureTestingModule({
            providers: [{provide: EditorService, useValue: {nativeAvailable: false}}],
        });
        const fixture = TestBed.createComponent(EnginePortal);
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Available in the desktop editor');
        expect(fixture.debugElement.query(By.directive(EngineViewportDirective))).toBeNull();
    });

    it('connects status, input, errors, enable state and bounds refresh to the native bridge', async () => {
        TestBed.configureTestingModule({
            providers: [
                {provide: EditorService, useValue: {nativeAvailable: true, viewport: signal(null)}},
            ],
        });
        TestBed.overrideComponent(EnginePortal, {
            remove: {imports: [EngineViewportDirective]},
            add: {imports: [ViewportStub]},
        });
        const fixture = TestBed.createComponent(EnginePortal);
        await fixture.whenStable();
        const directive: ViewportStub = fixture.debugElement
            .query(By.directive(ViewportStub))
            .injector.get(ViewportStub);
        const status = vi.fn();
        const errors = vi.fn();
        const inputs = vi.fn();
        fixture.componentInstance.viewportStatus.subscribe(status);
        fixture.componentInstance.viewportError.subscribe(errors);
        fixture.componentInstance.viewportInput.subscribe(inputs);
        const snapshot: ViewportStatus = {
            attached: true,
            visible: true,
            width: 640,
            height: 480,
            draw_calls: 1,
            instances: 1,
            triangles: 12,
            gpu_time_ms: null,
            gpu_timestamps_supported: false,
            error: null,
        };
        directive.engineViewportStatus.emit(snapshot);
        expect(status).toHaveBeenCalledWith(snapshot);
        const input: ViewportInput = {
            kind: 'pointer_down',
            position: [12, 24],
            extent: [640, 480],
            button: 'left',
            code: null,
            repeat: false,
            modifiers: {shift: false, control: false, alt: false, super_key: false},
            delta: null,
        };
        directive.engineViewportInput.emit(input);
        expect(inputs).toHaveBeenCalledWith(input);
        const error = new EditorClientError('backend', 'editor_viewport_attach', 'GPU unavailable');
        directive.engineViewportError.emit(error);
        fixture.detectChanges();
        expect(errors).toHaveBeenCalledWith(error);
        expect(fixture.nativeElement.textContent).toContain('GPU unavailable');
        fixture.componentInstance.refreshBounds();
        expect(directive.refreshBounds).toHaveBeenCalledOnce();
        fixture.componentRef.setInput('enabled', false);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(false);
    });

    it('hides the native surface for a modal and restores it when the modal is removed', async () => {
        TestBed.configureTestingModule({
            providers: [{provide: EditorService, useValue: {nativeAvailable: true}}],
        });
        TestBed.overrideComponent(EnginePortal, {
            remove: {imports: [EngineViewportDirective]},
            add: {imports: [ViewportStub]},
        });
        const query = document.querySelector.bind(document);
        // JSDOM has no top-layer modal state; map it to the modal's open attribute.
        const querySpy = vi
            .spyOn(document, 'querySelector')
            .mockImplementation((selector) =>
                query(selector === 'dialog:modal' ? 'dialog[open]' : selector),
            );
        const fixture = TestBed.createComponent(EnginePortal);
        await fixture.whenStable();
        const directive = fixture.debugElement
            .query(By.directive(ViewportStub))
            .injector.get(ViewportStub);
        const modal = document.createElement('dialog');
        modal.setAttribute('open', '');
        document.body.append(modal);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(false);
        });
        modal.remove();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(true);
        });
        querySpy.mockRestore();
    });
});

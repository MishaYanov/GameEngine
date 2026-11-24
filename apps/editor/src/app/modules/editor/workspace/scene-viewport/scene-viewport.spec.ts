import {Directive, input, output, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {vi} from 'vitest';
import {EngineViewportDirective} from '../../../../core/editor/engine-viewport.directive';
import {EditorClientError, EditorService} from '../../../../core/editor/editor.service';
import type {ViewportInput, ViewportStatus} from '../../../../core/editor/editor.models';
import {SceneViewport} from './scene-viewport';
import {ContextMenuService} from '../../../../shared/components/context-menu/context-menu.service';

@Directive({selector: '[engineViewport]', exportAs: 'engineViewport'})
class ViewportStub {
    readonly engineViewport = input(true);
    readonly engineViewportInput = output<ViewportInput>();
    readonly engineViewportStatus = output<ViewportStatus>();
    readonly engineViewportError = output<EditorClientError>();
    readonly refreshBounds = vi.fn();
}

describe('Scene viewport', () => {
    it('shows a browser fallback without attempting a native attachment', async () => {
        TestBed.configureTestingModule({
            providers: [{provide: EditorService, useValue: {nativeAvailable: false}}],
        });
        const fixture = TestBed.createComponent(SceneViewport);
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
        TestBed.overrideComponent(SceneViewport, {
            remove: {imports: [EngineViewportDirective]},
            add: {imports: [ViewportStub]},
        });
        const fixture = TestBed.createComponent(SceneViewport);
        await fixture.whenStable();
        const directive: ViewportStub = fixture.debugElement
            .query(By.directive(ViewportStub))
            .injector.get(ViewportStub);
        const status = vi.fn();
        const errors = vi.fn();
        const inputs = vi.fn();
        const actions = vi.fn();
        fixture.componentInstance.viewportStatus.subscribe(status);
        fixture.componentInstance.viewportError.subscribe(errors);
        fixture.componentInstance.viewportInput.subscribe(inputs);
        fixture.componentInstance.editorActionRequested.subscribe(actions);
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
        expect(actions).not.toHaveBeenCalled();
        directive.engineViewportInput.emit({
            ...input, kind: 'key_down', button: null, code: 'KeyS', modifiers: {...input.modifiers, control: true},
        });
        expect(actions).toHaveBeenCalledExactlyOnceWith('document.save');
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
        TestBed.overrideComponent(SceneViewport, {
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
        const fixture = TestBed.createComponent(SceneViewport);
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

    it('hides the surface for an HTML popover used by the top menu', async () => {
        TestBed.configureTestingModule({
            providers: [{provide: EditorService, useValue: {nativeAvailable: true}}],
        });
        TestBed.overrideComponent(SceneViewport, {
            remove: {imports: [EngineViewportDirective]},
            add: {imports: [ViewportStub]},
        });
        vi.stubGlobal('CSS', {supports: (feature: string) => feature === 'selector(:popover-open)'});
        const popover = document.createElement('div');
        const query = document.querySelector.bind(document);
        let open = false;
        const querySpy = vi.spyOn(document, 'querySelector').mockImplementation(selector =>
            selector === ':popover-open' ? (open ? popover : null) :
                query(selector === 'dialog:modal' ? 'dialog[open]' : selector));
        document.body.append(popover);
        try {
            const fixture = TestBed.createComponent(SceneViewport);
            await fixture.whenStable();
            const directive = fixture.debugElement.query(By.directive(ViewportStub)).injector.get(ViewportStub);

            open = true;
            popover.dispatchEvent(new Event('toggle'));
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(false);

            open = false;
            popover.dispatchEvent(new Event('toggle'));
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(true);
        } finally {
            popover.remove();
            querySpy.mockRestore();
            vi.unstubAllGlobals();
        }
    });

    it('hides the native surface for context menus and respects disabled and modal state when they close', async () => {
        TestBed.configureTestingModule({
            providers: [{provide: EditorService, useValue: {nativeAvailable: true}}],
        });
        TestBed.overrideComponent(SceneViewport, {
            remove: {imports: [EngineViewportDirective]},
            add: {imports: [ViewportStub]},
        });
        const query = document.querySelector.bind(document);
        const querySpy = vi.spyOn(document, 'querySelector').mockImplementation(selector =>
            query(selector === 'dialog:modal' ? 'dialog[open]' : selector));
        const fixture = TestBed.createComponent(SceneViewport);
        await fixture.whenStable();
        const directive = fixture.debugElement.query(By.directive(ViewportStub)).injector.get(ViewportStub);
        const menu = TestBed.inject(ContextMenuService);
        const owner: HTMLElement = fixture.nativeElement;
        const openMenu = () => menu.open({
            label: 'Entity actions', owner, returnFocus: owner, x: 10, y: 10,
            items: [{
                kind: 'action', id: 'select', label: 'Select', disabled: false,
                destructive: false, execute: vi.fn()
            }],
        });

        expect(directive.engineViewport()).toBe(true);
        expect(openMenu()).toBe(true);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(false);
        menu.close(false);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(true);

        openMenu();
        fixture.componentRef.setInput('enabled', false);
        menu.close(false);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(false);
        fixture.componentRef.setInput('enabled', true);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(true);

        const modal = document.createElement('dialog');
        modal.setAttribute('open', '');
        document.body.append(modal);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(false);
        });
        openMenu();
        menu.close(false);
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(false);
        modal.remove();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(true);
        });

        document.body.append(modal);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(false);
        });
        openMenu();
        modal.remove();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(directive.engineViewport()).toBe(false);

        const actions = vi.fn();
        const inputs = vi.fn();
        fixture.componentInstance.editorActionRequested.subscribe(actions);
        fixture.componentInstance.viewportInput.subscribe(inputs);
        const saveInput: ViewportInput = {
            kind: 'key_down', code: 'KeyS', repeat: false, position: null, extent: [640, 480], button: null, delta: null,
            modifiers: {shift: false, control: true, alt: false, super_key: false},
        };
        directive.engineViewportInput.emit(saveInput);
        expect(inputs).not.toHaveBeenCalled();
        expect(actions).not.toHaveBeenCalled();
        menu.close(false);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(directive.engineViewport()).toBe(true);
        });
        directive.engineViewportInput.emit(saveInput);
        expect(inputs).toHaveBeenCalledExactlyOnceWith(saveInput);
        expect(actions).toHaveBeenCalledExactlyOnceWith('document.save');
        querySpy.mockRestore();
    });
});

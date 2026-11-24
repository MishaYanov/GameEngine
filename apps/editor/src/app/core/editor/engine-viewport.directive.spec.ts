import {Component, input, viewChild} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorService} from './editor.service';
import {EngineViewportDirective} from './engine-viewport.directive';
import type {ViewportBounds, ViewportInput} from './editor.models';

@Component({
    imports: [EngineViewportDirective],
    template: '<div #target="engineViewport" [engineViewport]="enabled()"></div>',
})
class ViewportHarness {
    readonly enabled = input(true);
    readonly surface = viewChild.required(EngineViewportDirective);
}

function deferred() {
    let resolve!: () => void;
    const promise = new Promise<void>(complete => { resolve = complete; });
    return {promise, resolve};
}

describe('EngineViewportDirective', () => {
    afterEach(() => {
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
    });

    it('preserves a queued hide before revealing a viewport whose first bounds call is still pending', async () => {
        const calls: ViewportBounds[] = [];
        let onNativeInput: (event: ViewportInput) => void = () => undefined;
        const attach = deferred();
        const update = deferred();
        const secondHide = deferred();
        let hides = 0;
        const viewport = {
            nativeAvailable: true,
            viewport: () => null,
            reserveViewport: () => () => undefined,
            listenViewportInput: async (handler: typeof onNativeInput) => {
                onNativeInput = handler;
                return () => undefined;
            },
            attachViewport: (bounds: ViewportBounds) => {
                calls.push(bounds);
                return attach.promise;
            },
            updateViewportBounds: (bounds: ViewportBounds) => {
                calls.push(bounds);
                if (bounds.visible) return update.promise;
                return ++hides === 2 ? secondHide.promise : Promise.resolve();
            },
            detachViewport: vi.fn().mockResolvedValue(null),
        };
        vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
        const getStyle = window.getComputedStyle.bind(window);
        vi.spyOn(window, 'getComputedStyle').mockImplementation(element => {
            const style = getStyle(element);
            style.opacity ||= '1';
            return style;
        });
        vi.stubGlobal('ResizeObserver', class {
            observe(): void {}
            disconnect(): void {}
        });
        vi.stubGlobal('IntersectionObserver', class {
            constructor(private readonly notify: (entries: Array<Pick<IntersectionObserverEntry, 'target' | 'isIntersecting'>>) => void) {}
            observe(target: Element): void { this.notify([{target, isIntersecting: true}]); }
            disconnect(): void {}
        });
        vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(800);
        vi.spyOn(document.documentElement, 'clientHeight', 'get').mockReturnValue(600);
        TestBed.configureTestingModule({providers: [{provide: EditorService, useValue: viewport}]});
        const fixture = TestBed.createComponent(ViewportHarness);
        fixture.detectChanges();
        const div = (fixture.nativeElement as HTMLElement).querySelector('div')!;
        vi.spyOn(div, 'clientWidth', 'get').mockReturnValue(640);
        vi.spyOn(div, 'clientHeight', 'get').mockReturnValue(480);
        vi.spyOn(div, 'getBoundingClientRect').mockReturnValue(new DOMRect(10, 20, 640, 480));
        await vi.waitFor(() => expect(calls.map(bounds => bounds.visible)).toEqual([true]));

        fixture.componentRef.setInput('enabled', false);
        fixture.detectChanges();
        await fixture.whenStable();
        fixture.componentRef.setInput('enabled', true);
        fixture.detectChanges();
        await fixture.whenStable();
        await new Promise<void>(done => requestAnimationFrame(() => done()));

        attach.resolve();
        await vi.waitFor(() => expect(calls.map(bounds => bounds.visible)).toEqual([true, false, true]));

        const input = vi.fn();
        fixture.componentInstance.surface().engineViewportInput.subscribe(input);
        const save: ViewportInput = {
            kind: 'key_down', code: 'KeyS', repeat: false, position: null, extent: [640, 480], button: null, delta: null,
            modifiers: {shift: false, control: true, alt: false, super_key: false},
        };
        onNativeInput(save);
        expect(input).not.toHaveBeenCalled();
        update.resolve();
        await update.promise;
        await Promise.resolve();
        onNativeInput(save);
        expect(input).toHaveBeenCalledExactlyOnceWith(save);

        fixture.componentRef.setInput('enabled', false);
        fixture.detectChanges();
        await vi.waitFor(() => expect(calls.map(bounds => bounds.visible)).toEqual([true, false, true, false]));
        fixture.componentRef.setInput('enabled', true);
        fixture.detectChanges();
        await fixture.whenStable();
        await new Promise<void>(done => requestAnimationFrame(() => done()));
        onNativeInput(save);
        expect(input).toHaveBeenCalledTimes(1);

        secondHide.resolve();
        await vi.waitFor(() => expect(calls.map(bounds => bounds.visible)).toEqual([true, false, true, false, true]));
        onNativeInput(save);
        expect(input).toHaveBeenCalledTimes(2);
    });
});

import {Component, ErrorHandler, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import {ContextActionBuilder} from '../../actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from '../../builder/context-menu-builder';
import {EditorContextMenu} from './context-menu';
import {EditorContextMenuTrigger} from './context-menu-trigger';
import {ContextMenuService} from './context-menu.service';

@Component({
    imports: [EditorContextMenu, EditorContextMenuTrigger],
    template: `
        <button id="target" [editorContextMenu]="source" editorContextMenuLabel="Cube actions">Cube</button>
        <div id="outer" [editorContextMenu]="outer">
            <button id="inner" [editorContextMenu]="source">Inner</button>
        </div>
        <button id="empty" [editorContextMenu]="[]">No actions</button>
        <button id="outside">Outside</button>
        <editor-context-menu/>
    `,
})
class MenuHost {
    readonly enabled = signal(true);
    readonly calls: string[] = [];
    readonly actions = ContextMenuBuilder.create<string>()
        .action(ContextActionBuilder.create<string>('select', 'Select')
            .enabledWhen(() => this.enabled()).run(name => {
                this.calls.push(name);
            }).build())
        .action(ContextActionBuilder.create<string>('blocked', 'Disabled').enabledWhen(() => false)
            .run(() => {
                throw new Error('Disabled action ran');
            }).build())
        .separator()
        .submenu('parent', 'Parenting', builder => builder
            .action(ContextActionBuilder.create<string>('root', 'Move to Root')
                .run(name => {
                    this.calls.push(`root:${name}`);
                }).build())
            .submenu('more', 'More', nested => nested
                .action(ContextActionBuilder.create<string>('nested', 'Nested action')
                    .run(name => {
                        this.calls.push(`nested:${name}`);
                    }).build())));
    readonly source = () => this.actions.build('Cube');
    readonly outer = this.actions.build('Outer');
}

describe('EditorContextMenu', () => {
    const handleError = vi.fn();
    beforeEach(() => {
        handleError.mockReset();
        TestBed.configureTestingModule({providers: [{provide: ErrorHandler, useValue: {handleError}}]});
    });

    async function setup() {
        const fixture = TestBed.createComponent(MenuHost);
        fixture.detectChanges();
        await fixture.whenStable();
        const host: HTMLElement = fixture.nativeElement;
        const target = host.querySelector<HTMLElement>('#target')!;
        const context = async (element = target, x = 200, y = 100) => {
            const event = new MouseEvent('contextmenu', {
                bubbles: true,
                cancelable: true,
                button: 2,
                clientX: x,
                clientY: y
            });
            element.dispatchEvent(event);
            fixture.detectChanges();
            await fixture.whenStable();
            return event;
        };
        const items = () => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
        const item = (text: string) => items().find(button => button.textContent?.trim() === text)!;
        const key = async (key: string, options: KeyboardEventInit = {}) => {
            const event = new KeyboardEvent('keydown', {bubbles: true, cancelable: true, key, ...options});
            host.ownerDocument.activeElement?.dispatchEvent(event);
            fixture.detectChanges();
            await fixture.whenStable();
            return event;
        };
        return {fixture, host, target, context, item, key, service: TestBed.inject(ContextMenuService)};
    }

    it('replaces the native menu for actionable targets, focuses it and runs the clicked action', async () => {
        const {fixture, host, target, context, item, service} = await setup();
        expect((await context()).defaultPrevented).toBe(true);
        expect(host.querySelector('[role="menu"]')?.getAttribute('aria-label')).toBe('Cube actions');
        expect(host.ownerDocument.activeElement).toBe(item('Select'));
        item('Select').click();
        fixture.detectChanges();
        expect(fixture.componentInstance.calls).toEqual(['Cube']);
        expect(service.request()).toBeNull();
        expect(host.ownerDocument.activeElement).toBe(target);
    });

    it('opens using Shift+F10 and the ContextMenu key and restores focus on Escape', async () => {
        const {host, target, item, key, service} = await setup();
        for (const [value, shiftKey] of [['F10', true], ['ContextMenu', false]] as const) {
            target.focus();
            expect((await key(value, {shiftKey})).defaultPrevented).toBe(true);
            expect(host.ownerDocument.activeElement).toBe(item('Select'));
            await key('Escape');
            expect(service.request()).toBeNull();
            expect(host.ownerDocument.activeElement).toBe(target);
        }
    });

    it('navigates nested submenus, skips separators, and supports typeahead and Home/End', async () => {
        const {fixture, host, context, item, key} = await setup();
        await context();
        await key('End');
        expect(host.ownerDocument.activeElement).toBe(item('Parenting'));
        await key('ArrowRight');
        expect(host.ownerDocument.activeElement).toBe(item('Move to Root'));
        await key('ArrowDown');
        await key('ArrowRight');
        expect(host.ownerDocument.activeElement).toBe(item('Nested action'));
        await key('ArrowLeft');
        expect(host.ownerDocument.activeElement).toBe(item('More'));
        await key('Home');
        await key('Enter');
        expect(fixture.componentInstance.calls).toEqual(['root:Cube']);

        await context();
        await key('p');
        expect(host.ownerDocument.activeElement).toBe(item('Parenting'));
        await key('Home');
        await key('ArrowUp');
        expect(host.ownerDocument.activeElement).toBe(item('Parenting'));
    });

    it('keeps disabled items accessible and never executes them or actions that become stale', async () => {
        const {fixture, context, item, key, service} = await setup();
        await context();
        await key('ArrowDown');
        expect(item('Disabled').getAttribute('aria-disabled')).toBe('true');
        await key('Enter');
        expect(service.request()).not.toBeNull();
        fixture.componentInstance.enabled.set(false);
        item('Select').click();
        expect(fixture.componentInstance.calls).toEqual([]);
    });

    it('enters an already-open submenu after clicking its parent and reveals the focused row', async () => {
        const {fixture, host, context, item, key} = await setup();
        await context();
        item('Parenting').dispatchEvent(new Event('pointerenter'));
        fixture.detectChanges();
        await fixture.whenStable();
        item('Parenting').focus();
        item('Parenting').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(host.ownerDocument.activeElement).toBe(item('Move to Root'));
        const row = item('More');
        const panel = row.closest<HTMLElement>('[role="menu"]')!;
        panel.getBoundingClientRect = () => new DOMRect(0, 0, 260, 120);
        row.getBoundingClientRect = () => new DOMRect(0, 110, 260, 26);
        await key('End');
        expect(host.ownerDocument.activeElement).toBe(row);
        expect(panel.scrollTop).toBe(16);
        expect(host.scrollTop).toBe(0);
    });

    it('uses the closest nested trigger and leaves an empty menu to the browser', async () => {
        const {host, context, service} = await setup();
        await context(host.querySelector<HTMLElement>('#inner')!);
        expect(service.request()?.owner.id).toBe('inner');
        service.close(false);
        expect((await context(host.querySelector<HTMLElement>('#empty')!)).defaultPrevented).toBe(false);
        expect(service.request()).toBeNull();
    });

    it('keeps a fixed toolbar menu open through unrelated tree scrolling', async () => {
        const {host, context, service} = await setup();
        await context();
        host.querySelector('#outer')!.dispatchEvent(new Event('scroll'));
        expect(service.request()?.owner.id).toBe('target');

        await context(host.querySelector('#inner')!);
        host.querySelector('#outer')!.dispatchEvent(new Event('scroll'));
        expect(service.request()).toBeNull();
    });

    it('dismisses on outside interaction, scroll and resize without stealing outside focus', async () => {
        const {fixture, host, context, key, service} = await setup();
        await context();
        host.querySelector<HTMLElement>('#outside')!.focus();
        expect(service.request()).toBeNull();
        await context();
        document.dispatchEvent(new Event('scroll'));
        expect(service.request()).toBeNull();
        await context();
        window.dispatchEvent(new Event('resize'));
        expect(service.request()).toBeNull();
        await context();
        expect((await key('Tab')).defaultPrevented).toBe(false);
        expect(service.request()).toBeNull();
        await context();
        fixture.destroy();
        expect(service.request()).toBeNull();
    });

    it('clamps menus to the viewport and has accessible menu markup', async () => {
        const {host, context} = await setup();
        await context(undefined, 100000, 100000);
        const panel = host.querySelector<HTMLElement>('[role="menu"]')!;
        expect(Number.parseFloat(panel.style.left)).toBeLessThan(window.innerWidth);
        expect(Number.parseFloat(panel.style.top)).toBeLessThan(window.innerHeight);
        const report = await axe.run(host, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });

    it('routes synchronous and asynchronous action failures to Angular error handling', async () => {
        const {target, service} = await setup();
        const failure = new Error('Action failed');
        for (const execute of [() => {
            throw failure;
        }, () => Promise.reject(failure)]) {
            const items: readonly ContextMenuItem[] = [{
                kind: 'action',
                id: 'fail',
                label: 'Fail',
                disabled: false,
                destructive: false,
                execute
            }];
            service.open({items, label: 'Failure', x: 0, y: 0, owner: target, returnFocus: target});
            service.execute(items[0]);
            await Promise.resolve();
        }
        expect(handleError).toHaveBeenCalledTimes(2);
        expect(handleError).toHaveBeenCalledWith(failure);
    });
});

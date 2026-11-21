import {ComponentFixture, TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import {SceneEntitiesPanel} from './scene-entities-panel';
import {EditorContextMenuTrigger} from '../../../../shared/components/context-menu/context-menu-trigger';
import type {ContextMenuItem} from '../../../../shared/builder/context-menu-builder';

function entity(key: string, parent: string | null = null, name = key): SceneEntity {
    return {key, parent, name, model_path: 'props/robot.glb', renderable: true,
        transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]}};
}
const nested = [entity('child', 'parent'), entity('parent'), entity('grandchild', 'child'), entity('other')];

describe('SceneEntitiesPanel hierarchy', () => {
    let fixture: ComponentFixture<SceneEntitiesPanel>;
    let component: SceneEntitiesPanel;
    let root: HTMLElement;
    const requested = vi.fn();
    const reparented = vi.fn();

    beforeEach(async () => {
        localStorage.clear(); requested.mockReset(); reparented.mockReset();
        await TestBed.configureTestingModule({imports: [SceneEntitiesPanel]}).compileComponents();
        fixture = TestBed.createComponent(SceneEntitiesPanel);
        fixture.componentRef.setInput('scene', {projectOpen: false, entities: []});
        component = fixture.componentInstance; root = fixture.nativeElement;
        component.selectionRequested.subscribe(requested);
        component.reparentRequested.subscribe(reparented);
        await fixture.whenStable();
    });
    async function open(entities: readonly SceneEntity[] = nested, selectedKeys: readonly string[] = []): Promise<void> {
        fixture.componentRef.setInput('scene', {projectOpen: true, entities});
        fixture.componentRef.setInput('selectedKeys', selectedKeys);
        await fixture.whenStable();
    }
    function row(key: string): HTMLButtonElement {
        const result = root.querySelector<HTMLButtonElement>(`[data-key="${key}"]`);
        expect(result, `Missing hierarchy row: ${key}`).toBeTruthy(); return result!;
    }
    function tree(): HTMLElement {return root.querySelector('[role="tree"]')!;}
    function keys(): string[] {return [...root.querySelectorAll<HTMLElement>('[role="treeitem"]')].map(row => row.dataset['key']!);}
    async function keydown(key: string): Promise<void> {
        tree().dispatchEvent(new KeyboardEvent('keydown', {key, bubbles: true, cancelable: true}));
        await fixture.whenStable();
    }
    function dragEvent(type: string, transfer: DataTransfer): DragEvent {
        const event = new Event(type, {bubbles: true, cancelable: true});
        Object.defineProperty(event, 'dataTransfer', {value: transfer}); return event as DragEvent;
    }
    function transfer(): DataTransfer {
        const values = new Map<string, string>();
        return {effectAllowed: 'none', dropEffect: 'none',
            setData: (type: string, value: string) => values.set(type, value),
            getData: (type: string) => values.get(type) ?? ''} as unknown as DataTransfer;
    }
    async function drag(key: string): Promise<DataTransfer> {
        const data = transfer(); row(key).dispatchEvent(dragEvent('dragstart', data)); await fixture.whenStable(); return data;
    }
    function action(label: string): HTMLButtonElement {
        return [...root.querySelectorAll<HTMLButtonElement>('.hierarchy-actions button')].find(button => button.textContent?.trim() === label)!;
    }

    function contextAction(element: HTMLElement, id: string): Extract<ContextMenuItem, { kind: 'action' }> {
        const trigger = fixture.debugElement.queryAll(By.directive(EditorContextMenuTrigger))
            .find(candidate => candidate.nativeElement === element)!.injector.get(EditorContextMenuTrigger);
        const source = trigger.editorContextMenu();
        const items = typeof source === 'function' ? source() : source;
        const pending = [...items];
        while (pending.length) {
            const item = pending.shift()!;
            if (item.kind === 'action' && item.id === id) return item;
            if (item.kind === 'submenu') pending.push(...item.children);
        }
        throw new Error(`Missing entity context action: ${id}`);
    }

    it('builds entity actions for the clicked row independently of the current selection', async () => {
        await open(nested, ['parent']);
        const usages = vi.fn();
        component.usagesRequested.subscribe(usages);
        contextAction(row('child'), 'entity.find-usages').execute();
        expect(usages).toHaveBeenCalledExactlyOnceWith('child');
        expect(requested).not.toHaveBeenCalled();
        contextAction(row('child'), 'entity.select').execute();
        expect(requested).toHaveBeenLastCalledWith('child');
        contextAction(row('child'), 'entity.move-to-root').execute();
        expect(reparented).toHaveBeenLastCalledWith({key: 'child', parent: null});

        contextAction(row('child'), 'entity.set-parent').execute();
        row('other').click();
        expect(reparented).toHaveBeenLastCalledWith({key: 'child', parent: 'other'});
        expect(row('parent').getAttribute('aria-selected')).toBe('true');
    });

    it('uses the active descendant for keyboard context actions and rechecks stale hierarchy contexts', async () => {
        await open(nested, ['parent']);
        await keydown('ArrowDown');
        requested.mockReset();
        contextAction(tree(), 'entity.select').execute();
        expect(requested).toHaveBeenCalledExactlyOnceWith('child');
        const move = contextAction(row('child'), 'entity.move-to-root');
        fixture.componentRef.setInput('contextKey', 'another-scene');
        await fixture.whenStable();
        move.execute();
        expect(reparented).not.toHaveBeenCalled();
    });

    it('reveals and focuses a usage source through collapsed ancestors', async () => {
        await open(nested, ['parent']);
        row('parent').querySelector<HTMLElement>('.disclosure')!.click();
        await fixture.whenStable();
        expect(keys()).not.toContain('grandchild');
        component.revealEntity('grandchild');
        await fixture.whenStable();
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('grandchild').id);
        expect(document.activeElement).toBe(tree());
        expect(requested).not.toHaveBeenCalled();
    });

    it('rechecks disabled and removed entities before executing an open context action', async () => {
        await open(nested, ['parent']);
        const select = contextAction(row('child'), 'entity.select');
        const move = contextAction(row('child'), 'entity.move-to-root');
        fixture.componentRef.setInput('disabled', true);
        await fixture.whenStable();
        select.execute();
        move.execute();
        expect(requested).not.toHaveBeenCalled();
        expect(reparented).not.toHaveBeenCalled();
        fixture.componentRef.setInput('disabled', false);
        await open([entity('parent')], ['parent']);
        select.execute();
        move.execute();
        expect(requested).not.toHaveBeenCalled();
        expect(reparented).not.toHaveBeenCalled();
    });

    it('keeps project-closed and empty-scene states readable', async () => {
        expect(root.textContent).toContain('Open a project to view its scene entities.');
        expect(root.querySelector('[role="tree"]')).toBeNull();
        await open([]);
        expect(root.textContent).toContain('This scene contains no entities.');
        expect(root.querySelector('[role="treeitem"]')).toBeNull();
    });

    it('presents parent/child order and complete ARIA metadata even when source order is interleaved', async () => {
        await open();
        expect(keys()).toEqual(['parent', 'child', 'grandchild', 'other']);
        expect(row('parent').getAttribute('aria-level')).toBe('1');
        expect(row('child').getAttribute('aria-level')).toBe('2');
        expect(row('grandchild').getAttribute('aria-level')).toBe('3');
        expect(row('parent').getAttribute('aria-setsize')).toBe('2');
        expect(row('other').getAttribute('aria-posinset')).toBe('2');
        expect(row('parent').getAttribute('aria-expanded')).toBe('true');
        expect(row('grandchild').hasAttribute('aria-expanded')).toBe(false);
    });

    it('emits selection without optimistically replacing the authoritative selection', async () => {
        await open(nested, ['parent']);
        row('other').click();
        expect(requested).toHaveBeenCalledExactlyOnceWith('other');
        expect(row('parent').getAttribute('aria-selected')).toBe('true');
        expect(row('other').getAttribute('aria-selected')).toBe('false');
        fixture.componentRef.setInput('selectedKeys', ['other']); await fixture.whenStable();
        expect(row('other').getAttribute('aria-selected')).toBe('true');
        expect(row('parent').getAttribute('aria-selected')).toBe('false');
    });

    it('keeps the first selected entity active while displaying the entire received multi-selection', async () => {
        await open(nested, ['other', 'parent']);
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('other').id);
        expect(row('other').classList.contains('selected')).toBe(true);
        expect(row('parent').classList.contains('selected')).toBe(true);
        expect(root.querySelector('.entity-details')?.textContent).toContain('props/robot.glb');
    });

    it('collapses descendants without emitting selection and can reopen with the keyboard', async () => {
        await open();
        row('parent').querySelector<HTMLElement>('.disclosure')!.click(); await fixture.whenStable();
        expect(keys()).toEqual(['parent', 'other']);
        expect(row('parent').getAttribute('aria-expanded')).toBe('false');
        expect(requested).not.toHaveBeenCalled();
        await keydown('ArrowRight');
        expect(keys()).toEqual(['parent', 'child', 'grandchild', 'other']);
        expect(row('parent').getAttribute('aria-expanded')).toBe('true');
    });

    it('supports arrows, Home, End, Enter and parent navigation from the focused tree', async () => {
        await open(); tree().focus();
        await keydown('ArrowDown'); expect(requested).toHaveBeenLastCalledWith('child');
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('child').id);
        await keydown('ArrowLeft'); expect(row('child').getAttribute('aria-expanded')).toBe('false');
        await keydown('ArrowLeft'); expect(requested).toHaveBeenLastCalledWith('parent');
        await keydown('End'); expect(requested).toHaveBeenLastCalledWith('other');
        await keydown('Home'); expect(requested).toHaveBeenLastCalledWith('parent');
        await keydown('Enter'); expect(requested).toHaveBeenLastCalledWith('parent');
    });

    it('reveals a descendant selected in the viewport through collapsed ancestors', async () => {
        await open();
        row('parent').querySelector<HTMLElement>('.disclosure')!.click(); await fixture.whenStable();
        expect(root.querySelector('[data-key="grandchild"]')).toBeNull();
        fixture.componentRef.setInput('selectedKeys', ['grandchild']); await fixture.whenStable();
        expect(row('parent').getAttribute('aria-expanded')).toBe('true');
        expect(row('child').getAttribute('aria-expanded')).toBe('true');
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('grandchild').id);
    });

    it('supports name typeahead without forwarding tool shortcuts to the viewport', async () => {
        await open([entity('a', null, 'Arch'), entity('b', null, 'Bridge'), entity('c', null, 'Crate')]);
        await keydown('c'); expect(requested).toHaveBeenLastCalledWith('c');
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('c').id);
    });

    it('bounds DOM rows for 12,000 entities and virtualizes scrolling and keyboard jumps', async () => {
        await open(Array.from({length: 12000}, (_, index) => entity(`item-${index}`)));
        expect(root.querySelectorAll('[role="treeitem"]').length).toBeLessThan(40);
        expect((root.querySelector('.virtual-space') as HTMLElement).style.height).toBe('336000px');
        tree().scrollTop = 6000 * 28; tree().dispatchEvent(new Event('scroll')); await fixture.whenStable();
        expect(row('item-6000')).toBeTruthy();
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('item-0').id);
        expect(root.querySelectorAll('[role="treeitem"]').length).toBeLessThan(40);
        await keydown('End');
        expect(row('item-11999')).toBeTruthy();
        expect(tree().getAttribute('aria-activedescendant')).toBe(row('item-11999').id);
        expect(requested).toHaveBeenLastCalledWith('item-11999');
        expect(root.querySelectorAll('[role="treeitem"]').length).toBeLessThan(40);
    });

    it('traverses a deep hierarchy iteratively and keeps visible labels within a bounded indent', async () => {
        await open(Array.from({length: 3000}, (_, index) => entity(`deep-${index}`, index ? `deep-${index - 1}` : null)));
        await keydown('End');
        expect(row('deep-2999').getAttribute('aria-level')).toBe('3000');
        expect(Number.parseFloat(row('deep-2999').style.paddingInlineStart)).toBeLessThanOrEqual(102);
        expect(root.querySelectorAll('[role="treeitem"]').length).toBeLessThan(40);
    });

    it('requests one reparent on an internal drop without rewriting the local scene', async () => {
        await open(); const data = await drag('other');
        const hover = dragEvent('dragover', data); row('parent').dispatchEvent(hover);
        expect(hover.defaultPrevented).toBe(true);
        row('parent').dispatchEvent(dragEvent('drop', data)); await fixture.whenStable();
        expect(reparented).toHaveBeenCalledExactlyOnceWith({key: 'other', parent: 'parent'});
        expect(row('other').getAttribute('aria-level')).toBe('1');
    });

    it('clears a parent when dropping onto the Scene root', async () => {
        await open(); const data = await drag('child');
        root.querySelector('.scene-root')!.dispatchEvent(dragEvent('drop', data)); await fixture.whenStable();
        expect(reparented).toHaveBeenCalledExactlyOnceWith({key: 'child', parent: null});
    });

    it('rejects self-parenting and parenting beneath descendants before emitting a command', async () => {
        await open(); let data = await drag('parent');
        row('parent').dispatchEvent(dragEvent('drop', data)); await fixture.whenStable();
        expect(reparented).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('itself or its descendants');
        data = await drag('parent');
        const hover = dragEvent('dragover', data); row('grandchild').dispatchEvent(hover);
        expect(hover.defaultPrevented).toBe(false);
        row('grandchild').dispatchEvent(dragEvent('drop', data)); await fixture.whenStable();
        expect(reparented).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('itself or its descendants');
    });

    it('ignores external or stale drag payloads', async () => {
        await open(); const data = transfer(); data.setData('application/x-game-engine-hierarchy-entity', 'other');
        row('parent').dispatchEvent(dragEvent('drop', data));
        expect(reparented).not.toHaveBeenCalled();
        const internal = await drag('other');
        fixture.componentRef.setInput('contextKey', 'different-project'); await fixture.whenStable();
        row('parent').dispatchEvent(dragEvent('drop', internal));
        expect(reparented).not.toHaveBeenCalled();
    });

    it('provides keyboard parent selection and an explicit Move to Root alternative', async () => {
        await open(nested, ['child']);
        action('Set Parent…').click(); await fixture.whenStable();
        await keydown('End'); await keydown('Enter');
        expect(reparented).toHaveBeenLastCalledWith({key: 'child', parent: 'other'});
        expect(requested).not.toHaveBeenCalled();
        action('Move to Root').click(); await fixture.whenStable();
        expect(reparented).toHaveBeenLastCalledWith({key: 'child', parent: null});
    });

    it('cancels keyboard parenting with Escape', async () => {
        await open(nested, ['child']); action('Set Parent…').click(); await fixture.whenStable();
        await keydown('Escape'); await keydown('End'); await keydown('Enter');
        expect(reparented).not.toHaveBeenCalled();
        expect(requested).toHaveBeenLastCalledWith('other');
    });

    it('blocks selection and parent changes during disabled authoring states', async () => {
        await open(nested, ['child']); fixture.componentRef.setInput('disabled', true); await fixture.whenStable();
        expect(row('child').disabled).toBe(true);
        row('other').click(); await keydown('End');
        const data = transfer(); const event = dragEvent('dragstart', data); row('child').dispatchEvent(event);
        expect(event.defaultPrevented).toBe(true);
        expect(requested).not.toHaveBeenCalled(); expect(reparented).not.toHaveBeenCalled();
        expect(tree().getAttribute('aria-disabled')).toBe('true');
    });

    it('shows backend parenting failures near the hierarchy and passes accessibility structure checks', async () => {
        await open(nested, ['child']);
        fixture.componentRef.setInput('actionError', 'The parent transform cannot preserve this pose.'); await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('cannot preserve this pose');
        const report = await axe.run(root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});

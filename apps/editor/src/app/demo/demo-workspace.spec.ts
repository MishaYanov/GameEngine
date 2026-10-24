import {TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';
import {vi} from 'vitest';
import {DemoWorkspace} from './demo-workspace';
import {EditorService} from '../core/editor/editor.service';

describe('Demo workspace', () => {
    it('loads a project, updates selection and inspector, and resets only its own fixture', async () => {
        await TestBed.configureTestingModule({
            imports: [DemoWorkspace],
            providers: [provideRouter([])],
        }).compileComponents();
        const fixture = TestBed.createComponent(DemoWorkspace);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const editor = fixture.debugElement.injector.get(EditorService);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(root.querySelector('#demo-name')).not.toBeNull();
        });
        expect(root.textContent).toContain('Editor Playground');
        const name = root.querySelector('#demo-name') as HTMLInputElement;
        name.value = 'Hero';
        name.dispatchEvent(new Event('input', {bubbles: true}));
        const submit = new Event('submit', {bubbles: true, cancelable: true});
        root.querySelector('form')!.dispatchEvent(submit);
        await fixture.whenStable();
        expect(submit.defaultPrevented).toBe(true);
        expect(editor.document()?.entities.find((entity) => entity.key === 'player')?.name).toBe(
            'Hero',
        );
        expect(editor.snapshot()?.can_undo).toBe(true);
        await editor.execute({type: 'select', keys: ['crate']});
        await fixture.whenStable();
        expect((root.querySelector('#demo-name') as HTMLInputElement).value).toBe('Supply crate');
        const reset = [...root.querySelectorAll('button')].find((b) =>
            b.textContent?.includes('Reset sample'),
        )!;
        reset.click();
        await fixture.whenStable();
        expect(editor.selection()).toEqual(['player']);
        expect(editor.dirty()).toBe(false);
    });
});

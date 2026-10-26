import {TestBed} from '@angular/core/testing';
import {EditorPane} from './pane';

describe('EditorPane', () => {
    it('supports a title alone or an optional decorative title icon', () => {
        const fixture = TestBed.createComponent(EditorPane);
        fixture.componentRef.setInput('title', 'Scene');
        fixture.detectChanges();
        const element: HTMLElement = fixture.nativeElement;
        expect(element.querySelector('h2')?.textContent).toBe('Scene');
        expect(element.querySelector('editor-icon')).toBeNull();
        fixture.componentRef.setInput('icon', 'cube');
        fixture.detectChanges();
        expect(element.querySelector('editor-icon')?.getAttribute('aria-hidden')).toBe('true');
        expect(element.querySelector('section')?.getAttribute('aria-label')).toBe('Scene');
        fixture.componentRef.setInput('visible', false);
        fixture.detectChanges();
        expect(element.querySelector('section')).toBeNull();
    });
});

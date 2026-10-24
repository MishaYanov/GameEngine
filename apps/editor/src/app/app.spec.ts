import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {invoke} from '@tauri-apps/api/core';
import {App} from './app';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));

describe('Project setup', () => {
    beforeEach(async () => {
        vi.mocked(invoke).mockReset();
        vi.mocked(invoke).mockResolvedValue(null);
        await TestBed.configureTestingModule({imports: [App]}).compileComponents();
    });

    it('renders the project setup fields', async () => {
        const fixture = TestBed.createComponent(App);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        expect(root.querySelector('h1')?.textContent).toContain('Create your project');
        expect(root.querySelectorAll('input').length).toBe(2);
    });

    it('validates inputs before starting a build', async () => {
        const fixture = TestBed.createComponent(App);
        await fixture.whenStable();
        vi.mocked(invoke).mockClear();
        fixture.nativeElement
            .querySelector('form')
            .dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        fixture.detectChanges();
        expect(invoke).not.toHaveBeenCalled();
        expect(fixture.nativeElement.textContent).toContain('Enter a valid project name');
    });

    it('submits values and shows the installed project', async () => {
        const fixture = TestBed.createComponent(App);
        await fixture.whenStable();
        vi.mocked(invoke).mockResolvedValue({
            path: 'D:\\Projects\\Demo',
            launched: true,
            launchError: null,
        });
        const root = fixture.nativeElement as HTMLElement;
        for (const [selector, value] of [
            ['#project-name', 'Demo'],
            ['#destination', 'D:\\Projects'],
        ]) {
            const input = root.querySelector(selector) as HTMLInputElement;
            input.value = value;
            input.dispatchEvent(new Event('input'));
        }
        root
            .querySelector('form')!
            .dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        await fixture.whenStable();
        fixture.detectChanges();
        expect(invoke).toHaveBeenCalledWith('create_project', {
            name: 'Demo',
            destination: 'D:\\Projects',
        });
        expect(root.textContent).toContain('Project installed');
    });
});

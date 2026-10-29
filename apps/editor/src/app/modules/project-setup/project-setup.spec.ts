import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {invoke, isTauri} from '@tauri-apps/api/core';
import axe from 'axe-core';
import {ProjectSetup} from './project-setup';

vi.mock('@tauri-apps/api/core', () => ({isTauri: vi.fn(() => true), invoke: vi.fn()}));

describe('Project setup', () => {
    beforeEach(async () => {
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset();
        vi.mocked(invoke).mockResolvedValue(null);
        await TestBed.configureTestingModule({imports: [ProjectSetup]}).compileComponents();
    });

    it('renders the project setup fields', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        expect(root.querySelector('h1')?.textContent).toContain('Create your project');
        expect(root.querySelectorAll('input').length).toBe(2);
    });

    it('validates inputs before starting a build', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        vi.mocked(invoke).mockClear();
        fixture.nativeElement
            .querySelector('form')
            .dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        fixture.detectChanges();
        expect(invoke).not.toHaveBeenCalled();
        expect(fixture.nativeElement.textContent).toContain('Enter a valid project name');
    });

    it('chooses a destination and uses it when creating the project', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const destination = 'D:\\Game Projects\\משחקים';
        vi.mocked(invoke).mockClear();
        vi.mocked(invoke).mockResolvedValueOnce(destination);
        root.querySelector<HTMLButtonElement>('.browse')!.click();
        await fixture.whenStable();
        expect(invoke).toHaveBeenCalledExactlyOnceWith('choose_project_destination', {
            initialDirectory: null,
        });
        expect(root.querySelector<HTMLInputElement>('#destination')!.value).toBe(destination);
        const name = root.querySelector<HTMLInputElement>('#project-name')!;
        name.value = 'Demo';
        name.dispatchEvent(new Event('input'));
        vi.mocked(invoke).mockResolvedValueOnce({path: `${destination}\\Demo`, launched: true, launchError: null});
        root.querySelector('form')!.dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(invoke).toHaveBeenLastCalledWith('create_project', {name: 'Demo', destination});
    });

    it('starts browsing from the current destination and preserves it when cancelled', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const input = root.querySelector<HTMLInputElement>('#destination')!;
        input.value = 'D:\\Existing Projects';
        input.dispatchEvent(new Event('input'));
        vi.mocked(invoke).mockClear();
        vi.mocked(invoke).mockResolvedValueOnce(null);
        root.querySelector<HTMLButtonElement>('.browse')!.click();
        await fixture.whenStable();
        expect(invoke).toHaveBeenCalledExactlyOnceWith('choose_project_destination', {
            initialDirectory: input.value,
        });
        expect(input.value).toBe('D:\\Existing Projects');
    });

    it('prevents duplicate pickers and builds while a folder is being chosen', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        let finish!: (value: string | null) => void;
        vi.mocked(invoke).mockClear();
        vi.mocked(invoke).mockReturnValueOnce(
            new Promise<string | null>((resolve) => {
                finish = resolve;
            }),
        );
        const browse = root.querySelector<HTMLButtonElement>('.browse')!;
        browse.click();
        fixture.detectChanges();
        expect(browse.disabled).toBe(true);
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
        browse.click();
        root
            .querySelector('form')!
            .dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        expect(invoke).toHaveBeenCalledTimes(1);
        finish(null);
        await fixture.whenStable();
        expect(browse.disabled).toBe(false);
    });

    it('shows picker failures and allows trying again', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        vi.mocked(invoke).mockRejectedValueOnce('Folder chooser unavailable');
        const browse = root.querySelector<HTMLButtonElement>('.browse')!;
        browse.click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')!.textContent).toContain(
            'Folder chooser unavailable',
        );
        expect(browse.disabled).toBe(false);
        vi.mocked(invoke).mockResolvedValueOnce('D:\\Projects');
        browse.click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')).toBeNull();
        expect(root.querySelector<HTMLInputElement>('#destination')!.value).toBe('D:\\Projects');
    });

    it('keeps native folder browsing unavailable in browser preview', async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const browse = fixture.nativeElement.querySelector('.browse') as HTMLButtonElement;
        expect(browse.disabled).toBe(true);
        browse.click();
        expect(invoke).not.toHaveBeenCalled();
    });

    it('has no structural accessibility violations', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        // Color contrast requires a rendering browser rather than jsdom.
        const report = await axe.run(fixture.nativeElement, {
            rules: {'color-contrast': {enabled: false}},
        });
        expect(report.violations.map((violation) => violation.id)).toEqual([]);
    });

    it('submits values and shows the installed project', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
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

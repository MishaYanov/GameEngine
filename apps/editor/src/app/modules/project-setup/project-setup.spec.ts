import {TestBed} from '@angular/core/testing';
import {signal} from '@angular/core';
import {ActivatedRoute, convertToParamMap, provideRouter, Router} from '@angular/router';
import {vi} from 'vitest';
import {invoke, isTauri} from '@tauri-apps/api/core';
import axe from 'axe-core';
import {ProjectSetup} from './project-setup';
import {EditorService} from '../../core/editor/editor.service';
import type {EditorSnapshot} from '../../core/editor/editor.models';

vi.mock('@tauri-apps/api/core', () => ({isTauri: vi.fn(() => true), invoke: vi.fn()}));

describe('Project setup', () => {
    type SessionState = Pick<EditorSnapshot, 'revision' | 'mode' | 'transaction' | 'dirty' | 'project_dirty' | 'scenes'>;
    const current = signal<SessionState | null>(null);
    const editorBusy = signal(false);
    const workspaceGeneration = signal(7);
    const chooseManifest = vi.fn<() => Promise<string | null>>();
    const confirmDiscard = vi.fn<() => Promise<boolean>>();
    const openProject = vi.fn();
    const created = {path: 'D:\\Projects\\Demo', manifestPath: 'D:\\Projects\\Demo\\project.json'};

    beforeEach(async () => {
        current.set(null);
        editorBusy.set(false);
        workspaceGeneration.set(7);
        chooseManifest.mockReset().mockResolvedValue(null);
        confirmDiscard.mockReset().mockResolvedValue(true);
        openProject.mockReset().mockResolvedValue(null);
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset();
        vi.mocked(invoke).mockResolvedValue(null);
        await TestBed.configureTestingModule({
            imports: [ProjectSetup],
            providers: [provideRouter([]), {
                provide: EditorService,
                useValue: {
                    snapshot: current, busy: editorBusy, workspaceGeneration,
                    chooseProjectManifest: chooseManifest, confirmProjectDiscard: confirmDiscard, openProject,
                },
            }]
        }).compileComponents();
        vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    });

    function fill(root: HTMLElement): void {
        for (const [selector, value] of [['#project-name', 'Demo'], ['#destination', 'D:\\Projects']]) {
            const input = root.querySelector<HTMLInputElement>(selector)!;
            input.value = value;
            input.dispatchEvent(new Event('input'));
        }
    }

    function submit(root: HTMLElement): void {
        root.querySelector('form')!.dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
    }

    function session(patch: Partial<SessionState> = {}): SessionState {
        return {revision: 4, mode: 'edit', transaction: null, dirty: false, project_dirty: false, scenes: [], ...patch};
    }

    it('renders the project setup fields', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        expect(root.querySelector('h1')?.textContent).toContain('Open or create a project');
        expect(root.querySelector('h2')?.textContent).toContain('Create a project');
        expect(root.querySelector('.open-existing')?.textContent).toContain('Open Existing Project');
        expect(root.querySelectorAll('input').length).toBe(2);
        expect(root.querySelector<HTMLSelectElement>('#project-template')?.value).toBe('blank');
    });

    it('opens a selected manifest from startup without creating a project', async () => {
        chooseManifest.mockResolvedValueOnce(created.manifestPath);
        let finishOpen!: () => void;
        openProject.mockImplementationOnce(() => new Promise<void>(resolve => finishOpen = resolve));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        root.querySelector<HTMLButtonElement>('.open-existing')!.click();
        await vi.waitFor(() => expect(openProject).toHaveBeenCalledOnce());

        expect(chooseManifest).toHaveBeenCalledOnce();
        expect(invoke).not.toHaveBeenCalled();
        expect(openProject).toHaveBeenCalledWith(created.manifestPath, {
            expectedGeneration: 7, expectedRevision: null, discardUnsaved: false,
        });
        expect(TestBed.inject(Router).navigateByUrl).not.toHaveBeenCalled();
        finishOpen();
        await fixture.whenStable();
        expect(TestBed.inject(Router).navigateByUrl).toHaveBeenCalledWith('/editor', {replaceUrl: true});
    });

    it('keeps focus and creation fields when the existing project picker is canceled', async () => {
        let finishPicking!: (manifest: string | null) => void;
        chooseManifest.mockImplementationOnce(() => new Promise(resolve => finishPicking = resolve));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        const open = root.querySelector<HTMLButtonElement>('.open-existing')!;
        open.focus();
        open.click();
        fixture.detectChanges();
        expect(open.getAttribute('aria-disabled')).toBe('true');
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
        open.click();
        submit(root);
        expect(chooseManifest).toHaveBeenCalledOnce();
        expect(invoke).not.toHaveBeenCalled();

        finishPicking(null);
        await fixture.whenStable();
        expect(open.getAttribute('aria-disabled')).toBe('false');
        expect(root.ownerDocument.activeElement).toBe(open);
        expect(root.querySelector<HTMLInputElement>('#project-name')!.value).toBe('Demo');
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false);
        expect(openProject).not.toHaveBeenCalled();
        expect(TestBed.inject(Router).navigateByUrl).not.toHaveBeenCalled();
    });

    it('rejects a picker response from a previous workspace', async () => {
        let finishPicking!: (manifest: string) => void;
        chooseManifest.mockImplementationOnce(() => new Promise(resolve => finishPicking = resolve));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        root.querySelector<HTMLButtonElement>('.open-existing')!.click();
        workspaceGeneration.set(8);
        finishPicking(created.manifestPath);
        await fixture.whenStable();

        expect(root.querySelector('[role="alert"]')?.textContent).toContain('The current workspace changed');
        expect(openProject).not.toHaveBeenCalled();
    });

    it('honors the discard decision before loading an existing project', async () => {
        current.set(session({project_dirty: true}));
        chooseManifest.mockResolvedValueOnce(created.manifestPath);
        confirmDiscard.mockResolvedValueOnce(false);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        root.querySelector<HTMLButtonElement>('.open-existing')!.click();
        await fixture.whenStable();

        expect(confirmDiscard).toHaveBeenCalledOnce();
        expect(openProject).not.toHaveBeenCalled();
        expect(root.querySelector('[role="status"]')?.textContent).toContain('Opening canceled');
    });

    it('preselects Cinder Oath from Open Sample and asks the host for an editable copy', async () => {
        const snapshot = TestBed.inject(ActivatedRoute).snapshot;
        vi.spyOn(snapshot, 'queryParamMap', 'get').mockReturnValue(convertToParamMap({
            from: 'editor', template: 'cinder_oath',
        }));
        vi.mocked(invoke).mockResolvedValue(created);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        expect(root.querySelector<HTMLSelectElement>('#project-template')?.value).toBe('cinder_oath');
        expect(root.querySelector<HTMLInputElement>('#project-name')?.value).toBe('Cinder Oath');
        fill(root);
        submit(root);
        await fixture.whenStable();

        expect(invoke).toHaveBeenCalledExactlyOnceWith('create_project', {
            name: 'Demo', destination: 'D:\\Projects', template: 'cinder_oath',
        });
        expect(openProject).toHaveBeenCalledWith(created.manifestPath, {
            expectedGeneration: 7, expectedRevision: null, discardUnsaved: false,
        });
    });

    it('validates inputs before creating a project', async () => {
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
        vi.mocked(invoke).mockResolvedValueOnce({path: `${destination}\\Demo`, manifestPath: `${destination}\\Demo\\project.json`});
        root.querySelector('form')!.dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(invoke).toHaveBeenLastCalledWith('create_project', {name: 'Demo', destination, template: 'blank'});
        expect(openProject).toHaveBeenCalledWith(`${destination}\\Demo\\project.json`, {
            expectedGeneration: 7, expectedRevision: null, discardUnsaved: false,
        });
        expect(TestBed.inject(Router).navigateByUrl).toHaveBeenCalledWith('/editor', {replaceUrl: true});
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

    it('prevents duplicate pickers and creation while a folder is being chosen', async () => {
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
        expect(fixture.nativeElement.querySelector('button[type="submit"]').disabled).toBe(true);
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

    it('keeps native folder browsing and project opening unavailable in browser preview', async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const browse = fixture.nativeElement.querySelector('.browse') as HTMLButtonElement;
        expect(browse.disabled).toBe(true);
        browse.click();
        const open = fixture.nativeElement.querySelector('.open-existing') as HTMLButtonElement;
        expect(open.disabled).toBe(true);
        open.click();
        expect(chooseManifest).not.toHaveBeenCalled();
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

    it('submits values and shows the created authoring project', async () => {
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        vi.mocked(invoke).mockResolvedValue(created);
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
            template: 'blank',
        });
        expect(root.textContent).toContain('Project created');
        expect(root.textContent).toContain(created.manifestPath);
        expect(root.textContent).not.toContain('Cargo');
        expect(root.textContent).not.toContain('triangles');
    });

    it('retains the created project after discard cancellation and opens it on retry without creating twice', async () => {
        current.set(session({dirty: true}));
        confirmDiscard.mockResolvedValueOnce(false);
        vi.mocked(invoke).mockResolvedValue(created);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        await fixture.whenStable();
        expect(confirmDiscard).toHaveBeenCalledOnce();
        expect(openProject).not.toHaveBeenCalled();
        expect(root.textContent).toContain(created.path);
        expect(root.textContent).toContain('Your current project remains open');
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
        expect(root.querySelector<HTMLInputElement>('#project-name')!.disabled).toBe(true);
        root.querySelector<HTMLButtonElement>('.open-created')!.click();
        await fixture.whenStable();
        expect(openProject).toHaveBeenCalledExactlyOnceWith(created.manifestPath, {
            expectedGeneration: 7, expectedRevision: 4, discardUnsaved: true,
        });
        expect(invoke).toHaveBeenCalledExactlyOnceWith('create_project', {
            name: 'Demo',
            destination: 'D:\\Projects',
            template: 'blank'
        });
    });

    it('captures the reviewed revision before waiting for discard confirmation', async () => {
        current.set(session({dirty: true}));
        let answer!: (confirmed: boolean) => void;
        confirmDiscard.mockImplementation(() => new Promise<boolean>(resolve => answer = resolve));
        vi.mocked(invoke).mockResolvedValue(created);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        await vi.waitFor(() => expect(confirmDiscard).toHaveBeenCalledOnce());
        current.set(session({revision: 9, dirty: true}));
        answer(true);
        await fixture.whenStable();
        expect(openProject).toHaveBeenCalledWith(created.manifestPath, {
            expectedGeneration: 7, expectedRevision: 4, discardUnsaved: true,
        });
    });

    it('keeps the created path after open failure and retries opening instead of copying assets again', async () => {
        vi.mocked(invoke).mockResolvedValue(created);
        openProject.mockRejectedValueOnce(new Error('Cannot open this project yet.'));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')!.textContent).toContain('Cannot open this project yet');
        expect(root.textContent).toContain(created.path);
        expect(TestBed.inject(Router).navigateByUrl).not.toHaveBeenCalled();
        root.querySelector<HTMLButtonElement>('.open-created')!.click();
        await fixture.whenStable();
        expect(openProject).toHaveBeenCalledTimes(2);
        expect(invoke).toHaveBeenCalledOnce();
        expect(TestBed.inject(Router).navigateByUrl).toHaveBeenCalledWith('/editor', {replaceUrl: true});
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('restores the form after creation fails so the user can correct the destination', async () => {
        vi.mocked(invoke).mockRejectedValueOnce(new Error('The destination already exists.'));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')!.textContent).toContain('destination already exists');
        expect(root.querySelector<HTMLInputElement>('#destination')!.disabled).toBe(false);
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false);
        expect(root.querySelector('.open-created')).toBeNull();
        expect(openProject).not.toHaveBeenCalled();
    });

    it.each([
        {state: session({mode: 'play'}), busy: false, message: 'Stop Play'},
        {state: session({transaction: 'Move entity'}), busy: false, message: 'Finish the current edit'},
        {state: session(), busy: true, message: 'Wait for the current editor operation'},
    ])('preserves the current session when opening is blocked: $message', async ({state, busy, message}) => {
        current.set(state);
        editorBusy.set(busy);
        vi.mocked(invoke).mockResolvedValue(created);
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        await fixture.whenStable();
        expect(openProject).not.toHaveBeenCalled();
        expect(confirmDiscard).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')!.textContent).toContain(message);
        expect(root.textContent).toContain(created.path);
        expect(root.querySelector<HTMLButtonElement>('.open-created')!.disabled).toBe(false);
    });

    it('keeps Create project busy until creation finishes and navigates only after the editor accepts it', async () => {
        let finishCreate!: (value: typeof created) => void;
        let finishOpen!: () => void;
        vi.mocked(invoke).mockImplementationOnce(() => new Promise<typeof created>(resolve => finishCreate = resolve));
        openProject.mockImplementationOnce(() => new Promise<void>(resolve => finishOpen = resolve));
        const fixture = TestBed.createComponent(ProjectSetup);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        fill(root);
        submit(root);
        fixture.detectChanges();
        expect(root.querySelector('button[type="submit"]')!.textContent).toContain('Creating project');
        expect(root.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true);
        finishCreate(created);
        await vi.waitFor(() => expect(openProject).toHaveBeenCalledOnce());
        expect(TestBed.inject(Router).navigateByUrl).not.toHaveBeenCalled();
        finishOpen();
        await fixture.whenStable();
        expect(TestBed.inject(Router).navigateByUrl).toHaveBeenCalledWith('/editor', {replaceUrl: true});
    });
});

import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';
import {invoke, isTauri} from '@tauri-apps/api/core';
import axe from 'axe-core';
import {vi} from 'vitest';

import {Editor} from './editor';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => false)}));

describe('Editor', () => {
    let component: Editor;
    let fixture: ComponentFixture<Editor>;

    beforeEach(async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        await TestBed.configureTestingModule({
            imports: [Editor],
            providers: [provideRouter([])],
        }).compileComponents();

        fixture = TestBed.createComponent(Editor);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('has no structural accessibility violations', async () => {
        // Color contrast needs a rendering browser and is checked in the visual audit.
        const results = await axe.run(fixture.nativeElement, {
            rules: {'color-contrast': {enabled: false}},
        });
        expect(
            results.violations.map((violation) => ({
                id: violation.id,
                nodes: violation.nodes.map((node) => node.target),
            })),
        ).toEqual([]);
    });
});

it('waits for the retained workspace check before offering to open a project', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    let reportEmpty!: (cause: unknown) => void;
    const restore = new Promise<never>((_, reject) => {
        reportEmpty = reject;
    });
    vi.mocked(invoke).mockReset().mockImplementation(async operation => {
        if (operation === 'editor_snapshot') return restore;
        if (operation === 'editor_capabilities') return {
            protocol_version: 2, scene_schema_version: 3, embedded_viewport_supported: true,
        };
        return null;
    });
    await TestBed.configureTestingModule({imports: [Editor], providers: [provideRouter([])]}).compileComponents();
    const fixture = TestBed.createComponent(Editor);
    fixture.detectChanges();
    const root: HTMLElement = fixture.nativeElement;
    const button = root.querySelector<HTMLButtonElement>('.empty-open')!;
    expect(button.disabled).toBe(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('editor_snapshot', undefined));
    reportEmpty('Open a scene before using the editor');
    await vi.waitFor(() => {
        fixture.detectChanges();
        expect(button.disabled).toBe(false);
    });
    expect(root.querySelector('.open-error')).toBeNull();
    button.click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('choose_project_manifest', undefined));
});

import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {EditorService} from '../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {WORKSPACE_LAYOUT_SCOPE} from '../../../shared/services/layout-preferences-storage';
import {WORKSPACE_LAYOUT_SCOPE_PROVIDER} from './workspace-layout-preferences';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));

describe('Project layout identity', () => {
    const viewport = signal<{workspace_generation: number} | null>(null);
    const snapshot = signal<object | null>(null);
    const workspaceGeneration = signal(0);
    const identityA = 'a'.repeat(64);
    const identityB = 'b'.repeat(64);

    beforeEach(() => {
        viewport.set(null);
        snapshot.set(null);
        workspaceGeneration.set(0);
        vi.mocked(invoke).mockReset();
        TestBed.configureTestingModule({providers: [
            WORKSPACE_LAYOUT_SCOPE_PROVIDER,
            {provide: EditorService, useValue: {nativeAvailable: true, snapshot, workspaceGeneration}},
            {provide: ViewportEditorService, useValue: {state: viewport}},
        ]});
    });

    it('keeps appearance global before Open and ignores a retired project’s late reply', async () => {
        let finishFirst!: (scope: string) => void;
        vi.mocked(invoke)
            .mockImplementationOnce(() => new Promise(resolve => finishFirst = resolve))
            .mockResolvedValueOnce(identityB);
        const scope = TestBed.inject(WORKSPACE_LAYOUT_SCOPE);
        expect(scope()).toBeNull();
        snapshot.set({});
        viewport.set({workspace_generation: 4});
        workspaceGeneration.set(1);
        TestBed.tick();
        expect(scope()).toBeUndefined();
        expect(invoke).toHaveBeenCalledWith('editor_layout_preferences_project_scope', {workspaceGeneration: 4});
        workspaceGeneration.set(2);
        viewport.set({workspace_generation: 5});
        TestBed.tick();
        await vi.waitFor(() => expect(scope()).toBe(identityB));
        finishFirst(identityA);
        await Promise.resolve();
        expect(scope()).toBe(identityB);
    });

    it('falls back to read-only global defaults when no valid project identity can be resolved', async () => {
        vi.mocked(invoke).mockResolvedValue({unexpected: 'data'});
        const scope = TestBed.inject(WORKSPACE_LAYOUT_SCOPE);
        snapshot.set({});
        expect(scope()).toBe(false);
        viewport.set({workspace_generation: 4});
        TestBed.tick();
        await vi.waitFor(() => expect(scope()).toBe(false));
    });
});

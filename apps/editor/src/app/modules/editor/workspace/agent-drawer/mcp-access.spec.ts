import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import axe from 'axe-core';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {McpAccess} from './mcp-access';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

describe('McpAccess', () => {
    const state = signal<{workspace_generation: number} | null>({workspace_generation: 21});
    const connection = {
        workspace_generation: 21,
        worker_path: 'authoring-agent-worker.exe',
        port: 52319,
        token: 'fixture-token',
        expires_in_seconds: 600,
    };

    beforeEach(() => {
        vi.mocked(invoke).mockReset();
        state.set({workspace_generation: 21});
        TestBed.configureTestingModule({
            imports: [McpAccess],
            providers: [
                {provide: EditorService, useValue: {nativeAvailable: true}},
                {provide: ViewportEditorService, useValue: {state}},
            ],
        });
    });

    it('keeps connection settings private until native consent and removes them on revoke', async () => {
        const fixture = TestBed.createComponent(McpAccess);
        fixture.detectChanges();
        const view: HTMLElement = fixture.nativeElement;
        expect(view.querySelector('textarea')).toBeNull();
        expect(invoke).not.toHaveBeenCalled();

        view.querySelector('details')!.open = true;
        const button = view.querySelector('button')!;
        button.focus();
        expect((await axe.run(view, {rules: {'color-contrast': {enabled: false}}})).violations).toEqual([]);
        vi.mocked(invoke).mockResolvedValueOnce(connection).mockResolvedValueOnce(true);
        button.click();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(invoke).toHaveBeenCalledWith('editor_mcp_grant', {workspaceGeneration: 21});
        expect(view.querySelector('textarea')?.value).toContain('"GAME_ENGINE_MCP_TOKEN": "fixture-token"');
        expect(view.querySelector('button')).toBe(button);
        expect(document.activeElement).toBe(button);
        expect(view.querySelector('[aria-live]')?.textContent).toBe('MCP read access is active.');
        expect((await axe.run(view, {rules: {'color-contrast': {enabled: false}}})).violations).toEqual([]);

        button.click();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(invoke).toHaveBeenCalledWith('editor_mcp_revoke', {workspaceGeneration: 21, token: connection.token});
        expect(view.querySelector('textarea')).toBeNull();
        expect(view.querySelector('[aria-live]')?.textContent).toBe('MCP read access revoked.');
    });

    it('revokes an approval that arrives after the project switched', async () => {
        const fixture = TestBed.createComponent(McpAccess);
        fixture.detectChanges();
        const view: HTMLElement = fixture.nativeElement;
        let approve: (result: unknown) => void = () => {};
        vi.mocked(invoke).mockReturnValueOnce(new Promise(resolve => { approve = resolve; })).mockResolvedValueOnce(true);

        view.querySelector('button')?.click();
        state.set({workspace_generation: 22});
        approve(connection);
        await fixture.whenStable();
        fixture.detectChanges();

        expect(invoke).toHaveBeenCalledWith('editor_mcp_revoke', {workspaceGeneration: 21, token: connection.token});
        expect(view.querySelector('textarea')).toBeNull();
    });

    it.each(['grant', 'revoke'])('drops a late %s failure after the project switched', async (operation) => {
        const fixture = TestBed.createComponent(McpAccess);
        fixture.detectChanges();
        const view: HTMLElement = fixture.nativeElement;
        const button = view.querySelector('button')!;
        if (operation === 'revoke') {
            vi.mocked(invoke).mockResolvedValueOnce(connection);
            button.click();
            await fixture.whenStable();
            fixture.detectChanges();
        }
        let rejectRequest: (cause: unknown) => void = () => {};
        vi.mocked(invoke).mockReturnValueOnce(new Promise((_, reject) => { rejectRequest = reject; }));
        vi.mocked(invoke).mockResolvedValueOnce(true);
        button.click();
        state.set({workspace_generation: 22});
        rejectRequest(new Error('Old workspace request failed'));
        await fixture.whenStable();
        fixture.detectChanges();

        expect(view.querySelector('[role="alert"]')).toBeNull();
        expect(view.querySelector('[aria-live]')?.textContent).toBe('');
        expect(view.querySelector('textarea')).toBeNull();
    });
});

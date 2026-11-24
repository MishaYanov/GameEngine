import {Component, computed, DestroyRef, effect, inject, signal} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';

interface EditorMcpConnection {
    readonly workspace_generation: number;
    readonly worker_path: string | null;
    readonly port: number;
    readonly token: string;
    readonly expires_in_seconds: number;
}

@Component({
    selector: 'editor-mcp-access',
    templateUrl: './mcp-access.html',
    styles: [`
        :host { display: block; margin-block-start: var(--space-3); }
        details { display: block; }
        summary { cursor: pointer; }
        label { display: block; }
        textarea {
            box-sizing: border-box;
            inline-size: 100%;
            background: var(--color-control);
            color: var(--color-text);
            resize: vertical;
        }
    `],
})
export class McpAccess {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly destroy = inject(DestroyRef);
    private readonly connection = signal<EditorMcpConnection | null>(null);
    private expiry: ReturnType<typeof setTimeout> | undefined;
    protected readonly enabled = computed(() => this.editor.nativeAvailable && this.generation() != null);
    protected readonly pending = signal(false);
    protected readonly failure = signal<string | null>(null);
    protected readonly status = signal('');
    protected readonly configuration = computed(() => {
        const connection = this.connection();
        return connection && JSON.stringify({
            command: connection.worker_path ?? 'authoring-agent-worker',
            args: ['--mcp'],
            env: {
                GAME_ENGINE_MCP_PORT: String(connection.port),
                GAME_ENGINE_MCP_TOKEN: connection.token,
            },
        }, null, 2);
    });
    private readonly generation = computed(() => this.viewport.state()?.workspace_generation);
    private observedGeneration = this.generation();

    constructor() {
        effect(() => {
            const generation = this.generation();
            const active = this.connection();
            if (generation !== this.observedGeneration) {
                this.observedGeneration = generation;
                this.status.set('');
                this.failure.set(null);
            }
            if (active && generation !== active.workspace_generation) {
                this.forget();
                void this.revoke(active).catch(() => {});
            }
        });
        this.destroy.onDestroy(() => {
            const active = this.connection();
            this.forget();
            if (active) void this.revoke(active).catch(() => {});
        });
    }

    protected toggle(): Promise<void> {
        return this.connection() ? this.stop() : this.allow();
    }

    private async allow(): Promise<void> {
        const generation = this.generation();
        if (generation == null || this.pending() || this.connection()) return;
        this.pending.set(true);
        this.failure.set(null);
        try {
            const connection = await invoke<EditorMcpConnection | null>('editor_mcp_grant', {
                workspaceGeneration: generation,
            });
            if (!connection) return;
            if (!this.current(connection.workspace_generation)) {
                await this.revoke(connection);
                return;
            }
            this.connection.set(connection);
            this.status.set('MCP read access is active.');
            this.expiry = setTimeout(() => {
                this.forget();
                this.status.set('MCP read access expired.');
            }, connection.expires_in_seconds * 1000);
        } catch {
            if (this.current(generation)) this.failure.set('External MCP access could not be started.');
        } finally {
            if (!this.destroy.destroyed) this.pending.set(false);
        }
    }

    private async stop(): Promise<void> {
        const connection = this.connection();
        if (!connection || this.pending()) return;
        this.pending.set(true);
        this.failure.set(null);
        try {
            await this.revoke(connection);
            if (this.current(connection.workspace_generation)) {
                this.forget();
                this.status.set('MCP read access revoked.');
            }
        } catch {
            if (this.current(connection.workspace_generation)) {
                this.failure.set('MCP access could not be revoked. Try again or close the project.');
            }
        } finally {
            if (!this.destroy.destroyed) this.pending.set(false);
        }
    }

    private current(generation: number): boolean {
        return !this.destroy.destroyed && this.generation() === generation;
    }

    private revoke(connection: EditorMcpConnection): Promise<boolean> {
        return invoke<boolean>('editor_mcp_revoke', {
            workspaceGeneration: connection.workspace_generation,
            token: connection.token,
        });
    }

    private forget(): void {
        clearTimeout(this.expiry);
        this.expiry = undefined;
        this.connection.set(null);
    }
}

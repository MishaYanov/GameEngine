import {Component, computed, inject, signal} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {AgentAnswerActions, type AgentAnswerAction} from './agent-answer-actions';
import {AgentDrawerService} from './agent-drawer.service';
import {AgentSelectionContextService} from './agent-selection-context';

@Component({
    selector: 'editor-agent-conversation',
    templateUrl: './agent-conversation.html',
    styleUrl: './agent-conversation.scss',
})
export class EditorAgentConversation {
    protected readonly session = inject(AgentDrawerService);
    protected readonly navigation = inject(AgentAnswerActions);
    protected readonly retry = signal<AgentAnswerAction | null>(null);
    private readonly selection = inject(AgentSelectionContextService);
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    protected readonly question = signal('');
    protected readonly componentId = signal('');
    protected readonly componentIds = this.selection.componentIds;
    protected readonly nativeReady = computed(() => this.editor.nativeAvailable &&
        this.editor.snapshot()?.mode === 'edit' && this.viewport.state()?.workspace_generation != null);
    protected readonly canSubmit = computed(() => this.nativeReady() && !this.session.running() &&
        this.question().trim().length > 0 && (!this.selection.pinned() || !!this.selection.context()));

    protected async open(action: AgentAnswerAction, refresh = false): Promise<void> {
        const opened = await this.navigation.run(action, refresh);
        this.retry.set(!opened && this.navigation.notice() ? action : null);
    }

    protected async ask(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (!this.canSubmit()) return;
        const question = this.question();
        if (await this.session.start(question, this.selection.capture(
            this.componentIds().includes(this.componentId()) ? this.componentId() : undefined,
        )) && this.question() === question) {
            this.question.set('');
        }
    }
}

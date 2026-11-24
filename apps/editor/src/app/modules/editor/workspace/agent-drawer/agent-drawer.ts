import {Component, ElementRef, inject, viewChild} from '@angular/core';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {EditorAgentConversation} from './agent-conversation';
import {McpAccess} from './mcp-access';
import {AgentSelectionContextService} from './agent-selection-context';

@Component({
    selector: 'editor-agent-drawer',
    imports: [EditorPane, EditorAgentConversation, McpAccess],
    templateUrl: './agent-drawer.html',
    styleUrl: './agent-drawer.scss',
})
export class AgentDrawer {
    protected readonly selection = inject(AgentSelectionContextService);
    private readonly content = viewChild<ElementRef<HTMLElement>>('content');

    focus(): void {
        this.content()?.nativeElement.focus({preventScroll: true});
    }
}

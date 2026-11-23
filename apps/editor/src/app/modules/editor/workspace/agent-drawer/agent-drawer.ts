import {Component, ElementRef, viewChild} from '@angular/core';
import {EditorPane} from '../../../../shared/components/pane/pane';

@Component({
    selector: 'editor-agent-drawer',
    imports: [EditorPane],
    templateUrl: './agent-drawer.html',
    styleUrl: './agent-drawer.scss',
})
export class AgentDrawer {
    private readonly content = viewChild<ElementRef<HTMLElement>>('content');

    focus(): void {
        this.content()?.nativeElement.focus({preventScroll: true});
    }
}

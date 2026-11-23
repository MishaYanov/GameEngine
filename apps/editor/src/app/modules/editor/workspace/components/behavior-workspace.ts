import {Component, effect, inject, viewChild} from '@angular/core';
import {ComponentEditor} from './component-editor';
import {ComponentWorkspaceSession} from './component-workspace-session';

@Component({
    selector: 'editor-behavior-workspace',
    imports: [ComponentEditor],
    template: `
        <section aria-label="Behavior" class="behavior-workspace">
            <header>
                <h2>{{ session.description() }}</h2>
                @if (session.target(); as target) {
                    @if (target.record.data.kind === 'script') {
                        @let settings = target.record.data.settings;
                        @let script = session.scriptType(settings.script_id);
                        @if (!script) {
                            <p role="status">{{ settings.script_id }} is unavailable in this project.</p>
                        } @else if (script.config_version !== settings.config_version) {
                            <p role="status">{{ script.label }} requires a configuration reset before applying changes.</p>
                        }
                        @if (script?.receive_local_actions) {
                            <p>Receives local input from the project action profile.</p>
                        }
                    }
                } @else {
                    <p role="status">Select the original object or reopen Behavior from another component.</p>
                }
            </header>
            <editor-component-editor [focusedComponent]="session.record()" [ownerKey]="session.ownerKey()"
                [editable]="session.editable()" [pending]="session.pending()" [error]="session.error()"
                [acknowledged]="session.acknowledged()" [contextKey]="session.context()"
                [models]="session.models()" [entities]="session.objectTargets()" [gameActions]="session.gameActions()"
                [scriptTypes]="session.scriptTypes()" [scriptSlots]="session.scriptSlots()"
                (actionRequested)="session.apply($event)"/>
        </section>
    `,
    styles: `
        :host { display: block; height: 100%; overflow: auto; }
        .behavior-workspace { max-width: 44rem; margin-inline: auto; padding: 1rem; }
        h2 { margin-block: 0 .75rem; }
    `,
})
export class BehaviorWorkspace {
    protected readonly session = inject(ComponentWorkspaceSession);
    private readonly editor = viewChild(ComponentEditor);

    constructor() {
        effect(onCleanup => {
            const form = this.editor();
            onCleanup(this.session.connectForm(form));
        });
    }
}

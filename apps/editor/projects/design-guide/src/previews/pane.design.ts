import {Component, inject} from '@angular/core';
import {SubjectStore} from '../../../../src/app/core/subjects/subject.store';
import {EditorPane} from '../../../../src/app/shared/components/pane/pane';
import {Drawer} from '../../../../src/app/shared/components/drawer/drawer';
import {SubjectCard} from '../../../../src/app/shared/components/subject/subject-card';
import {setupSubjectPreview} from './subject.design';

@Component({
    providers: [SubjectStore],
    imports: [Drawer, EditorPane, SubjectCard],
    template: `
        <app-drawer
                label="Inspector"
                icon="layers"
                preferenceKey="design:inspector"
                [config]="drawerConfig"
                resizeLabel="Resize Inspector"
        >
            <editor-pane title="Inspector" icon="layers" layout="cards">
                @for (subject of store.subjects(); track subject.id) {
                    <editor-subject-card [subjectId]="subject.id"/>
                }
            </editor-pane>
        </app-drawer>`,
    styles: `
      :host {
        display: flex;
        height: 100%;
        width: 100%;
        min-width: 0;
      }
    `,
})
class PanePreview {
    readonly drawerConfig = {
        width: 'var(--layout-inspector-width)',
        minWidth: 'var(--layout-drawer-min-width)',
        maxWidth: 'var(--layout-drawer-max-width)',
    };
    protected readonly store = inject(SubjectStore);

    constructor() {
        setupSubjectPreview(this.store);
    }
}

export default {component: PanePreview};

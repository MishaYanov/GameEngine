import {Component, computed, signal} from '@angular/core';
import {form, required} from '@angular/forms/signals';
import {defaultTransform, SUBJECT_DEFINITIONS} from '../../../../src/app/core/subjects/subject.models';
import {EditorDialog} from '../../../../src/app/shared/components/dialog/dialog';
import type {DialogSubject} from '../../../../src/app/shared/components/dialog/dialog-settings';

@Component({
    imports: [EditorDialog],
    template: `
        <div class="launchers">
            <button type="button" (click)="plainOpen.set(true)">Open form dialog</button>
            <button type="button" (click)="subjectOpen.set(true)">Open subject dialog</button>
        </div>
        <p role="status">{{ message() }}</p>
        <editor-dialog
                title="Project settings"
                icon="folder"
                preferenceKey="design:dialog:form"
                [(open)]="plainOpen"
                [form]="plainForm"
                (submitted)="save('Project settings')"
        />
        <editor-dialog
                title="Object properties"
                icon="object"
                preferenceKey="design:dialog:subjects"
                [(open)]="subjectOpen"
                [form]="subjectForm"
                [subjects]="subjects()"
                (submitted)="save('Object properties')"
        />
    `,
    styles: `
      .launchers {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-2);
      }

      button {
        min-height: var(--layout-subject-control-height);
        padding: var(--space-2) var(--space-3);
        border: var(--border-width) solid var(--color-subject-edge);
        border-radius: var(--radius-sm);
        background: var(--color-subject-toggle);
        color: var(--color-text);
        cursor: var(--cursor-action);
      }

      button:hover {
        background: var(--color-surface-hover);
      }

      p {
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
      }
    `,
})
class DialogPreview {
    readonly plainOpen = signal(false);
    readonly subjectOpen = signal(false);
    readonly message = signal('Open a dialog to edit its form.');
    readonly plainData = signal({name: 'My project', settings: {autosave: true, interval: 30}});
    readonly plainForm = form(this.plainData, (path) =>
        required(path.name, {message: 'Enter a project name.'}),
    );
    readonly subjectData = signal({
        object: {name: 'Object', visible: true},
        transform: defaultTransform(),
    });
    readonly subjectForm = form(this.subjectData, (path) =>
        required(path.object.name, {message: 'Enter an object name.'}),
    );
    readonly subjects = computed<readonly DialogSubject[]>(() => [
        {
            id: 'object',
            title: 'Object',
            icon: 'object',
            form: this.subjectForm.object,
            inputs: SUBJECT_DEFINITIONS.object.inputs,
        },
        {
            id: 'transform',
            title: 'Transform',
            icon: 'cube',
            form: this.subjectForm.transform,
            inputs: SUBJECT_DEFINITIONS.transform.inputs,
        },
    ]);

    save(title: string): void {
        this.message.set(`${title} applied.`);
        this.plainOpen.set(false);
        this.subjectOpen.set(false);
    }
}

export default {component: DialogPreview};

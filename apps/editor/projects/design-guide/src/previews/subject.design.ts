import {Component, signal} from '@angular/core';
import {form} from '@angular/forms/signals';
import {EditorSubject} from '../../../../src/app/shared/components/subject/subject';
import {SubjectStore} from '../../../../src/app/core/subjects/subject.store';
import {defaultTransform} from '../../../../src/app/core/subjects/subject.models';

export function transformDefaults() {
    return defaultTransform();
}

export function setupSubjectPreview(store: InstanceType<typeof SubjectStore>): void {
    store.setScriptAssets([
        {
            assetId: 'player-controller',
            path: 'scripts/player_controller.rs',
            className: 'PlayerController',
            properties: [
                {name: 'speed', type: 'number', defaultValue: 5},
                {name: 'jumpHeight', type: 'number', defaultValue: 2, underExpanded: true},
                {name: 'canJump', type: 'boolean', defaultValue: true, underExpanded: true},
            ],
        },
    ]);
    store.setSubjects([
        {id: 'object', kind: 'object', data: {name: 'Object', visible: true}},
        {id: 'transform', kind: 'transform', data: defaultTransform()},
        {id: 'script', kind: 'script', script: null, data: {enabled: true, properties: {}}},
    ]);
    store.attachScript('script', 'player-controller');
}

@Component({
    imports: [EditorSubject],
    template: `
        <editor-subject
                title="Transform"
                icon="cube"
                [form]="transformForm"
        />`,
    styles: `
      :host {
        display: block;
        width: var(--layout-inspector-width);
        max-width: 100%;
      }
    `,
})
class SubjectPreview {
    readonly model = signal(transformDefaults());
    readonly transformForm = form(this.model);
}

export default {component: SubjectPreview};

import {Component, computed, effect, inject, Injector, input, linkedSignal, untracked,} from '@angular/core';
import {disabled, form, FormField, hidden} from '@angular/forms/signals';
import {
    defaultTransform,
    ObjectSubjectData,
    ScriptSubjectData,
    SubjectInputDefinition,
} from '../../../core/subjects/subject.models';
import {SubjectStore} from '../../../core/subjects/subject.store';
import {EditorSubject} from './subject';

/** Binds typed inspector data to local Signal Forms; presentation stays in EditorSubject. */
@Component({
    selector: 'editor-subject-card',
    imports: [EditorSubject, FormField],
    templateUrl: './subject-card.html',
    styleUrl: './subject-card.scss',
})
export class SubjectCard {
    readonly subjectId = input.required<string>();
    protected readonly store = inject(SubjectStore);
    protected readonly subject = computed(() =>
        this.store.subjects().find((subject) => subject.id === this.subjectId()),
    );
    protected readonly script = computed(() => {
        const subject = this.subject();
        return subject?.kind === 'script' ? subject.script : null;
    });
    protected readonly missingAsset = computed(() => {
        const script = this.script();
        return script && !this.store.scriptAssets().some((asset) => asset.assetId === script.assetId)
            ? script
            : null;
    });
    protected readonly scriptInputs = computed<readonly SubjectInputDefinition<ScriptSubjectData>[]>(
        () => {
            const asset = this.store
                .scriptAssets()
                .find((asset) => asset.assetId === this.script()?.assetId);
            return [
                ...this.store.definitions.script.inputs,
                ...(asset?.properties.map((property) => ({
                    path: `properties.${property.name}` as const,
                    type: property.type,
                    underExpanded: property.underExpanded ?? false,
                })) ?? []),
            ];
        },
    );
    private readonly injector = inject(Injector);
    private readonly objectModel = linkedSignal<ObjectSubjectData>(() => {
        const subject = this.subject();
        return subject?.kind === 'object' ? subject.data : {name: '', visible: true};
    });
    protected readonly objectForm = computed(() =>
        untracked(() => form(this.objectModel, {injector: this.injector})),
    );
    private readonly transformModel = linkedSignal(() => {
        const subject = this.subject();
        return subject?.kind === 'transform' ? subject.data : defaultTransform();
    });
    protected readonly transformForm = computed(() =>
        untracked(() => form(this.transformModel, {injector: this.injector})),
    );
    private readonly scriptModel = linkedSignal<ScriptSubjectData>(() => {
        const subject = this.subject();
        return subject?.kind === 'script' ? subject.data : {enabled: true, properties: {}};
    });
    protected readonly scriptForm = computed(() =>
        untracked(() =>
            form(
                this.scriptModel,
                (path) => {
                    hidden(path.properties, {
                        when: () => Object.keys(this.scriptModel().properties).length === 0,
                    });
                    disabled(path, {
                        when: () => {
                            const subject = this.subject();
                            return (
                                subject?.kind === 'script' &&
                                subject.script !== null &&
                                !this.store
                                    .scriptAssets()
                                    .some((asset) => asset.assetId === subject.script?.assetId)
                            );
                        },
                    });
                },
                {injector: this.injector},
            ),
        ),
    );
    private readonly assetId = linkedSignal(() => {
        const subject = this.subject();
        return subject?.kind === 'script' ? (subject.script?.assetId ?? '') : '';
    });
    protected readonly assetForm = computed(() =>
        untracked(() => form(this.assetId, {injector: this.injector})),
    );

    constructor() {
        effect(() => {
            const subject = this.subject();
            if (!subject) return;
            switch (subject.kind) {
                case 'object': {
                    const data = this.objectModel();
                    if (data !== subject.data)
                        this.store.updateSubject({id: subject.id, kind: 'object', data});
                    break;
                }
                case 'transform': {
                    const data = this.transformModel();
                    if (data !== subject.data)
                        this.store.updateSubject({id: subject.id, kind: 'transform', data});
                    break;
                }
                case 'script': {
                    const id = this.assetId();
                    if (id !== (subject.script?.assetId ?? '')) {
                        if (id) this.store.attachScript(subject.id, id);
                        else this.store.detachScript(subject.id);
                    } else {
                        const data = this.scriptModel();
                        if (data !== subject.data)
                            this.store.updateSubject({id: subject.id, kind: 'script', data});
                    }
                    break;
                }
            }
        });
    }
}

import {Component, computed, DestroyRef, ElementRef, forwardRef, inject, input, model,} from '@angular/core';
import {Field, FieldTree, FormField} from '@angular/forms/signals';
import {EditorIcon, EditorIconName} from '../../icons/editor-icon';
import {SubjectRegistry} from '../../services/subject-registry.service';
import type {SubjectInput} from '../../../core/subjects/subject.models';

interface SubjectField {
    readonly key: string;
    readonly label: string;
    readonly field: FieldTree<unknown>;
}

type SubjectChoice = string | { readonly value: string; readonly label: string };
export type SubjectOptions = Readonly<Record<string, readonly SubjectChoice[]>>;

let nextTraitsId = 0;

function fieldEntries(node: FieldTree<unknown>): readonly SubjectField[] {
    const value = node().value();
    if (value === null || typeof value !== 'object') return [];
    const children: [string, FieldTree<unknown> | undefined][] = Array.isArray(value)
        ? Array.from(node as FieldTree<readonly unknown[]>).map((field, index) => [
            String(index),
            field,
        ])
        : Array.from(node as unknown as Iterable<[string, FieldTree<unknown> | undefined]>);
    return children.flatMap(([key, field]) => (field ? [{key, label: labelFor(key), field}] : []));
}

function labelFor(key: string): string {
    const text = key.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/[_-]/g, ' ');
    return text.charAt(0).toUpperCase() + text.slice(1);
}

@Component({
    selector: 'editor-subject-fields',
    imports: [FormField, forwardRef(() => SubjectFields)],
    template: `
        @for (entry of entries(); track entry.key) {
            @if (!entry.field().hidden()) {
                @if (isGroup(entry.field)) {
                    <fieldset [hidden]="concealed(entry)">
                        <legend>{{ entry.label }}</legend>
                        <editor-subject-fields
                                [fields]="entry.field"
                                [path]="path() + ' ' + entry.label"
                                [fieldPath]="entryPath(entry.key)"
                                [advancedTraits]="advancedTraits()"
                                [expanded]="expanded()"
                                [options]="options()"
                        />
                    </fieldset>
                } @else {
                    <div class="field" [hidden]="concealed(entry)">
                        @if (numeric(entry.field); as field) {
                            <label class="number-control">
                                <span>{{ entry.label }}</span>
                                <input
                                        type="number"
                                        step="any"
                                        [formField]="field"
                                        [attr.aria-label]="path() + ' ' + entry.label"
                                        [attr.aria-describedby]="errorId(entry.field)"
                                        [attr.aria-invalid]="errorId(entry.field) ? 'true' : null"
                                />
                            </label>
                        } @else if (boolean(entry.field); as field) {
                            <label class="boolean-control">
                                <input
                                        type="checkbox"
                                        [formField]="field"
                                        [attr.aria-label]="path() + ' ' + entry.label"
                                        [attr.aria-describedby]="errorId(entry.field)"
                                        [attr.aria-invalid]="errorId(entry.field) ? 'true' : null"
                                />
                                <span>{{ entry.label }}</span>
                            </label>
                        } @else if (text(entry.field); as field) {
                            <label class="text-control">
                                <span>{{ entry.label }}</span>
                                @if (options()[entryPath(entry.key)]; as choices) {
                                    <select [formField]="field" [attr.aria-label]="path() + ' ' + entry.label">
                                        @for (choice of choices; track choiceValue(choice)) {
                                            <option [value]="choiceValue(choice)">{{ choiceLabel(choice) }}</option>
                                        }
                                    </select>
                                } @else {
                                    <input
                                            type="text"
                                            [formField]="field"
                                            [attr.aria-label]="path() + ' ' + entry.label"
                                            [attr.aria-describedby]="errorId(entry.field)"
                                            [attr.aria-invalid]="errorId(entry.field) ? 'true' : null"
                                    />
                                }
                            </label>
                        }
                        @if (errorId(entry.field); as id) {
                            <span class="error" [id]="id">{{ errorMessage(entry.field) }}</span>
                        }
                    </div>
                }
            }
        }
    `,
    styleUrl: './subject-fields.scss',
    host: {'[class.vector]': 'vector()'},
})
export class SubjectFields {
    readonly options = input<SubjectOptions>({});

    protected choiceValue(choice: SubjectChoice): string {
        return typeof choice === 'string' ? choice : choice.value;
    }

    protected choiceLabel(choice: SubjectChoice): string {
        return typeof choice === 'string' ? choice : choice.label;
    }

    readonly fields = input.required<FieldTree<unknown>>();
    readonly path = input.required<string>();
    readonly fieldPath = input('');
    readonly advancedTraits = input<readonly string[]>([]);
    readonly expanded = input(true);
    protected readonly entries = computed(() => fieldEntries(this.fields()), {
        equal: (previous, next) =>
            previous.length === next.length &&
            previous.every(
                (entry, index) => entry.key === next[index].key && entry.field === next[index].field,
            ),
    });
    protected readonly vector = computed(() => {
        const entries = this.entries();
        return entries.length > 0 && entries.every((entry) => ['x', 'y', 'z', 'w'].includes(entry.key));
    });

    protected isGroup(field: FieldTree<unknown>): boolean {
        const value = field().value();
        return value !== null && typeof value === 'object';
    }

    protected entryPath(key: string): string {
        return this.fieldPath() ? `${this.fieldPath()}.${key}` : key;
    }

    protected concealed(entry: SubjectField): boolean {
        return (
            !this.expanded() &&
            this.advancedTraits().length > 0 &&
            !this.primary(entry.field, this.entryPath(entry.key))
        );
    }

    protected numeric(field: FieldTree<unknown>): Field<number | null> | undefined {
        const value = field().value();
        return typeof value === 'number' || value === null
            ? (field as Field<number | null>)
            : undefined;
    }

    protected boolean(field: FieldTree<unknown>): Field<boolean> | undefined {
        return typeof field().value() === 'boolean' ? (field as Field<boolean>) : undefined;
    }

    protected text(field: FieldTree<unknown>): Field<string> | undefined {
        return typeof field().value() === 'string' ? (field as Field<string>) : undefined;
    }

    protected errorId(field: FieldTree<unknown>): string | null {
        return field().touched() && field().invalid() ? `${field().name()}-error` : null;
    }

    protected errorMessage(field: FieldTree<unknown>): string {
        return field()
            .errors()
            .map((error) => error.message ?? 'Enter a valid value.')
            .join(' ');
    }

    private primary(field: FieldTree<unknown>, path: string): boolean {
        if (
            field().hidden() ||
            this.advancedTraits().some((trait) => path === trait || path.startsWith(`${trait}.`))
        )
            return false;
        if (!this.isGroup(field)) return true;
        return fieldEntries(field).some((child) => this.primary(child.field, `${path}.${child.key}`));
    }
}

@Component({
    selector: 'editor-subject',
    imports: [SubjectFields, EditorIcon],
    templateUrl: './subject.html',
    styleUrl: './subject.scss',
    host: {'[hidden]': 'form()().hidden()'},
})
export class EditorSubject {
    readonly renderFields = input(true);
    readonly options = input<SubjectOptions>({});
    readonly title = input.required<string>();
    readonly icon = input<EditorIconName>();
    readonly form = input.required<FieldTree<unknown>>();
    readonly inputs = input<readonly SubjectInput[]>([]);
    readonly expanded = model(false);
    readonly collapsed = model(false);
    protected readonly advancedTraits = computed(() =>
        this.inputs()
            .filter((input) => input.underExpanded)
            .map((input) => input.path),
    );
    protected readonly traitsId = `subject-traits-${nextTraitsId++}`;
    protected readonly bodyId = `${this.traitsId}-body`;
    protected readonly expandable = computed(
        () => this.renderFields() && this.advancedTraits().length > 0,
    );

    constructor() {
        const registry = inject(SubjectRegistry, {optional: true});
        const element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
        if (registry) {
            const unregister = registry.register({
                element,
                title: this.title,
                icon: this.icon,
                hidden: () => this.form()().hidden(),
                focus: () => {
                    element.scrollIntoView?.({block: 'nearest'});
                    element.querySelector<HTMLElement>('h3')?.focus({preventScroll: true});
                },
            });
            inject(DestroyRef).onDestroy(unregister);
        }
    }

    protected toggleFromHeader(event: MouseEvent): void {
        if (event.target instanceof Element && event.target.closest('.subject-actions')) return;
        this.collapsed.update((collapsed) => !collapsed);
        (event.currentTarget as HTMLElement)
            .querySelector<HTMLButtonElement>('.header-toggle')
            ?.focus({preventScroll: true});
    }
}

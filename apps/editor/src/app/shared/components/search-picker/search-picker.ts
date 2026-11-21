import {
    afterRenderEffect,
    Component,
    computed,
    ElementRef,
    input,
    linkedSignal,
    output,
    signal,
    viewChild
} from '@angular/core';

export interface SearchPickerOption {
    readonly id: string;
    readonly label: string;
    readonly description?: string;
    readonly keywords?: string;
    readonly disabled?: boolean;
}

let nextPickerId = 0;

@Component({
    selector: 'editor-search-picker',
    templateUrl: './search-picker.html',
    styleUrl: './search-picker.scss',
})
export class EditorSearchPicker {
    readonly label = input.required<string>();
    readonly placeholder = input('Search…');
    readonly options = input.required<readonly SearchPickerOption[]>();
    readonly value = input<string | null>(null);
    readonly disabled = input(false);
    readonly expanded = input(false);
    readonly autofocus = input(false);
    readonly clearable = input(false);
    readonly picked = output<string>();

    protected readonly id = `editor-search-picker-${nextPickerId++}`;
    protected readonly selected = computed(() => this.options().find(option => option.id === this.value()));
    protected readonly missing = computed(() => !!this.value() && !this.selected());
    protected readonly visible = computed(() => !this.disabled() && (this.expanded() || this.opened()));
    protected readonly display = computed(() => this.query() ?? this.selected()?.label ?? this.value() ?? '');
    private readonly field = viewChild.required<ElementRef<HTMLInputElement>>('field');
    private readonly list = viewChild<ElementRef<HTMLElement>>('list');
    private readonly opened = signal(false);
    private readonly query = signal<string | null>(null);
    private readonly searchable = computed(() => this.options().map(option => ({
        option,
        text: `${option.label} ${option.description ?? ''} ${option.keywords ?? ''} ${option.id}`.toLocaleLowerCase(),
    })));
    protected readonly matches = computed(() => {
        const terms = (this.query() ?? '').trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
        return this.searchable()
            .filter(({text}) => terms.every(term => text.includes(term)))
            .map(({option}) => option);
    });
    private readonly enabled = computed(() => this.matches().filter(option => !option.disabled));
    protected readonly active = linkedSignal({
        source: () => ({options: this.enabled(), query: this.query(), value: this.value()}),
        computation: ({options, query, value}) =>
            (query === null ? options.find(option => option.id === value)?.id : undefined) ??
            options[0]?.id ?? null,
    });

    constructor() {
        afterRenderEffect({
            mixedReadWrite: () => {
                const field = this.field().nativeElement;
                if (this.autofocus() && !this.disabled() && !field.closest('dialog:not([open])')) {
                    field.focus({preventScroll: true});
                }
            },
        });
        afterRenderEffect({
            mixedReadWrite: () => {
                const list = this.list()?.nativeElement;
                const active = this.active();
                if (!list || active === null) return;
                const item = list.ownerDocument.getElementById(this.optionId(active));
                if (!item) return;
                const listBounds = list.getBoundingClientRect();
                const itemBounds = item.getBoundingClientRect();
                if (itemBounds.top < listBounds.top) list.scrollTop += itemBounds.top - listBounds.top;
                else if (itemBounds.bottom > listBounds.bottom) list.scrollTop += itemBounds.bottom - listBounds.bottom;
            },
        });
    }

    protected optionId(id: string): string {
        return `${this.id}-option-${encodeURIComponent(id)}`;
    }

    protected open(event?: FocusEvent): void {
        if (this.disabled()) return;
        this.opened.set(true);
        if (event && this.query() === null && this.display()) this.field().nativeElement.select();
    }

    protected close(): void {
        if (this.expanded()) return;
        this.opened.set(false);
        this.query.set(null);
        this.field().nativeElement.value = this.display();
    }

    protected search(value: string): void {
        if (this.disabled()) return;
        this.query.set(value);
        this.opened.set(true);
    }

    protected choose(option: SearchPickerOption): void {
        if (this.disabled() || option.disabled) return;
        this.close();
        this.picked.emit(option.id);
    }

    protected clear(): void {
        if (this.disabled()) return;
        this.field().nativeElement.focus({preventScroll: true});
        this.close();
        this.picked.emit('');
    }

    protected navigate(event: KeyboardEvent): void {
        if (this.disabled() || event.isComposing || event.altKey || event.ctrlKey || event.metaKey) return;
        if (event.key === 'Escape' && this.visible() && !this.expanded()) {
            event.preventDefault();
            event.stopPropagation();
            this.close();
            return;
        }
        if (event.key === 'Enter' && this.visible()) {
            event.preventDefault();
            event.stopPropagation();
            const option = this.enabled().find(option => option.id === this.active());
            if (option) this.choose(option);
            return;
        }
        if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
        event.preventDefault();
        event.stopPropagation();
        const options = this.enabled();
        const direction = event.key === 'ArrowDown' ? 1 : -1;
        const index = options.findIndex(option => option.id === this.active());
        const next = !this.visible() || index < 0
            ? (direction > 0 ? 0 : options.length - 1)
            : (index + direction + options.length) % options.length;
        this.open();
        this.active.set(options[next]?.id ?? null);
    }
}

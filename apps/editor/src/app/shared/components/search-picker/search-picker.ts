import {
    afterNextRender,
    afterRenderEffect,
    Component,
    computed,
    ElementRef,
    inject,
    Injector,
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
    readonly describedBy = input<string | null>(null);
    readonly resultLimit = input(Infinity);
    readonly revealResults = input(false);
    readonly picked = output<string>();

    protected readonly id = `editor-search-picker-${nextPickerId++}`;
    protected readonly selected = computed(() => this.options().find(option => option.id === this.value()));
    protected readonly missing = computed(() => !!this.value() && !this.selected());
    protected readonly descriptionIds = computed(() =>
        [this.describedBy(), this.missing() ? this.id + '-missing' : null].filter(Boolean).join(' ') || null);
    protected readonly visible = computed(() => !this.disabled() && (this.expanded() || this.opened()));
    protected readonly display = computed(() => this.query() ?? this.selected()?.label ?? this.value() ?? '');
    protected readonly displayTitle = computed(() => this.visible() ? null : this.selected()?.label ?? null);
    private readonly injector = inject(Injector);
    private readonly field = viewChild.required<ElementRef<HTMLInputElement>>('field');
    private readonly list = viewChild<ElementRef<HTMLElement>>('list');
    private readonly opened = signal(false);
    private readonly query = signal<string | null>(null);
    private readonly searchable = computed(() => this.options().map(option => ({
        option,
        text: `${option.label} ${option.description ?? ''} ${option.keywords ?? ''} ${option.id}`.toLocaleLowerCase(),
    })));
    private readonly searchResult = computed(() => {
        const terms = (this.query() ?? '').trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
        const limit = Math.max(1, this.resultLimit());
        if (terms.length === 0) {
            const options = this.options();
            return options.length <= limit ? {options, limited: false} : {
                options: options.slice(0, limit),
                limited: true
            };
        }

        const options: SearchPickerOption[] = [];
        for (const {text, option} of this.searchable()) {
            if (!terms.every(term => text.includes(term))) continue;
            if (options.length >= limit) return {options, limited: true};
            options.push(option);
        }
        return {options, limited: false};
    });
    protected readonly matches = computed(() => this.searchResult().options);
    protected readonly limited = computed(() => this.searchResult().limited);
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
                if (this.revealResults()) this.revealInPane(list, item);
            },
        });
    }

    private revealInPane(list: HTMLElement, active: HTMLElement): void {
        const field = this.field().nativeElement;
        const owner = field.ownerDocument;
        if (owner.activeElement !== field) return;
        const view = owner.defaultView;
        if (!view) return;
        let pane = list.parentElement;
        while (pane && pane !== owner.body) {
            const overflow = view.getComputedStyle(pane).overflowY;
            if ((overflow === 'auto' || overflow === 'scroll') && pane.scrollHeight > pane.clientHeight) break;
            pane = pane.parentElement;
        }
        if (!pane || pane === owner.body) return;
        const paneTop = pane.getBoundingClientRect().top + pane.clientTop;
        const paneBottom = paneTop + pane.clientHeight;
        const overlap = active.getBoundingClientRect().bottom - paneBottom;
        const roomAboveField = field.getBoundingClientRect().top - paneTop;
        // Move only the local pane and leave the filtered input visible.
        if (overlap > 0 && roomAboveField > 0) pane.scrollTop += Math.min(overlap, roomAboveField);
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
        afterNextRender({
            write: () => {
                if (!this.visible()) this.field().nativeElement.value = this.display();
            }
        }, {injector: this.injector});
    }

    protected search(value: string): void {
        if (this.disabled()) return;
        this.query.set(value);
        this.opened.set(true);
    }

    protected choose(option: SearchPickerOption): void {
        if (this.disabled() || option.disabled) return;
        this.picked.emit(option.id);
        this.close();
    }

    protected clear(): void {
        if (this.disabled()) return;
        this.field().nativeElement.focus({preventScroll: true});
        this.picked.emit('');
        this.close();
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

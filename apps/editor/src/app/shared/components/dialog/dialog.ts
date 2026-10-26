import {
    afterRenderEffect,
    Component,
    computed,
    DestroyRef,
    DOCUMENT,
    ElementRef,
    inject,
    input,
    model,
    output,
    signal,
    viewChild,
} from '@angular/core';
import {FieldTree, submit} from '@angular/forms/signals';
import {EditorIcon, EditorIconName} from '../../icons/editor-icon';
import {Config} from '../../models/config.model';
import {EditorComponentService} from '../../services/editor-component.service';
import {EditorSubject, SubjectFields} from '../subject/subject';
import {DEFAULT_DIALOG_SETTINGS, DialogSettings, dialogSettings, DialogSubject,} from './dialog-settings';

let nextDialogId = 0;

@Component({
    selector: 'editor-dialog',
    imports: [EditorIcon, EditorSubject, SubjectFields],
    providers: [EditorComponentService],
    templateUrl: './dialog.html',
    styleUrl: './dialog.scss',
})
export class EditorDialog {
    readonly title = input.required<string>();
    readonly icon = input<EditorIconName>();
    readonly open = model(false);
    readonly config = model<Config<DialogSettings>>({});
    readonly preferenceKey = input<string>();
    readonly form = input<FieldTree<unknown>>();
    readonly subjects = input<readonly DialogSubject[]>([]);
    /** Set false when projecting custom rows instead of rendering the form automatically. */
    readonly renderFields = input(true);
    readonly submitLabel = input('Apply');
    readonly submitted = output<void>();
    readonly closed = output<void>();
    protected readonly titleId = `editor-dialog-${nextDialogId++}`;
    protected readonly rendered = signal(false);
    protected readonly settings = computed(() => ({
        ...DEFAULT_DIALOG_SETTINGS,
        ...dialogSettings(this.config()),
    }));
    protected readonly expanded = computed(
        () => this.settings().expandable && this.settings().expanded,
    );
    protected readonly pending = computed(
        () =>
            this.form()?.().pending() ||
            this.form()?.().submitting() ||
            this.subjects().some((subject) => subject.form().pending() || subject.form().submitting()),
    );
    private readonly surface = viewChild<ElementRef<HTMLDialogElement>>('surface');
    private readonly behavior = inject(EditorComponentService);
    readonly configReady = this.behavior.initialized;
    readonly configError = this.behavior.error;
    private readonly document = inject(DOCUMENT);
    private drag:
        | { pointer: number; x: number; y: number; target: HTMLElement; mode: 'move' | 'resize' }
        | undefined;
    private frame: number | undefined;
    private dx = 0;
    private dy = 0;
    private listening = false;
    private returnFocus: HTMLElement | null = null;

    constructor() {
        this.behavior.connectConfig(
            () => this.preferenceKey() ?? `dialog:${this.title()}`,
            this.config,
            DEFAULT_DIALOG_SETTINGS,
            dialogSettings,
        );
        afterRenderEffect(() => {
            const dialog = this.surface()?.nativeElement;
            if (!dialog) return;
            if (this.open() && !dialog.open) {
                this.rendered.set(true);
                const active = this.document.activeElement;
                this.returnFocus =
                    active instanceof HTMLElement && !dialog.contains(active) ? active : null;
                dialog.showModal();
            } else if (!this.open() && dialog.open) dialog.close();
            this.listenForResize(this.open());
            if (this.open()) this.constrainPosition();
        });
        inject(DestroyRef).onDestroy(() => {
            this.stopDrag();
            this.listenForResize(false);
            const dialog = this.surface()?.nativeElement;
            if (dialog?.open) dialog.close();
        });
    }

    close(): void {
        this.stopDrag();
        this.open.set(false);
    }

    protected didClose(): void {
        if (this.surface()?.nativeElement.open) return;
        this.stopDrag();
        this.open.set(false);
        if (this.returnFocus?.isConnected) this.returnFocus.focus({preventScroll: true});
        this.closed.emit();
    }

    protected trapFocus(event: KeyboardEvent): void {
        if (event.key !== 'Tab') return;
        const dialog = this.surface()?.nativeElement;
        if (!dialog) return;
        const controls = [
            ...dialog.querySelectorAll<HTMLElement>(
                'button, input, select, textarea, a[href], [tabindex]',
            ),
        ].filter(
            (control) =>
                control.tabIndex >= 0 &&
                !control.matches(':disabled') &&
                control.getClientRects().length > 0,
        );
        const first = controls[0];
        const last = controls.at(-1);
        if (event.shiftKey && this.document.activeElement === first && last) {
            event.preventDefault();
            last.focus();
        } else if (!event.shiftKey && this.document.activeElement === last && first) {
            event.preventDefault();
            first.focus();
        }
    }

    protected cancel(event: Event): void {
        event.preventDefault();
        if (this.settings().closeOnEscape) this.close();
    }

    protected backdrop(event: MouseEvent): void {
        const dialog = this.surface()?.nativeElement;
        if (!dialog || event.target !== dialog || !this.settings().closeOnBackdrop) return;
        const rect = dialog.getBoundingClientRect();
        if (
            event.clientX < rect.left ||
            event.clientX > rect.right ||
            event.clientY < rect.top ||
            event.clientY > rect.bottom
        )
            this.close();
    }

    protected toggleExpanded(): void {
        this.stopDrag();
        this.config.update((config) => ({...config, expanded: !this.expanded(), x: 0, y: 0}));
    }

    protected startDrag(event: PointerEvent, mode: 'move' | 'resize' = 'move'): void {
        if (
            !(mode === 'move' ? this.settings().draggable : this.settings().resizable) ||
            this.expanded() ||
            event.button !== 0 ||
            event.isPrimary === false ||
            (mode === 'move' && (event.target as Element).closest('[data-dialog-action]'))
        )
            return;
        const target = event.currentTarget as HTMLElement;
        this.stopDrag();
        target.setPointerCapture(event.pointerId);
        this.drag = {pointer: event.pointerId, x: event.clientX, y: event.clientY, target, mode};
        (mode === 'move' ? target.querySelector<HTMLButtonElement>('.drag-handle') : target)?.focus({
            preventScroll: true,
        });
        event.preventDefault();
    }

    protected moveDrag(event: PointerEvent): void {
        if (!this.drag || this.drag.pointer !== event.pointerId) return;
        if (
            !(this.drag.mode === 'move' ? this.settings().draggable : this.settings().resizable) ||
            this.expanded()
        ) {
            this.stopDrag();
            return;
        }
        this.dx += event.clientX - this.drag.x;
        this.dy += event.clientY - this.drag.y;
        this.drag.x = event.clientX;
        this.drag.y = event.clientY;
        if (this.frame === undefined)
            this.frame = this.document.defaultView?.requestAnimationFrame(() => {
                this.frame = undefined;
                this.flushPointer();
            });
    }

    protected finishDrag(event: PointerEvent): void {
        if (this.drag?.pointer === event.pointerId) {
            this.flushPointer();
            this.stopDrag();
        }
    }

    protected resizeWithKeyboard(event: KeyboardEvent): void {
        if (!this.settings().resizable || this.expanded()) return;
        const step = event.shiftKey ? 40 : 10;
        const dx = event.key === 'ArrowRight' ? step : event.key === 'ArrowLeft' ? -step : 0;
        const dy = event.key === 'ArrowDown' ? step : event.key === 'ArrowUp' ? -step : 0;
        if (!dx && !dy) return;
        event.preventDefault();
        this.resizeBy(dx, dy);
    }

    protected moveWithKeyboard(event: KeyboardEvent): void {
        if (!this.settings().draggable || this.expanded()) return;
        const step = event.shiftKey ? 40 : 10;
        const directions: Record<string, readonly [number, number]> = {
            ArrowLeft: [-step, 0],
            ArrowRight: [step, 0],
            ArrowUp: [0, -step],
            ArrowDown: [0, step],
        };
        const delta = directions[event.key];
        if (!delta) return;
        event.preventDefault();
        this.moveBy(...delta);
    }

    protected constrainPosition(): void {
        this.moveBy(0, 0);
    }

    protected async submitForm(event: Event): Promise<void> {
        event.preventDefault();
        const root = this.form();
        const fields = root ? [root] : this.subjects().map((subject) => subject.form);
        if (!fields.length || this.pending()) return;
        // Validate every supplied form, including fields currently folded in a Subject.
        const valid = await Promise.all(fields.map((field) => submit(field, async () => undefined)));
        if (valid.every(Boolean)) this.submitted.emit();
    }

    private readonly onWindowResize = () => this.constrainPosition();

    private listenForResize(open: boolean): void {
        if (open === this.listening) return;
        this.listening = open;
        const view = this.document.defaultView;
        if (open) view?.addEventListener('resize', this.onWindowResize, {passive: true});
        else view?.removeEventListener('resize', this.onWindowResize);
    }

    private flushPointer(): void {
        const dx = this.dx;
        const dy = this.dy;
        this.dx = this.dy = 0;
        if (!this.drag || (!dx && !dy)) return;
        if (
            !this.open() ||
            this.expanded() ||
            !(this.drag.mode === 'resize' ? this.settings().resizable : this.settings().draggable)
        )
            return;
        if (this.drag.mode === 'resize') this.resizeBy(dx, dy);
        else this.moveBy(dx, dy);
    }

    private stopDrag(): void {
        const drag = this.drag;
        this.drag = undefined;
        if (this.frame !== undefined) this.document.defaultView?.cancelAnimationFrame(this.frame);
        this.frame = undefined;
        this.dx = this.dy = 0;
        if (drag?.target.hasPointerCapture(drag.pointer))
            drag.target.releasePointerCapture(drag.pointer);
    }

    private resizeBy(dx: number, dy: number): void {
        const dialog = this.surface()?.nativeElement;
        const view = this.document.defaultView;
        if (!dialog?.open || !view) return;
        const before = dialog.getBoundingClientRect();
        const settings = this.settings();
        const width = Math.max(
            0,
            Math.min(before.width + dx, this.document.documentElement.clientWidth - before.left),
        );
        const height = Math.max(0, Math.min(before.height + dy, view.innerHeight - before.top));
        // Let CSS resolve theme variables, min/max expressions and viewport limits.
        dialog.style.setProperty('--dialog-width', `${width}px`);
        dialog.style.setProperty('--dialog-height', `${height}px`);
        const after = dialog.getBoundingClientRect();
        const sizedWidth = `${Math.round(after.width)}px`;
        const sizedHeight = `${Math.round(after.height)}px`;
        dialog.style.setProperty('--dialog-width', sizedWidth);
        dialog.style.setProperty('--dialog-height', sizedHeight);
        if (settings.width === sizedWidth && settings.height === sizedHeight) return;
        this.config.update((config) => ({
            ...config,
            width: sizedWidth,
            height: sizedHeight,
            x: settings.x + (after.width - before.width) / 2,
            y: settings.y + (after.height - before.height) / 2,
        }));
    }

    private moveBy(dx: number, dy: number): void {
        const dialog = this.surface()?.nativeElement;
        const view = this.document.defaultView;
        if (!dialog?.open || !view || this.expanded()) return;
        const rect = dialog.getBoundingClientRect();
        const settings = this.settings();
        const x = settings.x + Math.min(Math.max(dx, -rect.left), view.innerWidth - rect.right);
        const y = settings.y + Math.min(Math.max(dy, -rect.top), view.innerHeight - rect.bottom);
        if (settings.x !== x || settings.y !== y) this.config.update((config) => ({...config, x, y}));
    }
}

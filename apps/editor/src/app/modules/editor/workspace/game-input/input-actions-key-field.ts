import {Component, DestroyRef, effect, inject, input, output, signal, untracked} from '@angular/core';
import {gameKeyFromCode} from '../../../../core/game-input/game-input.models';
import {INPUT_ACTION_KEYS} from '../../../../core/game-input/input-actions.models';

const KEY_CAPTURE_MS = 8000;
const knownKeys = new Set(INPUT_ACTION_KEYS.map(item => item.key));

@Component({
    selector: 'editor-input-actions-key-field',
    template: `
        <label>{{ label() }}
            <select #choice [value]="value()" [disabled]="disabled()" (change)="select(choice.value)">
                @if (!known(value())) {
                    <option [value]="value()" [selected]="true">{{ value() }}</option>
                }
                @for (key of keys; track key.key) {
                    <option [value]="key.key" [selected]="key.key === value()">{{ key.label }}</option>
                }
            </select>
        </label>
        <button #capture type="button" [disabled]="disabled()" [attr.aria-pressed]="armed()"
                [attr.aria-label]="'Capture ' + label()" (click)="begin(capture)"
                (keydown)="captureKey($event)" (blur)="cancel()">
            {{ armed() ? 'Listening…' : 'Capture Key' }}
        </button>
        @if (message(); as text) {
            <small role="status">{{ text }}</small>
        }
    `,
    styles: [' :host { display: flex; align-items: end; flex-wrap: wrap; gap: var(--space-2); } label { display: grid; gap: var(--space-1); }'],
    host: {'(window:blur)': 'cancel()'},
})
export class InputActionsKeyField {
    readonly value = input.required<string>();
    readonly label = input.required<string>();
    readonly disabled = input(false);
    readonly keySelected = output<string>();
    protected readonly keys = INPUT_ACTION_KEYS;
    protected readonly armed = signal(false);
    protected readonly message = signal<string | null>(null);
    private sequence = 0;
    private original = '';
    private timer: ReturnType<typeof setTimeout> | null = null;

    constructor() {
        effect(() => {
            if (this.armed() && (this.disabled() || this.original !== this.value())) untracked(() => this.cancel());
        });
        inject(DestroyRef).onDestroy(() => this.cancel());
    }

    protected known(value: string): boolean { return knownKeys.has(value); }

    protected select(value: string): void {
        if (this.disabled() || !this.known(value)) return;
        this.cancel();
        this.keySelected.emit(value);
    }

    protected begin(button: HTMLButtonElement): void {
        if (this.disabled()) return;
        const wasArmed = this.armed();
        this.cancel();
        if (wasArmed) return;
        const request = this.sequence;
        this.original = this.value();
        this.armed.set(true);
        this.message.set('Press a key within 8 seconds. Escape cancels.');
        button.focus({preventScroll: true});
        this.timer = setTimeout(() => {
            if (request !== this.sequence) return;
            this.cancel();
            this.message.set('Key capture timed out.');
        }, KEY_CAPTURE_MS);
    }

    protected cancel(): void {
        this.sequence++;
        this.armed.set(false);
        this.message.set(null);
        if (this.timer !== null) clearTimeout(this.timer);
        this.timer = null;
    }

    protected captureKey(event: KeyboardEvent): void {
        if (!this.armed() || this.disabled() || event.isComposing || event.repeat) return;
        if (event.code === 'Tab') {
            this.cancel();
            return;
        }
        if (event.ctrlKey && !event.code.startsWith('Control') ||
            event.altKey && !event.code.startsWith('Alt') || event.metaKey) return;
        event.preventDefault();
        event.stopPropagation();
        if (event.code === 'Escape' || event.key === 'Escape') {
            this.cancel();
            return;
        }
        const key = gameKeyFromCode(event.code);
        if (!this.known(key)) {
            this.message.set('That key is unavailable. Try another key or press Escape.');
            return;
        }
        this.cancel();
        this.keySelected.emit(key);
    }
}

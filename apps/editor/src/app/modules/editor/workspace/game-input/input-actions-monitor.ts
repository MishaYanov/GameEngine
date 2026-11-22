import {Component, computed, DestroyRef, effect, inject, input, signal, untracked} from '@angular/core';
import {gameKeyFromCode} from '../../../../core/game-input/game-input.models';
import {FACE_BUTTONS, type InputActionKeys, type InputActionMap} from '../../../../core/game-input/input-actions.models';

type Axis = readonly [number, number];
const zero: Axis = [0, 0];

interface MonitorSample {
    readonly buttons: readonly boolean[];
    readonly left: Axis;
    readonly right: Axis;
}

@Component({
    selector: 'editor-input-actions-monitor',
    template: `
        <section class="monitor" tabindex="0" aria-label="Monitor Input Actions"
                 (focus)="focus()" (blur)="clear()" (keydown)="key($event, true)" (keyup)="key($event, false)"
                 (pointermove)="pointer($event)">
            <h3>Input monitor</h3>
            <p>Focus this area to test keys, pointer movement, or a connected gamepad.</p>
            @if (focused()) {
                <p>{{ device() ? 'Gamepad connected' : 'No gamepad detected' }}</p>
            }
            <dl>
                @for (row of values(); track $index) {
                    <dt>{{ row.label }}</dt><dd>{{ row.value }}</dd>
                }
            </dl>
        </section>
    `,
    styles: [`
        .monitor { border: 1px solid var(--color-border); padding: var(--space-2); }
        dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: var(--space-1) var(--space-3); }
        dd { margin: 0; }
    `],
    host: {'(window:blur)': 'clear()'},
})
export class InputActionsMonitor {
    readonly active = input(false);
    readonly map = input.required<InputActionMap>();
    protected readonly focused = signal(false);
    private readonly held = signal<ReadonlySet<string>>(new Set());
    protected readonly device = signal<MonitorSample | null>(null);
    private readonly delta = signal<Axis>(zero);
    private sampleTimer: ReturnType<typeof setInterval> | null = null;
    private pointerTimer: ReturnType<typeof setTimeout> | null = null;
    protected readonly values = computed(() => {
        const held = this.held();
        const pad = this.device();
        const pointer = this.delta();
        return this.map().actions.map(action => {
            const keyboard = (keys: InputActionKeys) => normalize(
                Number(held.has(keys.positive_x)) - Number(held.has(keys.negative_x)),
                Number(held.has(keys.positive_y)) - Number(held.has(keys.negative_y)), 0,
            );
            if (action.value === 'button') {
                const pressed = action.bindings.some(binding =>
                    binding.kind === 'key' ? held.has(binding.key) :
                    binding.kind === 'gamepad_button' && pad?.buttons[FACE_BUTTONS.indexOf(binding.button)]);
                return {label: action.label, value: pressed ? 'Pressed' : 'Released'};
            }
            let axis = zero;
            const walking = action.value === 'vector2';
            for (const binding of action.bindings) {
                const next = 'keys' in binding && binding.kind === (walking ? 'keyboard_vector' : 'keyboard_look') ?
                    keyboard(binding.keys) : walking && binding.kind === 'gamepad_left_stick' ? pad?.left :
                    !walking && binding.kind === 'gamepad_right_stick' ? pad?.right : undefined;
                if (next && Math.hypot(...next) > Math.hypot(...axis)) axis = next;
            }
            const show = (value: Axis) => value.map(component => component.toFixed(2)).join(', ');
            return {label: action.label, value: !walking && action.bindings.some(binding => binding.kind === 'pointer_delta')
                ? `${show(axis)} · Pointer ${show(pointer)}` : show(axis)};
        });
    });

    constructor() {
        effect(() => { if (!this.active()) untracked(() => this.clear()); });
        inject(DestroyRef).onDestroy(() => this.clear());
    }

    protected focus(): void {
        if (!this.active() || this.focused()) return;
        this.focused.set(true);
        this.pollPad();
        this.sampleTimer = setInterval(() => this.pollPad(), 50);
    }

    protected clear(): void {
        this.focused.set(false);
        this.held.set(new Set());
        this.device.set(null);
        this.delta.set(zero);
        if (this.sampleTimer !== null) clearInterval(this.sampleTimer);
        if (this.pointerTimer !== null) clearTimeout(this.pointerTimer);
        this.sampleTimer = null;
        this.pointerTimer = null;
    }

    protected key(event: KeyboardEvent, down: boolean): void {
        if (!this.active() || !this.focused() || event.isComposing || event.code === 'Escape' || event.code === 'Tab') return;
        const key = gameKeyFromCode(event.code);
        if (!this.map().actions.some(action => action.bindings.some(binding =>
            (binding.kind === 'key' && binding.key === key) || ('keys' in binding && Object.values(binding.keys).includes(key))))) return;
        event.preventDefault();
        event.stopPropagation();
        if (this.held().has(key) === down) return;
        this.held.update(previous => {
            const held = new Set(previous);
            if (down) held.add(key);
            else held.delete(key);
            return held;
        });
    }

    protected pointer(event: PointerEvent): void {
        if (!this.active() || !this.focused() || ![event.movementX, event.movementY].every(Number.isFinite)) return;
        const clamp = (value: number) => Math.max(-8, Math.min(8, value));
        this.delta.set([clamp(event.movementX), clamp(event.movementY)]);
        if (this.pointerTimer !== null) clearTimeout(this.pointerTimer);
        this.pointerTimer = setTimeout(() => this.delta.set(zero), 160);
    }

    private pollPad(): void {
        let pad: Gamepad | null = null;
        try {
            pad = [...(navigator.getGamepads?.() ?? [])].find(entry => entry?.connected && entry.mapping === 'standard') ?? null;
        } catch {
            // A focused preview can still monitor keys if this host restricts gamepads.
        }
        if (!pad) {
            if (this.device()) this.device.set(null);
            return;
        }
        const axes = (index: number) => normalize(pad.axes[index] ?? 0, -(pad.axes[index + 1] ?? 0), 0.2);
        const next = {buttons: FACE_BUTTONS.map((_, index) => !!pad.buttons[index]?.pressed), left: axes(0), right: axes(2)};
        const current = this.device();
        if (!current || !current.buttons.every((pressed, index) => pressed === next.buttons[index]) ||
            !current.left.every((axis, index) => axis === next.left[index]) ||
            !current.right.every((axis, index) => axis === next.right[index])) this.device.set(next);
    }
}

function normalize(x: number, y: number, deadZone: number): Axis {
    const length = Math.hypot(x, y);
    if (!Number.isFinite(length) || length <= deadZone) return zero;
    const strength = (Math.min(length, 1) - deadZone) / (1 - deadZone);
    return [x / length * strength, y / length * strength];
}

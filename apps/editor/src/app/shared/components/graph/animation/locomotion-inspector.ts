import {Component, computed, effect, input, linkedSignal, output, signal} from '@angular/core';
import {disabled, form, FormField, hidden, min} from '@angular/forms/signals';
import {EditorSubject} from '../../subject/subject';
import {EditorRow} from '../../row/row';
import {validGraphValues} from '../graph.models';
import {
    AnimationClipAsset,
    LocomotionValues,
    locomotionWeights,
    MOTION_SLOTS,
    MotionSlot,
    slotVisible,
} from './animation.models';

@Component({
    selector: 'editor-locomotion-inspector',
    imports: [EditorSubject, EditorRow, FormField],
    templateUrl: './locomotion-inspector.html',
    styleUrl: './animation-inspector.scss',
})
export class LocomotionInspector {
    readonly values = input.required<LocomotionValues>();
    readonly nodeId = input.required<string>();
    readonly assets = input<readonly AnimationClipAsset[]>([]);
    readonly connected = input.required<ReadonlySet<string>>();
    readonly changed = output<LocomotionValues>();
    readonly openMachine = output<void>();
    protected readonly draft = linkedSignal(() => this.values());
    protected readonly fields = form(this.draft, (path) => {
        hidden(path.blend.walkSpeed, {when: () => this.draft().blend.mode === 'single'});
        hidden(path.blend.runSpeed, {when: () => this.draft().blend.mode === 'single'});
        min(path.blend.walkSpeed, 0.01);
        min(path.blend.runSpeed, 0.02);
        for (const slot of MOTION_SLOTS) {
            disabled(path.motions[slot.id], {when: () => this.wired(slot.id)});
            disabled(path.exposed[slot.id], {when: () => this.wired(slot.id)});
        }
        disabled(path.blend.mode, {
            when: () =>
                MOTION_SLOTS.some((slot) => slot.id !== 'idle' && !slot.air && this.wired(slot.id)),
        });
        disabled(path.air.enabled, {
            when: () =>
                this.connected().has(`${this.nodeId()}:input:jump`) ||
                MOTION_SLOTS.some((slot) => slot.air && this.wired(slot.id)),
        });
        disabled(path.rootMotion, {
            when: () => this.connected().has(`${this.nodeId()}:output:rootMotion`),
        });
        disabled(path.emitEvents, {
            when: () => this.connected().has(`${this.nodeId()}:output:events`),
        });
    });
    protected readonly slots = computed(() =>
        MOTION_SLOTS.filter((slot) => slotVisible(this.draft(), slot)),
    );
    protected readonly velocity = signal({x: 0, forward: 2});
    protected readonly previewForm = form(this.velocity);
    protected readonly weights = computed(() =>
        Object.entries(
            locomotionWeights(this.values(), this.velocity().x, this.velocity().forward),
        ).map(([id, weight]) => ({
            id,
            label: MOTION_SLOTS.find((slot) => slot.id === id)?.label ?? id,
            percent: Math.round(weight * 100),
        })),
    );
    protected readonly valid = computed(() => validGraphValues('animation.locomotion', this.draft()));
    protected readonly blendOptions = {
        mode: [
            {value: 'single', label: 'Single motion'},
            {value: 'speed1d', label: '1D · Speed'},
            {value: 'direction2d', label: '2D · Direction + speed'},
        ],
    };
    protected readonly point = computed(() => ({
        x: Math.max(5, Math.min(95, 50 + (this.velocity().x / this.values().blend.runSpeed) * 45)),
        y: Math.max(
            5,
            Math.min(95, 50 - (this.velocity().forward / this.values().blend.runSpeed) * 45),
        ),
    }));
    protected readonly warnings = computed(() =>
        this.slots().flatMap((slot) => {
            if (this.wired(slot.id)) return [];
            const id = this.draft().motions[slot.id];
            if (!id) return [`${slot.label}: no clip assigned.`];
            const asset = this.assets().find((asset) => asset.id === id);
            if (!asset) return [`${slot.label}: clip is missing from the asset catalog.`];
            return this.draft().skeletonId &&
            asset.skeletonId &&
            asset.skeletonId !== this.draft().skeletonId
                ? [`${slot.label}: skeleton does not match.`]
                : [];
        }),
    );

    constructor() {
        effect(() => {
            const value = this.draft();
            if (value !== this.values() && this.valid()) this.changed.emit(value);
        });
    }

    protected wired(slot: MotionSlot): boolean {
        return this.connected().has(`${this.nodeId()}:input:motion.${slot}`);
    }

    protected missing(id: string): boolean {
        return !!id && !this.assets().some((asset) => asset.id === id);
    }

    protected previewClick(event: MouseEvent): void {
        if (event.detail === 0) {
            this.velocity.set({x: 0, forward: 0});
            return;
        }
        const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
        const extent = this.values().blend.runSpeed;
        this.velocity.set({
            x: Math.max(
                -extent,
                Math.min(
                    extent,
                    ((event.clientX - rect.left - rect.width / 2) / (rect.width * 0.45)) * extent,
                ),
            ),
            forward: Math.max(
                -extent,
                Math.min(
                    extent,
                    ((rect.height / 2 - event.clientY + rect.top) / (rect.height * 0.45)) * extent,
                ),
            ),
        });
    }
}

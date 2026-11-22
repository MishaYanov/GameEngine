import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import type {FeedbackBusMix, FeedbackCueDraft} from '../../../../core/editor/feedback-audio.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {AudioCueSettings} from './audio-cue-settings';

const emptyMix: FeedbackBusMix = {bus_percent: {}, scene_bus_percent: {}};
const catalog: FeedbackCueDraft = {
    mix: emptyMix,
    workspace_generation: 12,
    catalog_digest: 'a'.repeat(64),
    cues: [{event: 'gate_opened', path: 'audio/cue-gate.wav'}],
};

describe(AudioCueSettings, () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            },
        });
    });

    afterAll(() => {
        if (originalShow) Object.defineProperty(prototype, 'showModal', originalShow);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (originalClose) Object.defineProperty(prototype, 'close', originalClose);
        else Reflect.deleteProperty(prototype, 'close');
    });

    async function setup(initial: FeedbackCueDraft = catalog) {
        const generation = signal(1);
        const snapshot = signal({
            mode: 'edit',
            transaction: null,
            project: {gameplay_module: 'courtyard_run'},
            active_scene_id: 'courtyard',
            scenes: [{id: 'courtyard'}, {id: 'garden'}],
        });
        const feedbackCues = vi.fn(async () => initial);
        const importFeedbackCue = vi.fn(async () => 'audio/cue-key.wav');
        const applyFeedbackCues = vi.fn(async () => catalog);
        const previewFeedbackCue = vi.fn();
        TestBed.configureTestingModule({
            providers: [
                {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
                {
                    provide: EditorService, useValue: {
                        nativeAvailable: true, snapshot, workspaceGeneration: generation, busy: signal(false),
                        feedbackCues, importFeedbackCue, applyFeedbackCues, previewFeedbackCue,
                    }
                },
            ]
        });
        const fixture = TestBed.createComponent(AudioCueSettings);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;

        function button(label: string): HTMLButtonElement {
            const item = [...root.querySelectorAll('button')].find(element =>
                element.textContent.trim() === label || element.getAttribute('aria-label') === label);
            if (!item) throw new Error('Missing button ' + label);
            return item;
        }

        function field(label: string): HTMLInputElement {
            const row = [...root.querySelectorAll('label')].find(item => item.textContent.trim() === label);
            const input = row?.querySelector('input');
            if (!input) throw new Error('Missing field ' + label);
            return input;
        }

        async function change(label: string, value: string) {
            const element = field(label);
            element.value = value;
            element.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        }

        async function selectScene(id: string) {
            const picker = [...root.querySelectorAll('select')].find(select =>
                select.closest('label')?.textContent?.trim().startsWith('Preview and edit scene'));
            if (!picker) throw new Error('Missing scene selector');
            picker.value = id;
            picker.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        }

        button('Feedback Audio…').click();
        await fixture.whenStable();
        return {
            root, fixture, generation, snapshot, feedbackCues, importFeedbackCue, applyFeedbackCues,
            previewFeedbackCue, button, field, change, selectScene
        };
    }

    it('discards edits on Close and performs no implicit playback or publication', async () => {
        const ui = await setup();
        await ui.change('Gate opened', 'audio/cue-draft.wav');
        expect(ui.button('Apply Feedback Audio').disabled).toBe(false);
        ui.button('Close').click();
        await ui.fixture.whenStable();
        expect(ui.root.querySelector('dialog[open]')).toBeNull();
        expect(ui.applyFeedbackCues).not.toHaveBeenCalled();
        expect(ui.previewFeedbackCue).not.toHaveBeenCalled();
        ui.button('Feedback Audio…').click();
        await ui.fixture.whenStable();
        expect(ui.field('Gate opened').value).toBe('audio/cue-gate.wav');
    });

    it('separates file import from optimistic catalog publication', async () => {
        const ui = await setup();
        await ui.change('Clip filename', 'key');
        ui.button('Choose WAV…').click();
        await ui.fixture.whenStable();
        expect(ui.importFeedbackCue).toHaveBeenCalledWith(12, 'key', 1);
        expect(ui.field('Gate opened').value).toBe('audio/cue-key.wav');
        expect(ui.applyFeedbackCues).not.toHaveBeenCalled();
        ui.button('Apply Feedback Audio').click();
        await ui.fixture.whenStable();
        expect(ui.applyFeedbackCues).toHaveBeenCalledWith(12, 'a'.repeat(64), [
            {event: 'gate_opened', path: 'audio/cue-key.wav'},
        ], 1, emptyMix);
    });

    it('lets the user explicitly save a silent catalog when the project uses the built-in tone', async () => {
        const ui = await setup({workspace_generation: 12, catalog_digest: null, cues: [], mix: emptyMix});
        ui.applyFeedbackCues.mockResolvedValueOnce({
            workspace_generation: 12,
            catalog_digest: 'b'.repeat(64),
            cues: [],
            mix: emptyMix,
        });
        expect(ui.root.textContent).toContain('Apply with all rows empty to turn it off.');
        expect(ui.button('Apply Feedback Audio').disabled).toBe(false);
        ui.button('Apply Feedback Audio').click();
        await ui.fixture.whenStable();
        expect(ui.applyFeedbackCues).toHaveBeenCalledExactlyOnceWith(12, null, [], 1, emptyMix);
        expect(ui.button('Apply Feedback Audio').disabled).toBe(true);
    });

    it('silences a pending audition when its assignment changes', async () => {
        const ui = await setup();
        let respond!: (bytes: unknown) => void;
        const response = new Promise<unknown>(resolve => {
            respond = resolve;
        });
        const decode = vi.fn();
        vi.stubGlobal('AudioContext', class {
            resume = vi.fn(async () => {
            });
            close = vi.fn(async () => {
            });
            decodeAudioData = decode;
        });
        try {
            ui.previewFeedbackCue.mockReturnValueOnce(response);
            ui.button('Preview Gate opened').click();
            await vi.waitFor(() => expect(ui.previewFeedbackCue).toHaveBeenCalledOnce());
            await ui.change('Gate opened', 'audio/cue-alternate.wav');
            respond(Array.from({length: 46}, () => 0));
            await ui.fixture.whenStable();
            expect(ui.button('Preview Gate opened')).toBeDefined();
            expect(decode).not.toHaveBeenCalled();
        } finally {
            ui.fixture.destroy();
            vi.unstubAllGlobals();
        }
    });

    it('cancels a pending decode when preview volume changes and keeps the catalog draft clean', async () => {
        const ui = await setup();
        let finish!: (buffer: AudioBuffer) => void;
        const decoded = new Promise<AudioBuffer>(resolve => {
            finish = resolve;
        });
        const decode = vi.fn(() => decoded);
        const createSource = vi.fn();
        vi.stubGlobal('AudioContext', class {
            resume = vi.fn(async () => {
            });
            close = vi.fn(async () => {
            });
            decodeAudioData = decode;
            createBufferSource = createSource;
        });
        try {
            ui.previewFeedbackCue.mockResolvedValueOnce(Array.from({length: 46}, () => 0));
            ui.button('Preview Gate opened').click();
            await vi.waitFor(() => expect(decode).toHaveBeenCalledOnce());
            await ui.change('Preview volume', '50');
            finish({} as AudioBuffer);
            await ui.fixture.whenStable();
            expect(ui.field('Preview volume').getAttribute('aria-valuetext')).toBe('50%');
            expect(createSource).not.toHaveBeenCalled();
            expect(ui.button('Apply Feedback Audio').disabled).toBe(true);

            ui.field('Mute preview').click();
            await ui.fixture.whenStable();
            ui.button('Preview Gate opened').click();
            await ui.fixture.whenStable();
            expect(ui.root.textContent).toContain('Preview skipped at zero volume.');
            expect(ui.previewFeedbackCue).toHaveBeenCalledOnce();
            expect(ui.applyFeedbackCues).not.toHaveBeenCalled();

            ui.button('Close').click();
            await ui.fixture.whenStable();
            ui.button('Feedback Audio…').click();
            await ui.fixture.whenStable();
            expect(ui.field('Preview volume').value).toBe('100');
            expect(ui.field('Mute preview').checked).toBe(false);
        } finally {
            ui.fixture.destroy();
            vi.unstubAllGlobals();
        }
    });

    it('edits campaign and scene percentages with one reviewed Apply', async () => {
        const ui = await setup();
        expect(ui.root.textContent).toContain('Gate opened authored level 100%');
        await ui.change('Campaign mechanisms level (%)', '80');
        await ui.selectScene('garden');
        await ui.change('Scene mechanisms level (%)', '50');
        expect(ui.root.textContent).toContain('Gate opened authored level 40%');
        await ui.selectScene('courtyard');
        expect(ui.root.textContent).toContain('Gate opened authored level 80%');
        await ui.selectScene('garden');
        expect(ui.field('Scene mechanisms level (%)').value).toBe('50');

        await ui.change('Scene mechanisms level (%)', '101');
        expect(ui.button('Apply Feedback Audio').disabled).toBe(true);
        await ui.change('Scene mechanisms level (%)', '50.5');
        const busField = ui.field('Scene mechanisms level (%)');
        expect(busField.getAttribute('aria-invalid')).toBe('true');
        expect(ui.root.querySelector('#' + busField.getAttribute('aria-describedby'))?.textContent)
            .toContain('Enter whole feedback levels');
        expect(ui.button('Apply Feedback Audio').disabled).toBe(true);
        await ui.change('Scene mechanisms level (%)', '50');
        expect(busField.hasAttribute('aria-invalid')).toBe(false);
        ui.button('Apply Feedback Audio').click();
        await ui.fixture.whenStable();
        expect(ui.applyFeedbackCues).toHaveBeenCalledExactlyOnceWith(12, 'a'.repeat(64), catalog.cues, 1, {
            bus_percent: {mechanisms: 80},
            scene_bus_percent: {garden: {mechanisms: 50}},
        });
    });

    it('cancels pending decode when the scene changes without making an authored edit', async () => {
        const ui = await setup({
            ...catalog,
            mix: {bus_percent: {mechanisms: 80}, scene_bus_percent: {garden: {mechanisms: 60}}},
        });
        let finish!: (buffer: AudioBuffer) => void;
        const decoded = new Promise<AudioBuffer>(resolve => {
            finish = resolve;
        });
        const decode = vi.fn(() => decoded);
        const createSource = vi.fn();
        vi.stubGlobal('AudioContext', class {
            resume = vi.fn(async () => undefined);
            close = vi.fn(async () => undefined);
            decodeAudioData = decode;
            createBufferSource = createSource;
        });
        try {
            ui.previewFeedbackCue.mockResolvedValueOnce(Array.from({length: 46}, () => 0));
            ui.button('Preview Gate opened').click();
            await vi.waitFor(() => expect(decode).toHaveBeenCalledOnce());
            await ui.selectScene('garden');
            finish({} as AudioBuffer);
            await ui.fixture.whenStable();
            expect(ui.root.textContent).toContain('Gate opened authored level 48%');
            expect(createSource).not.toHaveBeenCalled();
            expect(ui.button('Apply Feedback Audio').disabled).toBe(true);
            expect(ui.applyFeedbackCues).not.toHaveBeenCalled();
        } finally {
            ui.fixture.destroy();
            vi.unstubAllGlobals();
        }
    });

    it('validates relative path names, shows accessible text, and closes on a workspace change', async () => {
        const ui = await setup();
        await ui.change('Gate opened', 'audio/../outside.wav');
        expect(ui.button('Preview Gate opened').disabled).toBe(true);
        expect(ui.button('Apply Feedback Audio').disabled).toBe(true);
        const gateField = ui.field('Gate opened');
        expect(gateField.getAttribute('aria-invalid')).toBe('true');
        expect(ui.root.querySelector('#' + gateField.getAttribute('aria-describedby'))?.textContent)
            .toContain('Enter a project path');
        const report = await axe.run(ui.root, {runOnly: {type: 'tag', values: ['wcag2a', 'wcag2aa']}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
        ui.generation.set(2);
        await ui.fixture.whenStable();
        expect(ui.root.querySelector('dialog[open]')).toBeNull();
    });
});

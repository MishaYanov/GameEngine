import {TestBed} from '@angular/core/testing';
import type {AudioAuditionScope} from '../../../../core/editor/audio-source.models';
import type {ComponentRecord, ComponentTypeDescriptor} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import {ComponentEditor, type ComponentEditIntent} from '../components/component-editor';
import {audioClipDragType} from './audio-clip-drag';
import {AudioAuditionService} from './audio-audition.service';

const audio: ComponentTypeDescriptor = {kind: 'audio_source', label: 'Audio Source',
    type_id: 'engine.audio_source', description: 'Audio settings', max_per_object: 1};
const component: ComponentRecord = {id: 'audio', schema_version: 1, enabled: true, data: {kind: 'audio_source',
    settings: {clip: null, gain: 1, loop: false, playback: 'on_scene_enter', bus: 'effects'}}};
const bell = '11111111-1111-4111-8111-111111111111';
const chime = '22222222-2222-4222-8222-222222222222';
const mesh = '33333333-3333-4333-8333-333333333333';

describe('Audio Source', () => {
    it('focuses a missing clip, permits keyboard and local WAV assignment, and rejects stale or unrelated drops', async () => {
        TestBed.configureTestingModule({providers: [{provide: EditorService, useValue: {}}]});
        const fixture = TestBed.createComponent(ComponentEditor);
        const host: HTMLElement = fixture.nativeElement;
        const edits: ComponentEditIntent[] = [];
        fixture.componentInstance.actionRequested.subscribe(intent => edits.push(intent));
        fixture.componentRef.setInput('types', [audio]);
        fixture.componentRef.setInput('components', [component]);
        fixture.componentRef.setInput('editable', true);
        fixture.componentRef.setInput('assetGeneration', 9);
        fixture.componentRef.setInput('models', [
            {name: 'Bell', path: 'bell.wav', kind: 'audio_clip', asset_id: bell},
            {name: 'Chime', path: 'chime.wav', kind: 'audio_clip', asset_id: chime},
            {name: 'Mesh', path: 'bell.glb', kind: 'model', asset_id: mesh},
        ]);
        await fixture.whenStable();
        const button = (label: string) => [...host.querySelectorAll('button')].find(button => button.textContent.trim() === label)!;
        expect(host.querySelector('[role="status"][tabindex="0"]')?.textContent).toContain('No Audio Clip assigned');
        button('Assign Audio Clip…').click();
        await fixture.whenStable();
        const field = host.querySelector<HTMLInputElement>('[data-component-reference-field="Audio Clip"] input')!;
        expect(document.activeElement).toBe(field);
        field.value = 'Bell';
        field.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        field.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
        await fixture.whenStable();
        expect(field.value).toBe('Bell');
        const drop = async (value: string) => {
            const event = new Event('drop', {bubbles: true, cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {types: [audioClipDragType], getData: () => value}});
            field.dispatchEvent(event);
            await fixture.whenStable();
        };
        await drop('9:' + mesh);
        await drop('8:' + chime);
        expect(field.value).toBe('Bell');
        await drop('9:' + chime);
        expect(field.value).toBe('Chime');
        button('Apply Component').click();
        await fixture.whenStable();
        expect(edits[0]).toMatchObject({kind: 'replace', component: {id: 'audio', data: {settings: {clip: {asset: chime}}}}});
    });

    it('releases only the old audition when its native read finishes late', async () => {
        const wave = Array.from({length: 64_044}, () => 0);
        const source = {start: vi.fn(), stop: vi.fn(), disconnect: vi.fn(), connect: vi.fn(), onended: null};
        const device = {resume: vi.fn(async () => undefined), close: vi.fn(async () => undefined),
            decodeAudioData: vi.fn(async (_wave: ArrayBuffer) => ({})), createBufferSource: () => source,
            createGain: () => ({gain: {value: 1}, connect: vi.fn(), disconnect: vi.fn()}), destination: {}};
        vi.stubGlobal('AudioContext', function () { return device; });
        try {
            let finishFirst: (value: unknown) => void = () => undefined;
            const earlier = new Promise(resolve => finishFirst = resolve);
            const auditionAudioSource = vi.fn(async (): Promise<unknown> => ({token: chime, bytes: wave, gain: 0.4}));
            auditionAudioSource.mockReturnValueOnce(earlier);
            const stopAudioSourceAudition = vi.fn(async () => undefined);
            TestBed.configureTestingModule({providers: [{provide: EditorService, useValue: {auditionAudioSource, stopAudioSourceAudition}}]});
            const audition = TestBed.inject(AudioAuditionService);
            const old = Symbol('old'), next = Symbol('next');
            const scope: AudioAuditionScope = {owner: {kind: 'scene', key: 'bell', scene_id: 'main'},
                generation: 1, nativeGeneration: 2, revision: 3, contentRevision: 4};
            const first = audition.play(old, scope, 'audio', 0.4, () => true);
            await Promise.resolve();
            await audition.play(next, scope, 'audio', 0.4, () => true);
            finishFirst({token: bell, bytes: wave, gain: 0.4});
            await first;
            expect(source.start).toHaveBeenCalledOnce();
            expect(stopAudioSourceAudition).toHaveBeenCalledExactlyOnceWith(bell);
            expect(audition.isPlaying(next)).toBe(true);
            audition.stop(old);
            expect(source.stop).not.toHaveBeenCalled();
            audition.stop(next);
            expect(source.stop).toHaveBeenCalledOnce();
            expect(stopAudioSourceAudition).toHaveBeenLastCalledWith(chime);
        } finally {
            vi.unstubAllGlobals();
        }
    });
});

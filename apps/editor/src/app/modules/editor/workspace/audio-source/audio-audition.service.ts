import {DestroyRef, inject, Injectable, signal} from '@angular/core';
import {maxAudioAuditionWaveSize, readAudioAuditionReply, type AudioAuditionScope} from '../../../../core/editor/audio-source.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {CuePreview} from '../game-settings/cue-preview';

interface Audition {
    readonly owner: symbol;
    token: string | null;
}

@Injectable({providedIn: 'root'})
export class AudioAuditionService {
    private readonly editor = inject(EditorService);
    private readonly lane = new CuePreview(undefined, maxAudioAuditionWaveSize);
    private readonly playing = signal<Audition | null>(null);
    private nextRequest = 0;

    constructor() {
        inject(DestroyRef).onDestroy(() => {
            this.stop();
            this.lane.close();
        });
    }

    isPlaying(owner: symbol): boolean {
        return this.playing()?.owner === owner;
    }

    async play(owner: symbol, scope: AudioAuditionScope, componentId: string, gain: number, active: () => boolean): Promise<void> {
        this.stop();
        const session: Audition = {owner, token: null};
        this.playing.set(session);
        const requestId = ++this.nextRequest;
        const current = () => this.playing() === session && active();
        try {
            await this.lane.play(async () => {
                const reply = readAudioAuditionReply(await this.editor.auditionAudioSource(scope, componentId, requestId));
                if (!current()) {
                    this.release(reply.token);
                    return null;
                }
                session.token = reply.token;
                if (Math.fround(reply.gain) !== Math.fround(gain)) throw new Error('The Audio Source gain changed before previewing.');
                return reply.bytes;
            }, {levelPercent: gain * 100, muted: false}, current, () => this.stop(owner));
        } catch (error: unknown) {
            if (!current()) return;
            this.stop(owner);
            throw error;
        }
    }

    stop(owner?: symbol): void {
        const session = this.playing();
        if (!session || (owner !== undefined && session.owner !== owner)) return;
        this.playing.set(null);
        this.lane.stop();
        if (session.token) this.release(session.token);
    }

    private release(token: string): void {
        void this.editor.stopAudioSourceAudition(token).catch(() => undefined);
    }
}

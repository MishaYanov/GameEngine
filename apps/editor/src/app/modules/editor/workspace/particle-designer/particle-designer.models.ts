import type {ParticleEmitterSettings} from '../../../../core/editor/editor.models';
import {
    PARTICLE_NUMBER_FIELDS,
    type ParticleNumberKey,
    validParticleEmitterSettings,
} from '../../../../core/editor/particle-emitter.models';

export type ParticleDraft = Record<ParticleNumberKey | 'color', string>;
export const PARTICLE_FIELDS = PARTICLE_NUMBER_FIELDS;

export const PARTICLE_PRESETS = [
    {
        label: 'Embers', settings: {
            rate_per_second: 26, lifetime_seconds: 2.2, speed: 1.1, spread_degrees: 24,
            gravity: 1, size: .14, color: [1, .35, .08], seed: 851,
        }
    },
    {
        label: 'Fountain', settings: {
            rate_per_second: 60, lifetime_seconds: 1.8, speed: 7, spread_degrees: 32,
            gravity: -9.8, size: .17, color: [.18, .75, 1], seed: 502,
        }
    },
    {
        label: 'Sparks', settings: {
            rate_per_second: 96, lifetime_seconds: .65, speed: 5.5, spread_degrees: 75,
            gravity: -5, size: .1, color: [1, .78, .19], seed: 29,
        }
    },
] as const satisfies readonly { label: string; settings: ParticleEmitterSettings }[];

export function particlePresetLabel(settings: ParticleEmitterSettings): string {
    return PARTICLE_PRESETS.find(({settings: preset}) =>
        PARTICLE_FIELDS.every(({key}) => key === 'seed' ? settings[key] === preset[key]
            : Math.fround(settings[key]) === Math.fround(preset[key])) &&
        settings.color.every((channel, index) => Math.fround(channel) === Math.fround(preset.color[index])),
    )?.label ?? '';
}

function colorToHex(color: ParticleEmitterSettings['color']): string {
    return '#' + color.map(channel => Math.round(channel * 255).toString(16).padStart(2, '0')).join('');
}

export function draftFromSettings(settings: ParticleEmitterSettings): ParticleDraft {
    return {
        rate_per_second: String(settings.rate_per_second),
        lifetime_seconds: String(settings.lifetime_seconds),
        speed: String(settings.speed),
        spread_degrees: String(settings.spread_degrees),
        gravity: String(settings.gravity),
        size: String(settings.size),
        seed: String(settings.seed),
        color: colorToHex(settings.color),
    };
}

export function draftFieldError(draft: ParticleDraft, field: typeof PARTICLE_FIELDS[number]): string | null {
    const value = Number(draft[field.key]);
    const whole = field.key === 'seed';
    const compared = whole ? value : Math.fround(value);
    const minimum = whole ? field.min : Math.fround(field.min);
    const maximum = whole ? field.max : Math.fround(field.max);
    if (!draft[field.key].trim() || !Number.isFinite(compared) || compared < minimum ||
        compared > maximum || (whole && !Number.isInteger(value))) {
        return `Enter ${whole ? 'a whole number' : 'a number'} from ${field.min} to ${field.max}.`;
    }
    return null;
}

export function settingsFromDraft(
    draft: ParticleDraft,
    exactColor: ParticleEmitterSettings['color'],
): ParticleEmitterSettings | null {
    if (PARTICLE_FIELDS.some(field => draftFieldError(draft, field)) || !/^#[a-f0-9]{6}$/i.test(draft.color)) return null;
    const color = draft.color.toLowerCase();
    const channel = (index: number) => Number.parseInt(color.slice(index, index + 2), 16) / 255;
    const settings: ParticleEmitterSettings = {
        rate_per_second: Number(draft.rate_per_second),
        lifetime_seconds: Number(draft.lifetime_seconds),
        speed: Number(draft.speed),
        spread_degrees: Number(draft.spread_degrees),
        gravity: Number(draft.gravity),
        size: Number(draft.size),
        seed: Number(draft.seed),
        color: color === colorToHex(exactColor) ? exactColor : [channel(1), channel(3), channel(5)],
    };
    return validParticleEmitterSettings(settings) ? settings : null;
}

export function sameParticleSettings(first: ParticleEmitterSettings, second: ParticleEmitterSettings): boolean {
    return PARTICLE_FIELDS.every(({key}) => first[key] === second[key]) &&
        first.color.every((channel, index) => channel === second.color[index]);
}

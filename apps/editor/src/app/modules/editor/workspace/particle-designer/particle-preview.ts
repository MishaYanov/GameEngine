import type {ParticleEmitterSettings} from '../../../../core/editor/editor.models';

interface PreviewParticle {
    readonly born: number;
    readonly velocity: readonly [number, number, number];
}

const PREVIEW_HZ = 60;
const PREVIEW_STEP = 1 / PREVIEW_HZ;
const MAX_PREVIEW_PARTICLES = 192;
const FULL_TURN = 2 * Math.PI;

export class ParticlePreview {
    private readonly particles: PreviewParticle[] = [];
    private readonly minimumConeCosine: number;
    private seed: number;
    private tick = 0;
    private birthCredit = 0;
    private timeCredit = 0;

    constructor(readonly settings: ParticleEmitterSettings) {
        this.minimumConeCosine = Math.cos(settings.spread_degrees * Math.PI / 180);
        this.seed = settings.seed;
    }

    advance(seconds: number): void {
        this.timeCredit += Math.min(seconds, .1);
        while (this.timeCredit >= PREVIEW_STEP) {
            this.step();
            this.timeCredit -= PREVIEW_STEP;
        }
    }

    * positions(): Iterable<{ x: number; y: number; z: number; size: number }> {
        for (const particle of this.particles) {
            const age = (this.tick - particle.born) / PREVIEW_HZ;
            const [x, y, z] = particle.velocity;
            yield {
                x: x * age,
                y: y * age + this.settings.gravity * age * age / 2,
                z: z * age,
                size: this.settings.size * (1 - .75 * age / this.settings.lifetime_seconds),
            };
        }
    }

    private step(): void {
        this.tick++;
        const firstActive = this.particles.findIndex(particle =>
            (this.tick - particle.born) / PREVIEW_HZ < this.settings.lifetime_seconds);
        this.particles.splice(0, firstActive === -1 ? this.particles.length : firstActive);
        this.birthCredit += this.settings.rate_per_second;
        const births = Math.floor(this.birthCredit / PREVIEW_HZ);
        this.birthCredit -= births * PREVIEW_HZ;
        const available = Math.min(births, MAX_PREVIEW_PARTICLES - this.particles.length);
        for (let index = 0; index < available; index++) {
            const azimuth = this.random() * FULL_TURN;
            const cosine = 1 - this.random() * (1 - this.minimumConeCosine);
            const radius = Math.sqrt(Math.max(0, 1 - cosine * cosine));
            const speed = this.settings.speed;
            this.particles.push({
                born: this.tick,
                velocity: [Math.cos(azimuth) * radius * speed, cosine * speed, Math.sin(azimuth) * radius * speed],
            });
        }
    }

    private random(): number {
        this.seed = (Math.imul(this.seed, 1_664_525) + 1_013_904_223) >>> 0;
        return (this.seed >>> 8) / 16_777_216;
    }
}

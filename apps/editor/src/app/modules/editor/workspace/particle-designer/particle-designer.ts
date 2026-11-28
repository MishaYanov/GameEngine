import {Component, inject, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {draftFieldError, PARTICLE_FIELDS, PARTICLE_PRESETS} from './particle-designer.models';
import {ParticleDesignerSession} from './particle-designer-session';
import {ParticlePreviewCanvas} from './particle-preview-canvas';
import {EditorParticleSpritePicker} from './particle-sprite-picker';

@Component({
    selector: 'editor-particle-designer',
    imports: [EditorDialog, FormField, ParticlePreviewCanvas, EditorParticleSpritePicker],
    templateUrl: './particle-designer.html',
    styleUrl: './particle-designer.scss',
})
export class ParticleDesigner {
    protected readonly session = inject(ParticleDesignerSession);
    protected readonly fields = form(this.session.draft);
    protected readonly numberFields = PARTICLE_FIELDS;
    protected readonly presets = PARTICLE_PRESETS;
    protected readonly dialogConfig = {width: '58rem'};
    protected readonly running = signal(true);
    protected readonly restart = signal(0);

    protected setDialogOpen(open: boolean): void {
        if (!open) this.session.discardAndClose();
    }

    protected didClose(): void {
        this.running.set(true);
        this.restart.set(0);
    }

    protected togglePreview(): void {
        this.running.update(running => !running);
    }

    protected restartPreview(): void {
        this.restart.update(token => token + 1);
    }

    protected fieldError(field: typeof PARTICLE_FIELDS[number]): string | null {
        return draftFieldError(this.session.draft(), field);
    }
}

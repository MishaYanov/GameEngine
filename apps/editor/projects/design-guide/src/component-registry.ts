import type {Type} from '@angular/core';

export interface ComponentPreview {
    readonly component?: Type<unknown>;
    readonly inputs?: Record<string, unknown>;
    readonly requiredInputs?: readonly string[];
}

export interface ComponentSection {
    readonly id: string;
    readonly title: string;
    readonly requiredInputs: readonly string[];
    readonly load?: () => Promise<ComponentPreview>;
}

import {computed, Service, signal} from '@angular/core';
import type {EditorIconName} from '../icons/editor-icon';

export interface SubjectEntry {
    readonly element: HTMLElement;
    readonly title: () => string;
    readonly icon: () => EditorIconName | undefined;
    readonly hidden: () => boolean;
    readonly focus: () => void;
}

@Service({autoProvided: false})
export class SubjectRegistry {
    private readonly entries = signal<readonly SubjectEntry[]>([]);
    readonly subjects = computed(() => this.entries().filter((subject) => !subject.hidden()));

    register(subject: SubjectEntry): () => void {
        this.entries.update((entries) => [...entries, subject]);
        return () => this.entries.update((entries) => entries.filter((entry) => entry !== subject));
    }
}

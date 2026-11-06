import {Service} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';

export interface ProjectCreationResult {
    readonly path: string;
    readonly manifestPath: string;
}

/** Creates authoring projects; editor sessions and game exports have separate owners. */
@Service()
export class ProjectService {
    readonly nativeAvailable = typeof window !== 'undefined' && isTauri();

    create(name: string, destination: string): Promise<ProjectCreationResult> {
        if (!this.nativeAvailable) return this.unavailable();
        return invoke<ProjectCreationResult>('create_project', {
            name: name.trim(),
            destination: destination.trim(),
        });
    }

    chooseDestination(initialDirectory: string | null = null): Promise<string | null> {
        if (!this.nativeAvailable) return this.unavailable();
        return invoke<string | null>('choose_project_destination', {
            initialDirectory: initialDirectory?.trim() || null,
        });
    }

    private unavailable<T>(): Promise<T> {
        return Promise.reject(new Error('Open the desktop editor to create projects.'));
    }
}

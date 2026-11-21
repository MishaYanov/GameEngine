import type {EditorIconName} from '../icons/editor-icon';

/** A reusable intent whose availability and execution are scoped to the clicked item. */
export interface ContextAction<T> {
    readonly id: string;
    readonly label: string;
    readonly icon?: EditorIconName;
    readonly shortcut?: string;
    readonly destructive?: boolean;
    readonly visible?: (context: T) => boolean;
    readonly enabled?: (context: T) => boolean;
    readonly execute: (context: T) => void | Promise<unknown>;
}

export class ContextActionBuilder<T> {
    private options: Omit<ContextAction<T>, 'execute'>;
    private execute?: ContextAction<T>['execute'];

    private constructor(id: string, label: string) {
        if (!id.trim()) throw new Error('A context action requires an id.');
        if (!label.trim()) throw new Error('A context action requires a label.');
        this.options = {id, label};
    }

    static create<T>(id: string, label: string): ContextActionBuilder<T> {
        return new ContextActionBuilder<T>(id, label);
    }

    icon(icon: EditorIconName): this {
        this.options = {...this.options, icon};
        return this;
    }

    shortcut(shortcut: string): this {
        this.options = {...this.options, shortcut};
        return this;
    }

    destructive(destructive = true): this {
        this.options = {...this.options, destructive};
        return this;
    }

    when(visible: (context: T) => boolean): this {
        this.options = {...this.options, visible};
        return this;
    }

    enabledWhen(enabled: (context: T) => boolean): this {
        this.options = {...this.options, enabled};
        return this;
    }

    run(execute: ContextAction<T>['execute']): this {
        this.execute = execute;
        return this;
    }

    build(): ContextAction<T> {
        if (!this.execute) throw new Error(`Context action "${this.options.id}" requires a run callback.`);
        return Object.freeze({...this.options, execute: this.execute});
    }
}

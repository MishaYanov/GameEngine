import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {EditorService} from '../../../core/editor/editor.service';
import {WorkspaceNavigation, type WorkspaceLocation} from './workspace-navigation';

const root: WorkspaceLocation = {kind: 'folder', path: ''};
const actors: WorkspaceLocation = {kind: 'folder', path: 'actors'};
const architecture: WorkspaceLocation = {kind: 'folder', path: 'architecture'};
const scene: WorkspaceLocation = {kind: 'view', view: 'scene-viewport', origin: 'scene'};
const animation: WorkspaceLocation = {kind: 'view', view: 'animation-workspace', origin: 'scene'};

function setup() {
    const workspaceGeneration = signal(1);
    TestBed.configureTestingModule({
        providers: [
            WorkspaceNavigation, {provide: EditorService, useValue: {workspaceGeneration}},
        ]
    });
    return {history: TestBed.inject(WorkspaceNavigation), workspaceGeneration};
}

describe('Workspace navigation', () => {
    it('orders folder visits and view changes together, including an explicit return', async () => {
        const {history} = setup();
        const destinations: WorkspaceLocation[] = [];
        const apply = async (target: WorkspaceLocation) => {
            destinations.push(target);
            return 'applied' as const;
        };
        history.record(root, actors);
        history.record(scene, animation);
        history.record(animation, scene);

        await history.step('back', apply);
        await history.step('back', apply);
        await history.step('back', apply);
        expect(destinations).toEqual([animation, scene, root]);
        expect(history.canGoBack()).toBe(false);
        await history.step('forward', apply);
        expect(destinations.at(-1)).toEqual(actors);
    });

    it('aligns the Assets Back and Forward controls with the adjacent global history entry', async () => {
        const {history} = setup();
        history.record(root, actors);
        history.record(actors, architecture);
        history.record(architecture, actors, 'back');
        expect(history.canGoBack()).toBe(true);
        expect(history.canGoForward()).toBe(true);

        history.record(actors, architecture, 'forward');
        expect(history.canGoForward()).toBe(false);
        await history.step('back', async target => {
            expect(target).toEqual(actors);
            return 'applied';
        });
        history.record(actors, root);
        expect(history.canGoForward()).toBe(false);
    });

    it('keeps a cancelled destination and does not transfer history to another project', async () => {
        const {history, workspaceGeneration} = setup();
        history.record(root, actors);
        await history.step('back', async () => 'cancelled');
        expect(history.canGoBack()).toBe(true);
        expect(history.canGoForward()).toBe(false);

        const pending = history.step('back', async () => {
            workspaceGeneration.set(2);
            return 'applied';
        });
        await pending;
        expect(history.canGoBack()).toBe(false);
        expect(history.canGoForward()).toBe(false);
    });

    it('continues past a folder or view that is no longer available', async () => {
        const {history} = setup();
        history.record(root, actors);
        history.record(actors, architecture);
        const visits: WorkspaceLocation[] = [];
        await history.step('back', async target => {
            visits.push(target);
            return target === actors ? 'unavailable' : 'applied';
        });
        expect(visits).toEqual([actors, root]);
        expect(history.canGoBack()).toBe(false);
    });
});


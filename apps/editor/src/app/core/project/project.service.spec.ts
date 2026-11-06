import {TestBed} from '@angular/core/testing';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import {ProjectService} from './project.service';

vi.mock('@tauri-apps/api/core', () => ({isTauri: vi.fn(() => true), invoke: vi.fn()}));

describe('ProjectService', () => {
    beforeEach(() => {
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset();
        TestBed.configureTestingModule({});
    });

    it('creates an authoring project with normalized inputs and returns its manifest', async () => {
        const result = {path: 'D:\\Projects\\Demo', manifestPath: 'D:\\Projects\\Demo\\project.json'};
        vi.mocked(invoke).mockResolvedValue(result);
        expect(await TestBed.inject(ProjectService).create(' Demo ', ' D:\\Projects ')).toEqual(result);
        expect(invoke).toHaveBeenCalledExactlyOnceWith('create_project', {name: 'Demo', destination: 'D:\\Projects'});
    });

    it('uses the shared destination picker with optional initial directory', async () => {
        vi.mocked(invoke).mockResolvedValue(null);
        const projects = TestBed.inject(ProjectService);
        await projects.chooseDestination();
        expect(invoke).toHaveBeenLastCalledWith('choose_project_destination', {initialDirectory: null});
        await projects.chooseDestination(' D:\\Exports ');
        expect(invoke).toHaveBeenLastCalledWith('choose_project_destination', {initialDirectory: 'D:\\Exports'});
    });

    it('rejects native operations in browser preview without invoking IPC', async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        const projects = TestBed.inject(ProjectService);
        await expect(projects.create('Demo', 'D:\\Projects')).rejects.toThrow('desktop editor');
        await expect(projects.chooseDestination()).rejects.toThrow('desktop editor');
        expect(invoke).not.toHaveBeenCalled();
    });
});

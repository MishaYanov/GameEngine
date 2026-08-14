import { Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import {EngineStatus} from "./engine.models";

@Injectable({
    providedIn: 'root',
})
export class EngineService {

    public status(): Promise<EngineStatus> {
        return invoke<EngineStatus>('engine_status');
    }

}
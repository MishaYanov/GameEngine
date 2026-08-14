import {
  Component,
  inject,
  signal,
} from '@angular/core';

import { EngineService } from './core/engine/engine.service';
import { EngineStatus } from './core/engine/engine.models';

@Component({
  selector: 'app-root',
  template: `
    <h1>Game Engine</h1>

    @if (status()) {
      <div>
        <div>
          Renderer:
          {{ status()!.renderer }}
        </div>

        <div>
          GPU:
          {{ status()!.gpuName }}
        </div>

        <div>
          Vulkan:
          {{ status()!.vulkanApi }}
        </div>
      </div>
    }
  `,
})
export class App {
  private readonly engine =
      inject(EngineService);

  protected readonly status =
      signal<EngineStatus | null>(null);

  public async ngOnInit(): Promise<void> {
    this.status.set(
        await this.engine.status()
    );
  }
}
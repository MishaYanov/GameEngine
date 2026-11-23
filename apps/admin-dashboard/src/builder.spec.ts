import "@angular/compiler";
import {Injector, runInInjectionContext} from "@angular/core";
import {describe, expect, it, vi} from "vitest";
import {AdminApi} from "./api";
import {Builder} from "./builder";
import type {ProjectPreview} from "./models";

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<T>((accept, fail) => {
        resolve = accept;
        reject = fail;
    });
    return {promise, resolve, reject};
}

describe("Project preview request ordering", () => {
    it("waits for a trusted checkout before requesting a preview", async () => {
        const call = vi.fn();
        const injector = Injector.create({providers: [{provide: AdminApi, useValue: {call}}]});
        try {
            const builder = runInInjectionContext(injector, () => new Builder());
            vi.spyOn(builder, "state").mockReturnValue({workspace: "source", workspace_trusted: false});
            await builder.preview();
            expect(call).not.toHaveBeenCalled();
            expect(builder.busy()).toBe(false);
        } finally {
            injector.destroy();
        }
    });

    it.each([false, true])("discards a stale %s response after the destination is edited", async (failed) => {
        const pending = deferred<ProjectPreview>();
        const call = vi.fn().mockReturnValueOnce(pending.promise);
        const injector = Injector.create({providers: [{provide: AdminApi, useValue: {call}}]});
        try {
            const builder = runInInjectionContext(injector, () => new Builder());
            vi.spyOn(builder, "state").mockReturnValue({workspace: "source", workspace_trusted: true});
            builder.draft.destination = "destination-a";
            const original = {...builder.draft, source: "source"};
            const inFlight = builder.preview();
            expect(builder.busy()).toBe(true);
            expect(call).toHaveBeenCalledWith("preview_project", {input: original});

            builder.draft.destination = "destination-b";
            builder.invalidate();
            const oldPlan: ProjectPreview = {
                id: "old", input: original, files: [], total_bytes: 0,
                excluded: [], template_version: 1,
            };
            if (failed) pending.reject(new Error("Old destination is invalid"));
            else pending.resolve(oldPlan);
            await inFlight;
            expect(builder.plan()).toBeNull();
            expect(builder.error()).toBe("");
            expect(builder.busy()).toBe(false);
            expect(builder.draft.destination).toBe("destination-b");
            await builder.generate();
            expect(call).toHaveBeenCalledTimes(1);

            const currentPlan = {...oldPlan, id: "current", input: {...builder.draft, source: "source"}};
            call.mockResolvedValueOnce(currentPlan);
            await builder.preview();
            expect(builder.plan()).toEqual(currentPlan);
            expect(call).toHaveBeenLastCalledWith("preview_project", {input: currentPlan.input});
        } finally {
            injector.destroy();
        }
    });
});

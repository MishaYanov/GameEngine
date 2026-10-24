import "@angular/compiler";
import {beforeEach, describe, expect, it, vi} from "vitest";
import {AdminApi} from "./api";
import {emptyFilter, type LogPage} from "./models";

const native = vi.hoisted(() => ({ available: false, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
    isTauri: () => native.available,
    invoke: native.invoke,
}));
beforeEach(() => {
    native.available = false;
    native.invoke.mockReset();
    history.replaceState(null, "", "/");
});
describe("Dashboard transport", () => {
    it("does not present sample data as real desktop state", async () => {
        const api = new AdminApi();
        expect(api.preview).toBe(false);
        await expect(api.call("dashboard_snapshot")).rejects.toThrow(
            "Desktop services are unavailable",
        );
    });
    it("requires explicit preview and filters the sample logs", async () => {
        history.replaceState(null, "", "/?preview");
        const api = new AdminApi();
        const page = await api.call<LogPage>("log_page", {
            after: 0,
            filter: {...emptyFilter(), severity: "ERROR"},
        });
        expect(page.entries).toHaveLength(1);
        expect(page.entries[0].entry.level).toBe("ERROR");
    });
    it("disables process controls in preview", async () => {
        history.replaceState(null, "", "/?preview");
        await expect(
            new AdminApi().call("start_application", {id: "game"}),
        ).rejects.toThrow("Desktop actions are disabled");
    });
    it("uses the typed native command transport", async () => {
        native.available = true;
        native.invoke.mockResolvedValue({result: "native"});
        const api = new AdminApi();
        await expect(
            api.call("log_page", {
                after: 4,
                limit: 500,
                filter: emptyFilter(),
            }),
        ).resolves.toEqual({result: "native"});
        expect(native.invoke).toHaveBeenCalledWith("log_page", {
            after: 4,
            limit: 500,
            filter: emptyFilter(),
        });
    });
});

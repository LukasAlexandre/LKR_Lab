import { describe, expect, it } from "vitest";
import { PREFERENCE_SCOPE, preferences } from "../shared/preferences";
import { portablePreferences, run, syncStore } from "./sync";

describe("sync client", () => {
  it("envia só preferências marcadas como portáteis", () => {
    preferences.set({ sidebarCompact: true, density: "compact", promptFavorites: ["audit"], activeProjectId: "p1", portsFilter: "all" });
    const sent = portablePreferences();
    expect(Object.keys(sent).sort()).toEqual(
      Object.entries(PREFERENCE_SCOPE).filter(([, scope]) => scope === "portable").map(([key]) => key).sort(),
    );
    expect(sent).toEqual({ sidebarCompact: true, density: "compact", promptFavorites: ["audit"] });
    expect(JSON.stringify(sent)).not.toContain("p1");
  });
  it("fora do desktop nada roda e o estado fica vazio", async () => {
    await run();
    expect(syncStore.get()).toEqual({ status: null, busy: false, notice: null });
  });
});

import { describe, expect, it } from "vitest";
import { localInteger, localStatus } from "./local";
import type { LocalState } from "../../lib/bridge/types";

describe("local performance settings", () => {
  it("accepts zero and bounded whole numbers and rejects partial or invalid input", () => {
    expect(localInteger("0")).toBe(0);
    expect(localInteger(" 10 ")).toBe(10);
    expect(localInteger("65535")).toBe(65535);
    for (const value of ["", " ", "-1", "2.5", "65536", "1e3", "NaN", "12x"]) expect(localInteger(value),value).toBeNull();
  });
  it("only reports ready after both artifacts exist and running for the selected build", () => {
    const state: LocalState = { builds:[],models:[],installed:[],progress:null,running_model:null,running_build:null,
      recommended:"model",hardware:{ram_bytes:0,vram_mb:0,nvidia:false,physical_cores:1},folder:"" };
    expect(localStatus(null,"model","blas")).toBe("local.not_installed");
    state.installed=["model"];
    expect(localStatus(state,"model","blas")).toBe("local.not_installed");
    state.installed.push("blas");
    expect(localStatus(state,"model","blas")).toBe("local.ready");
    state.running_model="model"; state.running_build="blas";
    expect(localStatus(state,"model","blas")).toBe("local.running");
    expect(localStatus(state,"model","cuda12")).toBe("local.not_installed");
    state.progress={id:"other-model",received:10,total:100};
    expect(localStatus(state,"model","blas")).toBe("local.downloading");
  });
});

import { describe,expect,it } from "vitest";
import { initialWeatherState,weatherReducer } from "../lib/state";
import { snapshot } from "./fixtures";
describe("refresh state transitions",()=>{it("moves through refresh, success, stale, and error",()=>{const refreshing=weatherReducer({...initialWeatherState,snapshot},{type:"refresh"});expect(refreshing.phase).toBe("refreshing");expect(weatherReducer(refreshing,{type:"success",snapshot}).phase).toBe("success");expect(weatherReducer(refreshing,{type:"success",snapshot:{...snapshot,freshness:"stale"}}).phase).toBe("stale");expect(weatherReducer(refreshing,{type:"failure",error:"offline"})).toMatchObject({phase:"error",error:"offline",snapshot})})});

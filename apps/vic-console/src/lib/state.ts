import type { LoadState, WeatherSnapshot } from "../shared/types";
export type WeatherAction = {type:"load"}|{type:"refresh"}|{type:"success";snapshot:WeatherSnapshot}|{type:"failure";error:string};
export const initialWeatherState:LoadState = {phase:"loading",snapshot:null,error:null};
export function weatherReducer(state:LoadState, action:WeatherAction):LoadState {
  if(action.type==="load") return {...state,phase:"loading",error:null};
  if(action.type==="refresh") return {...state,phase:"refreshing",error:null};
  if(action.type==="success") return {phase:action.snapshot.freshness==="stale"?"stale":"success",snapshot:action.snapshot,error:action.snapshot.source_error};
  return {phase:"error",snapshot:state.snapshot,error:action.error};
}

import { useCallback, useEffect, useReducer, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { consoleApi } from "./lib/api";
import { initialWeatherState, weatherReducer } from "./lib/state";
import type { AppConfig, PanelId, TemperatureUnit } from "./shared/types";
import { TitleBar } from "./components/TitleBar";
import { NavRail } from "./components/NavRail";
import { WeatherDashboard } from "./components/WeatherDashboard";
import { PlaceholderPanel } from "./components/PlaceholderPanel";

export default function App(){
  const [panel,setPanel]=useState<PanelId>("weather");const [config,setConfig]=useState<AppConfig|null>(null);const [weather,dispatch]=useReducer(weatherReducer,initialWeatherState);
  const load=useCallback(async(force=false)=>{dispatch({type:force?"refresh":"load"});try{dispatch({type:"success",snapshot:await consoleApi.weather(force)})}catch(error){dispatch({type:"failure",error:String(error)})}},[]);
  useEffect(()=>{void Promise.all([load(),consoleApi.config().then(setConfig),consoleApi.settings().then(settings=>{if(["weather","tasks","news","business"].includes(settings.selected_panel))setPanel(settings.selected_panel as PanelId)}).catch(()=>undefined)]);const timer=window.setInterval(()=>void load(true),(config?.refresh_interval_minutes??30)*60_000);return()=>window.clearInterval(timer)},[config?.refresh_interval_minutes,load]);
  useEffect(()=>{const stop=listen<string>("vic-console-command",event=>{if(event.payload==="show_weather")setPanel("weather");if(event.payload==="refresh_dashboard"){setPanel("weather");void load(true)}});return()=>{void stop.then(unlisten=>unlisten())}},[load]);
  const choosePanel=(next:PanelId)=>{setPanel(next);void consoleApi.settings().then(settings=>consoleApi.updateSettings({...settings,selected_panel:next})).catch(()=>undefined)};
  const units=(unit:TemperatureUnit)=>{void consoleApi.settings().then(settings=>consoleApi.updateSettings({...settings,temperature_unit:unit})).then(()=>consoleApi.config()).then(value=>{setConfig(value);return load(true)}).catch(error=>dispatch({type:"failure",error:String(error)}))};
  const title=panel==="weather"?"Weather intelligence":panel[0].toUpperCase()+panel.slice(1);
  return <main className="app-shell"><TitleBar/><div className="app-body"><NavRail selected={panel} onSelect={choosePanel}/><section className="workspace"><header className="workspace-header"><div><p className="eyebrow">VIC INFORMATION SYSTEM</p><h1>{title}</h1></div><div className="refresh-status"><span className={weather.phase}/><div><small>LAST SUCCESSFUL REFRESH</small><strong>{weather.snapshot?new Date(weather.snapshot.data.fetched_at).toLocaleTimeString([],{hour:"numeric",minute:"2-digit"}):"Waiting…"}</strong></div></div></header>{panel==="weather"?<WeatherDashboard state={weather} config={config} onRefresh={()=>void load(true)} onUnits={units}/>:<PlaceholderPanel name={title}/>}</section></div></main>
}

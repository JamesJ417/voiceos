import { fireEvent,render,screen } from "@testing-library/react";
import { describe,expect,it,vi } from "vitest";
import { WeatherDashboard } from "../components/WeatherDashboard";
import { NavRail } from "../components/NavRail";
import { snapshot } from "./fixtures";
const config={location_name:"Newberry, Florida",latitude:29.6464,longitude:-82.6065,temperature_unit:"fahrenheit" as const,refresh_interval_minutes:30,api_endpoint:"https://api.open-meteo.com/v1/forecast"};
describe("weather rendering",()=>{
  it("renders loading",()=>{render(<WeatherDashboard state={{phase:"loading",snapshot:null,error:null}} config={config} onRefresh={vi.fn()} onUnits={vi.fn()}/>);expect(screen.getByText("Reading the sky")).toBeInTheDocument()});
  it("renders success with all ten forecast days",()=>{render(<WeatherDashboard state={{phase:"success",snapshot,error:null}} config={config} onRefresh={vi.fn()} onUnits={vi.fn()}/>);expect(screen.getByText("Newberry, Florida")).toBeInTheDocument();expect(screen.getAllByText(/Partly cloudy|Rain/).length).toBeGreaterThanOrEqual(10)});
  it("renders stale fallback and source error",()=>{render(<WeatherDashboard state={{phase:"stale",snapshot:{...snapshot,freshness:"stale",source_error:"offline"},error:"offline"}} config={config} onRefresh={vi.fn()} onUnits={vi.fn()}/>);expect(screen.getByText("Showing saved forecast")).toBeInTheDocument();expect(screen.getByText("offline")).toBeInTheDocument()});
  it("renders terminal error and retries",()=>{const retry=vi.fn();render(<WeatherDashboard state={{phase:"error",snapshot:null,error:"bad response"}} config={config} onRefresh={retry} onUnits={vi.fn()}/>);fireEvent.click(screen.getByText("Try again"));expect(retry).toHaveBeenCalledOnce()});
});
describe("navigation",()=>{it("navigates to placeholder panels",()=>{const select=vi.fn();render(<NavRail selected="weather" onSelect={select}/>);fireEvent.click(screen.getByText("News"));expect(select).toHaveBeenCalledWith("news");expect(screen.getAllByText("SOON")).toHaveLength(3)})});

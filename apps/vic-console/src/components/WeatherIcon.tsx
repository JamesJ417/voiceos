import { weatherDescription } from "../lib/weatherPresentation";
export function WeatherIcon({code,large=false}:{code:number;large?:boolean}){const symbol=code===0?"☀":code<=3?"◒":code===45||code===48?"≋":code>=95?"ϟ":code>=71&&code<=77?"✣":code>=51&&code<=82?"☂":"◌";return <span className={`weather-icon ${large?"large":""}`} aria-label={weatherDescription(code)} role="img">{symbol}</span>}

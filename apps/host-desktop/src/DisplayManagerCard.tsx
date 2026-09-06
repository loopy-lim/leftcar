import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SupportedLanguage } from "@leftcar/ui-tokens";
import { buttonVariants } from "./lib/variants";

type Position = "left" | "right" | "above" | "below";
type ProviderKind = "betterdisplay" | "cgvirtualdisplay";
interface ManagedDisplay { id:string; name:string; logical_width:number; logical_height:number; backing_width:number; backing_height:number; position:Position; verified:boolean }

const copy = {
  ko: { hint:"화면 크기와 위치를 고르면 태블릿용 작업 공간을 추가합니다.", name:"화면 이름", size:"화면 크기", width:"너비", height:"높이", quality:"글자 선명도", standard:"보통", hidpi:"선명하게", position:"놓을 위치", add:"화면 추가", remove:"제거", needsCheck:"확인 필요", retryCleanup:"정리 다시 시도", engine:"만드는 방식", compatible:"BetterDisplay", builtin:"내장 방식 (실험적)", left:"왼쪽", right:"오른쪽", above:"위", below:"아래", invalid:"너비와 높이는 800 이상의 정수여야 합니다.", workspace:"작업 영역", pixels:"선명도" },
  en: { hint:"Choose a size and position to add a workspace for your tablet.", name:"Screen name", size:"Screen size", width:"Width", height:"Height", quality:"Text clarity", standard:"Standard", hidpi:"Sharper", position:"Place it", add:"Add screen", remove:"Remove", needsCheck:"Needs verification", retryCleanup:"Retry cleanup", engine:"Creation method", compatible:"BetterDisplay", builtin:"Built-in method (experimental)", left:"Left", right:"Right", above:"Above", below:"Below", invalid:"Width and height must be integers of at least 800.", workspace:"Workspace", pixels:"Clarity" },
};
const positions: Position[] = ["left", "right", "above", "below"];
const parseDimension = (value:string) => { const number=Number(value); return Number.isInteger(number)&&number>=800 ? number : null; };
const friendlyName = (name:string) => name.replace(/ \[leftcar:[0-9a-f-]{36}\]$/i, "");

export default function DisplayManagerCard({enabled,language}:{enabled:boolean;language:SupportedLanguage}) {
  const t=copy[language];
  const [displays,setDisplays]=useState<ManagedDisplay[]>([]);
  const [name,setName]=useState("Tablet workspace");
  const [width,setWidth]=useState("1600");
  const [height,setHeight]=useState("1000");
  const [scale,setScale]=useState<1|2>(2);
  const [position,setPosition]=useState<Position>("right");
  const [providerKind,setProviderKind]=useState<ProviderKind>("betterdisplay");
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const refresh=useCallback(async()=>{try{setDisplays(await invoke<ManagedDisplay[]>("list_managed_displays"));setError(null);}catch(cause){setError(String(cause));}},[]);
  useEffect(()=>{if(enabled)void refresh();},[enabled,refresh]);
  const run=async(action:()=>Promise<unknown>)=>{setBusy(true);setError(null);try{await action();await refresh();}catch(cause){setError(String(cause));try{setDisplays(await invoke<ManagedDisplay[]>("list_managed_displays"));}catch{/* keep original error */}}finally{setBusy(false);}};
  const add=()=>{const parsedWidth=parseDimension(width),parsedHeight=parseDimension(height);if(parsedWidth===null||parsedHeight===null){setError(t.invalid);return;}void run(()=>invoke("add_managed_display",{providerKind,name,width:parsedWidth,height:parsedHeight,scale,position}));};
  return <div className="virtual-screen-manager">
    <p className="virtual-screen-hint">{t.hint}</p>
    <div className="virtual-screen-form">
      <label>{t.name}<input value={name} onChange={event=>setName(event.target.value)}/></label>
      <div className="virtual-screen-size-field"><span id="virtual-screen-size-label">{t.size}</span><span className="virtual-screen-size" role="group" aria-labelledby="virtual-screen-size-label"><input aria-label={t.width} inputMode="numeric" value={width} onChange={event=>setWidth(event.target.value)}/><span>×</span><input aria-label={t.height} inputMode="numeric" value={height} onChange={event=>setHeight(event.target.value)}/></span></div>
      <label>{t.quality}<select value={scale} onChange={event=>setScale(event.target.value==="2"?2:1)}><option value={1}>{t.standard}</option><option value={2}>{t.hidpi}</option></select></label>
      <label>{t.position}<select value={position} onChange={event=>setPosition(event.target.value as Position)}>{positions.map(item=><option key={item} value={item}>{t[item]}</option>)}</select></label>
      <label>{t.engine}<select value={providerKind} onChange={event=>setProviderKind(event.target.value as ProviderKind)}><option value="betterdisplay">{t.compatible}</option><option value="cgvirtualdisplay">{t.builtin}</option></select></label>
      <button className={buttonVariants({variant:"primary",size:"sm"})} disabled={busy||!enabled} onClick={add}>{t.add}</button>
    </div>
    <div className="virtual-screen-list">{displays.map(display=><div key={display.id} className="virtual-screen-row">
      <span><strong>{friendlyName(display.name)}</strong>{display.verified?` · ${t.workspace} ${display.logical_width}×${display.logical_height} · ${t.pixels} ${display.backing_width}×${display.backing_height}`:` · ${t.needsCheck}`}</span>
      <select aria-label={`${friendlyName(display.name)} ${t.position}`} value={display.position} disabled={busy||!display.verified} onChange={event=>void run(()=>invoke("set_managed_display_position",{id:display.id,position:event.target.value}))}>{positions.map(item=><option key={item} value={item}>{t[item]}</option>)}</select>
      <button className={buttonVariants({variant:"ghost",size:"sm"})} disabled={busy} onClick={()=>void run(()=>invoke("remove_managed_display",{id:display.id}))}>{display.verified?t.remove:t.retryCleanup}</button>
    </div>)}</div>
    {error&&<p className="font-rose virtual-screen-error" role="alert">{error}</p>}
  </div>;
}

import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {tr} from './i18n';
export function DownloadSettings(){
 const [value,setValue]=useState('0'),[saved,setSaved]=useState(''),[busy,setBusy]=useState(true),[error,setError]=useState('');
 useEffect(()=>{let active=true;invoke<number>('download_limit_get').then(rate=>{if(active)setValue(String(rate/1048576));}).catch(e=>{if(active)setError(String(e));}).finally(()=>{if(active)setBusy(false);});return()=>{active=false;};},[]);
 const rate=Number(value.replace(',','.'));
 const valid=value.trim()!==''&&Number.isFinite(rate)&&rate>=0&&rate<=10240;
 async function save(){if(!valid)return;setBusy(true);setError('');setSaved('');try{await invoke('download_limit_set',{bytesPerSecond:Math.round(rate*1048576)});setSaved('Сохранено');}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <section><h3>{tr('Загрузки')}</h3><div className="setting-row"><div><label htmlFor="download-speed-limit"><b>{tr('Ограничение скорости скачивания')}</b></label><span>{tr('Общий лимит для движков и материалов. 0 — без ограничений. Изменение применяется сразу.')}</span></div><div className="download-limit-controls"><input id="download-speed-limit" type="number" min="0" max="10240" step="0.1" value={value} disabled={busy} onChange={e=>{setValue(e.target.value);setSaved('');}}/><span>{tr('МБ/с')}</span><button disabled={busy||!valid} onClick={()=>void save()}>{tr('Применить')}</button></div></div>{saved&&<p role="status">{tr(saved)}</p>}{error&&<p role="alert" className="error-text">{tr(error)}</p>}</section>;
}

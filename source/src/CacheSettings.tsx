import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Trash2} from 'lucide-react';
import {tr,getLocale} from './i18n';
type Info={dataPath:string;cachePath:string;settingsPath:string;cacheBytes:number};
export function CacheSettings(){
 const [info,setInfo]=useState<Info|null>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState(''),[error,setError]=useState('');
 useEffect(()=>{let active=true;invoke<Info>('storage_info').then(v=>{if(active)setInfo(v);}).catch(e=>{if(active)setError(String(e));});return()=>{active=false;};},[]);
 async function clear(){setBusy(true);setError('');setMessage('');try{await invoke('clear_cache');setInfo(await invoke<Info>('storage_info'));setMessage('Кэш очищен.');}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <section><span className="eyebrow">{tr('ДАННЫЕ')}</span><h3>{tr('Настройки и кэш')}</h3>
 <p className="muted">{tr('Удаляются только каталоги и изображения из кэша. Настройки, вход в Epic и скачанные материалы сохраняются. Кэш создаётся заново по мере использования.')}</p>
 {info&&<><div className="setting-row"><div><b>{tr('Папка данных')}</b><span className="storage-path">{info.dataPath}</span></div></div><div className="setting-row"><div><b>{tr('Настройки')}</b><span className="storage-path">{info.settingsPath}</span></div></div><div className="setting-row"><div><b>{tr('Кэш')}</b><span className="storage-path">{info.cachePath}</span><span>{(info.cacheBytes/1048576).toLocaleString(getLocale(),{maximumFractionDigits:1})} MB</span></div><button disabled={busy} onClick={clear}><Trash2 size={17}/>{tr(busy?'Очистка кэша…':'Очистить кэш')}</button></div></>}
 {message&&<p role="status">{tr(message)}</p>}{error&&<p role="alert">{tr(error)}</p>}</section>;
}

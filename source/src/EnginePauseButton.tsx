import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Pause,Play} from 'lucide-react';
import {tr} from './i18n';
export function EnginePauseButton({paused,canPause}:{paused?:boolean;canPause?:boolean}){
 const [wanted,setWanted]=useState<boolean|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{setWanted(null);},[paused,canPause]);
 if(!canPause)return null;
 const effective=wanted??!!paused;
 async function toggle(){setBusy(true);setError('');const next=!effective;try{await invoke('engine_pause',{paused:next});setWanted(next);}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <div className="engine-pause-controls"><button className="engine-pause-button" disabled={busy} onClick={()=>void toggle()} title={tr('Пауза применяется в ближайшей безопасной точке')}>
 {effective?<Play size={16}/>:<Pause size={16}/>} {tr(effective?'Продолжить':'Пауза')}</button>
 {wanted===true&&!paused&&<small role="status">{tr('Ожидаем безопасной точки для паузы…')}</small>}
 {error&&<span className="error-text" role="alert">{tr(error)}</span>}</div>;
}

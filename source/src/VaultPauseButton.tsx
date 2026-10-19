import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {Pause,Play} from 'lucide-react';
import {tr} from './i18n';
export function VaultPauseButton({paused,canPause}:{paused?:boolean;canPause?:boolean}){
 const[waiting,setWaiting]=useState(false),[wanted,setWanted]=useState<boolean|null>(null),[error,setError]=useState('');const lock=useRef(false);
 useEffect(()=>setWanted(null),[paused,canPause]);
 if(!canPause)return null;const effective=wanted??!!paused;
 async function toggle(){if(lock.current)return;lock.current=true;setWaiting(true);setError('');const next=!effective;try{await invoke('vault_pause',{paused:next});setWanted(next);}catch(e){setError(String(e));}finally{lock.current=false;setWaiting(false);}}
 return <div className="engine-pause-controls"><button disabled={waiting} onClick={()=>void toggle()}>{effective?<Play size={16}/>:<Pause size={16}/>} {tr(effective?'Продолжить':'Пауза')}</button>{error&&<span role="alert" className="error-text">{tr(error)}</span>}</div>;
}

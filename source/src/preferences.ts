import {invoke} from '@tauri-apps/api/core';
const keys=['unreal-manager.language.v1','um-library-page-size','um-plugins-page-size','unreal-manager.download-history.v1'];
const native=()=>typeof window!=='undefined'&&'__TAURI_INTERNALS__' in window;
let queue=Promise.resolve();
export function getPreference(key:string){try{return localStorage.getItem(key);}catch{return null;}}
function report(error:unknown){window.dispatchEvent(new CustomEvent('preferences-error',{detail:String(error)}));}
export function setPreference(key:string,value:string){
 try{localStorage.setItem(key,value);}catch(error){report(error);}
 if(native())queue=queue.then(()=>invoke<void>('settings_write',{key,value})).catch(report);
}
export async function initializePreferences(){
 if(!native())return;
 const values=await invoke<Record<string,string>>('settings_read');
 for(const key of keys){
  if(typeof values[key]==='string')localStorage.setItem(key,values[key]);
  else {const value=getPreference(key);if(value!==null)await invoke('settings_write',{key,value});}
 }
}

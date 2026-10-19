import {getPreference,setPreference} from './preferences';
import type {VaultPackage} from './Vault';
export type DownloadRecord = {item:VaultPackage; completedAt:number|null};
export type DownloadHistory = {initialized:boolean; records:DownloadRecord[]};
export const historyKey='unreal-manager.download-history.v1';
export function readDownloadHistory():DownloadHistory {
 try {
  const value=JSON.parse(getPreference(historyKey)||'null');
  if(value?.initialized===true&&Array.isArray(value.records))return {initialized:true,records:value.records.filter((r:any)=>r?.item&&typeof r.item.id==='string'&&typeof r.item.title==='string'&&Array.isArray(r.item.engines)&&typeof r.item.bytes==='number'&&(r.completedAt===null||typeof r.completedAt==='number'))};
 } catch {}
 return {initialized:false,records:[]};
}
export function seedDownloadHistory(history:DownloadHistory,items:VaultPackage[]):DownloadHistory {
 return history.initialized?history:{initialized:true,records:items.map(item=>({item,completedAt:null}))};
}
export function recordDownload(history:DownloadHistory,item:VaultPackage,now=Date.now()):DownloadHistory {
 return {initialized:true,records:[{item,completedAt:now},...history.records.filter(r=>r.item.id!==item.id)].slice(0,500)};
}
export function clearDownloadHistory(history:DownloadHistory,id?:string):DownloadHistory {
 return {initialized:true,records:id?history.records.filter(r=>r.item.id!==id):[]};
}

import {getPreference,setPreference} from './preferences';
import {invoke} from '@tauri-apps/api/core';
import {useSyncExternalStore} from 'react';
import en from './locales/en.json';
import de from './locales/de.json';
import it from './locales/it.json';
import es from './locales/es.json';
import fr from './locales/fr.json';
import zh from './locales/zh.json';
import ja from './locales/ja.json';
import ko from './locales/ko.json';
import overrides from './locales/overrides.json';
import appUpdateMessages from './locales/app-updates.json';
import libraryControlMessages from './locales/library-controls.json';
export type Language='ru'|'en'|'de'|'it'|'es'|'fr'|'zh'|'ja'|'ko';
const supported=['ru','en','de','it','es','fr','zh','ja','ko'];
const key='unreal-manager.language.v1';
export function resolveLanguage(preference:string,systemLanguage:string):Language {
 const candidate=(preference==='system'?systemLanguage:preference).toLowerCase().split(/[-_]/)[0];
 return supported.includes(candidate)?candidate as Language:'en';
}
let preference='system';
try{const saved=getPreference(key);if(saved==='system'||supported.includes(saved||''))preference=saved!;}catch{}
const system=()=>typeof navigator==='undefined'?'en':navigator.language;
let language=resolveLanguage(preference,system());
const listeners=new Set<()=>void>();
const catalogs:Record<string,Record<string,string>>={en,de,it,es,fr,zh,ja,ko};
for(const [locale,entries] of Object.entries(overrides))Object.assign(catalogs[locale],entries);
for(const [locale,entries] of Object.entries(appUpdateMessages))Object.assign(catalogs[locale],entries);
for(const [locale,entries] of Object.entries(libraryControlMessages))Object.assign(catalogs[locale],entries);
const normalize=(s:string)=>s.replace(/\s+/g,' ').trim();
const escape=(s:string)=>s.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
const indexes=new Map<string,Map<string,string>>();
const patterns=new Map<string,{regex:RegExp;output:string;names:string[]}[]>();
const memo=new Map<string,string>();
for(const [locale,entries] of Object.entries(catalogs)){
 indexes.set(locale,new Map(Object.entries(entries).map(([a,b])=>[normalize(a),b])));
 const list=[];
 for(const [source,output] of Object.entries(entries)){
  const names=source.match(/\{[^{}]*\}/g);if(!names)continue;
  const parts=normalize(source).split(/\{[^{}]*\}/g);
  list.push({regex:new RegExp('^'+parts.map(escape).join('(.+?)')+'$'),output,names});
 }
 patterns.set(locale,list.sort((a,b)=>b.regex.source.length-a.regex.source.length));
}
function translate(value:string,depth=0):string {
 if(language==='ru'||!/[А-Яа-яЁё]/.test(value))return value;
 const cacheKey=language+'\0'+value;const cached=memo.get(cacheKey);if(cached!==undefined)return cached;
 const trimmed=normalize(value);let output=indexes.get(language)?.get(trimmed)??indexes.get('en')?.get(trimmed);
 if(output===undefined){
  for(const pattern of patterns.get(language)||[]){
   const match=pattern.regex.exec(trimmed);if(!match)continue;
   const captures=new Map<string,string[]>();pattern.names.forEach((name,i)=>{const values=captures.get(name)||[];values.push(depth<2&&/^(?:Не удалось|Ошибка|Некоррект)/.test(match[i+1])?translate(match[i+1],depth+1):match[i+1]);captures.set(name,values);});
   output=pattern.output.replace(/\{[^{}]*\}/g,name=>{const values=captures.get(name);return values?.length?(values.length>1?values.shift()!:values[0]):name;});break;
  }
 }
 if(output===undefined&&depth<2){const progress=/^(.*?)(\s*·\s*\d+(?:[.,]\d+)?%.*)$/.exec(trimmed);if(progress){const phase=translate(progress[1],depth+1);if(phase!==progress[1])output=phase+progress[2];}}
 const result=output===undefined?value:(value.match(/^\s*/)?.[0]||'')+output.trim()+(value.match(/\s*$/)?.[0]||'');
 if(memo.size>4096)memo.clear();memo.set(cacheKey,result);return result;
}
// Translation happens only at rendering boundaries. Paths, API values and state stay unchanged.
export function tr<T>(value:T):T{return (typeof value==='string'?translate(value):value) as T;}
export function getLocale(){return language==='zh'?'zh-CN':language;}
export function getLanguagePreference(){return preference;}
function syncNative(){void invoke('set_ui_language',{language}).catch(()=>{});}
function notify(){language=resolveLanguage(preference,system());memo.clear();if(typeof document!=='undefined')document.documentElement.lang=getLocale();syncNative();listeners.forEach(listener=>listener());}
export function setLanguagePreference(value:string){if(value!=='system'&&!supported.includes(value))return;preference=value;try{setPreference(key,value);}catch{}notify();}
export function useLanguage(){return useSyncExternalStore(listener=>{listeners.add(listener);return()=>{listeners.delete(listener);};},()=>preference+':'+language);}
if(typeof document!=='undefined')document.documentElement.lang=getLocale();
if(typeof window!=='undefined')window.addEventListener('languagechange',()=>{if(preference==='system')notify();});

syncNative();

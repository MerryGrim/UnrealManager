import {useSyncExternalStore} from 'react';
import {getPreference,setPreference} from './preferences';

export type Theme='dark'|'light';
const preferenceKey='unreal-manager.theme.v1';
const listeners=new Set<()=>void>();
let theme:Theme='dark';

function applyTheme(next:Theme){
    theme=next;
    document.documentElement.dataset.theme=next;
    document.documentElement.style.colorScheme=next;
    listeners.forEach(listener=>listener());
}

// Run after native preferences have been restored and before mounting the app.
export function initializeTheme(){
    applyTheme(getPreference(preferenceKey)==='light'?'light':'dark');
}

export function setTheme(next:Theme){
    if(next!=='dark'&&next!=='light')return;
    if(theme===next)return;
    setPreference(preferenceKey,next);
    applyTheme(next);
}

export function toggleTheme(){setTheme(theme==='dark'?'light':'dark');}
const subscribe=(listener:()=>void)=>{listeners.add(listener);return()=>{listeners.delete(listener);};};
const snapshot=()=>theme;

export function useTheme(){
    return {theme:useSyncExternalStore(subscribe,snapshot,snapshot),setTheme,toggleTheme};
}

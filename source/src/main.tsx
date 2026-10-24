import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles.css';
import './RedeemView.css';
import {initializePreferences,getPreference} from './preferences';
import {setLanguagePreference} from './i18n';
import {installMainContextMenuPolicy} from './contextMenuPolicy';

const removeContextMenuPolicy = installMainContextMenuPolicy();
if (import.meta.hot) import.meta.hot.dispose(removeContextMenuPolicy);

async function start(){
try {await initializePreferences();setLanguagePreference(getPreference('unreal-manager.language.v1')||'system');} catch(error){document.getElementById('root')!.textContent='Could not load settings: '+String(error);return;}
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode><App /></React.StrictMode>
);

}
void start();

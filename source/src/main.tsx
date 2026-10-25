import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles.css';
import './RedeemView.css';
import './WorkspaceHeader.css';
import {initializeTheme} from './theme';
import {initializePreferences,getPreference} from './preferences';
import {setLanguagePreference} from './i18n';
import {installMainContextMenuPolicy} from './contextMenuPolicy';
import {AppStartupFailure} from './AppStartupFailure';

const removeContextMenuPolicy = installMainContextMenuPolicy();
if (import.meta.hot) import.meta.hot.dispose(removeContextMenuPolicy);

const applicationRoot=ReactDOM.createRoot(document.getElementById('root')!);
let starting=false;
async function start(){
if(starting)return;
starting=true;
try {
initializeTheme();
try {await initializePreferences();initializeTheme();setLanguagePreference(getPreference('unreal-manager.language.v1')||'system');} catch(error){applicationRoot.render(<React.StrictMode><AppStartupFailure error={String(error)} onRetry={()=>{void start();}} /></React.StrictMode>);return;}
applicationRoot.render(
  <React.StrictMode><App /></React.StrictMode>
);
}finally{starting=false;}
}
void start();

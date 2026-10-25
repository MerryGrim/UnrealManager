import {tr} from './i18n';
import {useTheme,type Theme} from './theme';

export function ThemeSettings(){
    const {theme,setTheme}=useTheme();
    return <section className="theme-settings">
        <h3>{tr('Оформление')}</h3>
        <div className="setting-row"><div><b>{tr('Оформление')}</b></div>
            <select aria-label={tr('Оформление')} value={theme} onChange={event=>setTheme(event.target.value as Theme)}>
                <option value="dark">{tr('Тёмная')}</option>
                <option value="light">{tr('Светлая')}</option>
            </select>
        </div>
    </section>;
}

const fs=require('fs'),assert=require('assert');const {root,outputDirectory}=require('./test-env.cjs');const languages=['en','de','it','es','fr','zh','ja','ko'];const overrides=JSON.parse(fs.readFileSync(root+'/src/locales/overrides.json','utf8'));const source=JSON.parse(fs.readFileSync(root+'/src/locales/en.json','utf8'));const placeholders=s=>(s.match(/\{[^{}]*\}/g)||[]).sort();for(const lang of languages){const data={...JSON.parse(fs.readFileSync(root+'/src/locales/'+lang+'.json','utf8')),...overrides[lang]};for(const [key,value]of Object.entries(source)){assert.equal(typeof data[key],'string',lang+': '+key);assert(data[key].trim(),lang+': empty '+key);assert.deepEqual(placeholders(data[key]),placeholders(key),lang+': placeholders '+key);if(key!=='Русский')assert(!/[А-Яа-яЁё]/.test(data[key]),lang+': untranslated '+key);}console.log(lang,Object.keys(data).length,'messages checked');}const version=JSON.parse(fs.readFileSync(root+'/package.json')).version;assert.match(version,/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);assert.equal(JSON.parse(fs.readFileSync(root+'/src-tauri/tauri.conf.json')).version,version);assert(fs.readFileSync(root+'/src-tauri/Cargo.toml','utf8').includes('version = "'+version+'"'));console.log('PASS catalogue coverage, placeholders, language completeness and version consistency');


const updateLocales=JSON.parse(fs.readFileSync(root+'/src/locales/app-updates.json','utf8'));
const updateKeys=Object.keys(updateLocales.en);
for(const lang of languages){assert.deepEqual(Object.keys(updateLocales[lang]).sort(),[...updateKeys].sort(),lang+': update message keys');for(const [key,value]of Object.entries(updateLocales[lang])){assert.equal(typeof value,'string');assert(value.trim());assert.deepEqual(placeholders(value),placeholders(key),lang+': update placeholders '+key);assert(!/[А-Яа-яЁё]/.test(value),lang+': untranslated update '+key);}}
console.log(languages.length+' languages × '+updateKeys.length+' update messages checked');

const libraryLocales=JSON.parse(fs.readFileSync(root+'/src/locales/library-controls.json','utf8'));
const libraryKeys=['Сортировка библиотеки','Название: А–Я','Название: Я–А','Тип материала','Версия UE: сначала новые','Версия UE: сначала старые','Сначала скачанные','Сначала нескачанные','Навигация библиотеки','Проверяем регистрацию…','Проверяем регистрацию движков. При необходимости Windows запросит разрешение администратора.'];
assert.deepEqual(Object.keys(libraryLocales).sort(),[...languages].sort(),'exact non-Russian library locales');
for(const lang of languages){
    assert.deepEqual(Object.keys(libraryLocales[lang]).sort(),[...libraryKeys].sort(),lang+': library message keys');
    for(const [key,value]of Object.entries(libraryLocales[lang])){
        assert.equal(typeof value,'string',lang+': library '+key);
        assert(value.trim(),lang+': empty library '+key);
        assert.deepEqual(placeholders(value),placeholders(key),lang+': library placeholders '+key);
        assert(!/[А-Яа-яЁё]/.test(value),lang+': untranslated library '+key);
        assert.notEqual(value,key,lang+': Russian library fallback '+key);
    }
}
console.log(languages.length+' languages × '+libraryKeys.length+' library control messages checked');

const projectLocales=JSON.parse(fs.readFileSync(root+'/src/locales/project-discovery.json','utf8'));
const projectKeys=Object.keys(projectLocales.en);
assert.deepEqual(Object.keys(projectLocales).sort(),[...languages].sort(),'exact project discovery locales');
assert(projectKeys.includes('Найти проекты')&&projectKeys.includes('Остановить поиск'),'project discovery actions translated');
for(const lang of languages){
    assert.deepEqual(Object.keys(projectLocales[lang]).sort(),[...projectKeys].sort(),lang+': project discovery message keys');
    for(const [key,value]of Object.entries(projectLocales[lang])){
        assert.equal(typeof value,'string',lang+': project discovery '+key);
        assert(value.trim(),lang+': empty project discovery '+key);
        assert.deepEqual(placeholders(value),placeholders(key),lang+': project discovery placeholders '+key);
        assert(!/[А-Яа-яЁё]/.test(value),lang+': untranslated project discovery '+key);
    }
}
console.log(languages.length+' languages × '+projectKeys.length+' project discovery messages checked');

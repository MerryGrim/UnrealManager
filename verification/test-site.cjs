const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const assert = require('node:assert/strict');

const startedAt = new Date();
const outputDirectory = path.join(__dirname, 'current', `site-${startedAt.toISOString().replace(/[:.]/g, '-')}`);
const temporaryDirectory = path.join(outputDirectory, 'browser-temp');
fs.mkdirSync(temporaryDirectory, {recursive:true});
process.env.UM_VERIFICATION_OUTPUT_DIRECTORY = outputDirectory;
process.env.TEMP = temporaryDirectory;
process.env.TMP = temporaryDirectory;
const {playwright, browserLaunchOptions} = require('./test-env.cjs');
const docsRoot = fs.realpathSync(path.resolve(__dirname, '..', 'docs'));
const prefix = '/UnrealManager/';
const locales = {
  ru: {html:'ru',language:'Язык',theme:'Тема',options:['Системная','Светлая','Тёмная'],title:'Unreal Manager — Ваше рабочее пространство Unreal под вашим контролем'},
  en: {html:'en',language:'Language',theme:'Theme',options:['System','Light','Dark'],title:'Unreal Manager — Your Unreal workspace, under your control'},
  de: {html:'de',language:'Sprache',theme:'Darstellung',options:['System','Hell','Dunkel'],title:'Unreal Manager — Dein Unreal-Arbeitsbereich, unter deiner Kontrolle'},
  it: {html:'it',language:'Lingua',theme:'Tema',options:['Sistema','Chiaro','Scuro'],title:'Unreal Manager — Il tuo spazio Unreal, sotto il tuo controllo'},
  es: {html:'es',language:'Idioma',theme:'Tema',options:['Sistema','Claro','Oscuro'],title:'Unreal Manager — Tu espacio Unreal, bajo tu control'},
  fr: {html:'fr',language:'Langue',theme:'Thème',options:['Système','Clair','Sombre'],title:'Unreal Manager — Votre espace Unreal, sous votre contrôle'},
  zh: {html:'zh-CN',language:'语言',theme:'主题',options:['跟随系统','浅色','深色'],title:'Unreal Manager — 由你掌控的 Unreal 工作空间'},
  ja: {html:'ja',language:'言語',theme:'テーマ',options:['システム','ライト','ダーク'],title:'Unreal Manager — 自分で管理する Unreal ワークスペース'},
  ko: {html:'ko',language:'언어',theme:'테마',options:['시스템','밝게','어둡게'],title:'Unreal Manager — 직접 관리하는 Unreal 작업 공간'},
};
const results=[];
const pageErrors=[];
const consoleErrors=[];
const failedRequests=[];
const badResponses=[];
const externalRequests=[];
const serverRequests=[];
let server;
let browser;
let baseUrl;

function createServer() {
  const types={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml','.webp':'image/webp','.png':'image/png','.ico':'image/x-icon'};
  return http.createServer((request,response)=>{
    try {
      const address=new URL(request.url,'http://127.0.0.1');
      serverRequests.push(address.pathname);
      if (!['GET','HEAD'].includes(request.method)||!address.pathname.startsWith(prefix)) {
        response.writeHead(404);response.end();return;
      }
      const relative=decodeURIComponent(address.pathname.slice(prefix.length))||'index.html';
      const file=path.resolve(docsRoot,relative);
      if (!file.startsWith(docsRoot+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()) {
        response.writeHead(404);response.end();return;
      }
      if (!fs.realpathSync(file).startsWith(docsRoot+path.sep)) {response.writeHead(403);response.end();return;}
      response.writeHead(200,{'Content-Type':types[path.extname(file)]||'application/octet-stream','Cache-Control':'no-store'});
      response.end(request.method==='HEAD'?undefined:fs.readFileSync(file));
    } catch {response.writeHead(400);response.end();}
  });
}

async function context(options={}) {
  const value=await browser.newContext({locale:'en-US',reducedMotion:'reduce',colorScheme:'light',...options});
  await value.route('**/*',route=>{
    const url=route.request().url();
    if(new URL(url).origin!==new URL(baseUrl).origin){externalRequests.push(url);return route.abort();}
    return route.continue();
  });
  value.on('page',page=>{
    page.on('pageerror',error=>pageErrors.push(error.message));
    page.on('console',message=>{if(message.type()==='error')consoleErrors.push(message.text());});
    page.on('requestfailed',request=>failedRequests.push({url:request.url(),failure:request.failure()?.errorText}));
    page.on('response',response=>{if(response.status()>=400)badResponses.push({url:response.url(),status:response.status()});});
  });
  return value;
}

async function go(page,query='') {
  await page.goto(baseUrl+query,{waitUntil:'load'});
  await page.waitForFunction(()=>globalThis.UM_TRANSLATIONS&&document.querySelector('#language')?.value);
  await page.evaluate(async()=>{
    await document.fonts.ready;
    for(const image of document.images)image.loading='eager';
    await Promise.all(Array.from(document.images).map(image=>image.decode()));
  });
}

async function validateLocale(page,locale) {
  const expected=locales[locale];
  assert.equal(await page.locator('html').getAttribute('lang'),expected.html,`${locale}: html lang`);
  assert.equal(await page.title(),expected.title,`${locale}: title`);
  assert.equal(await page.locator('#language').inputValue(),locale);
  assert.equal(await page.locator('#language').getAttribute('aria-label'),expected.language);
  assert.equal(await page.locator('#theme').getAttribute('aria-label'),expected.theme);
  assert.deepEqual(await page.locator('#theme option').allTextContents(),expected.options);
  assert.deepEqual((await page.locator('#language option').evaluateAll(options=>options.map(option=>option.value))).sort(),Object.keys(locales).sort());
  const problems=await page.evaluate(locale=>{
    const copy=globalThis.UM_TRANSLATIONS[locale];
    const markers={'data-i18n':null,'data-i18n-alt':'alt','data-i18n-aria':'aria-label','data-i18n-title':'title'};
    const failures=[];
    for(const [marker,attribute] of Object.entries(markers))for(const element of document.querySelectorAll(`[${marker}]`)){
      const key=element.getAttribute(marker);
      if(typeof copy[key]!=='string'||!copy[key].trim()){failures.push(`Missing ${locale}.${key}`);continue;}
      const actual=attribute?element.getAttribute(attribute):element.textContent;
      if(actual!==copy[key])failures.push(`Not applied: ${locale}.${key}`);
    }
    if(document.querySelector('meta[name="description"]').content!==copy.metaDescription)failures.push('description');
    if(document.querySelector('meta[property="og:title"]').content!==copy.metaTitle)failures.push('og:title');
    if(document.querySelector('meta[property="og:description"]').content!==copy.metaDescription)failures.push('og:description');
    return failures;
  },locale);
  assert.deepEqual(problems,[],`${locale}: all visible copy/metadata/alt/aria/title markers must be translated without English fallback`);
}

async function validateTheme(page,theme) {
  assert.equal(await page.locator('html').getAttribute('data-theme'),theme);
  const colors=await page.evaluate(()=>({background:getComputedStyle(document.body).backgroundColor,text:getComputedStyle(document.body).color}));
  assert.equal(await page.locator('meta[name="theme-color"]').getAttribute('content'),theme==='dark'?'#101820':'#f5f6f2');
  return colors;
}

async function check(name,action) {
  try {const detail=await action();results.push({name,passed:true,...(detail||{})});console.log(`PASS ${name}`);}
  catch(error){results.push({name,passed:false,error:error.stack});console.error(`FAIL ${name}: ${error.message}`);}
}

async function main() {
  server=createServer();
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  baseUrl=`http://127.0.0.1:${server.address().port}${prefix}`;
  browser=await playwright.chromium.launch({...browserLaunchOptions,args:[...browserLaunchOptions.args,'--disable-background-networking','--disable-sync']});
  const matrixContext=await context();
  const page=await matrixContext.newPage();
  for(const viewport of [{name:'desktop',width:1280,height:900},{name:'mobile',width:390,height:844}]){
    await page.setViewportSize({width:viewport.width,height:viewport.height});
    for(const locale of Object.keys(locales)){
      const themeColors={};
      for(const theme of ['dark','light'])await check(`${viewport.name}-${locale}-${theme}`,async()=>{
        await go(page,`?lang=${locale}&theme=${theme}`);
        await validateLocale(page,locale);
        themeColors[theme]=await validateTheme(page,theme);
        const layout=await page.evaluate(()=>({viewport:innerWidth,width:document.documentElement.scrollWidth,images:Array.from(document.images).map(image=>({src:image.getAttribute('src'),loaded:image.complete&&image.naturalWidth>0,width:image.naturalWidth,height:image.naturalHeight}))}));
        assert(layout.width<=layout.viewport+1,`horizontal overflow: ${JSON.stringify(layout)}`);
        assert(layout.images.every(image=>image.loaded),'all image assets must load');
        const screenshots=layout.images.filter(image=>image.src.includes('/screenshots/'));
        assert.equal(screenshots.length,4);
        assert(screenshots.every(image=>image.width===1280&&image.height===800),'published screenshot dimensions');
        if((viewport.name==='desktop'&&((locale==='ru'&&theme==='dark')||(locale==='en'&&theme==='light')))||(viewport.name==='mobile'&&locale==='ru'&&theme==='dark')){
          await page.screenshot({path:path.join(outputDirectory,`${viewport.name}-${locale}-${theme}.png`),animations:'disabled'});
        }
        return {viewport:{width:viewport.width,height:viewport.height},locale,theme,colors:themeColors[theme],imageCount:layout.images.length};
      });
      await check(`${viewport.name}-${locale}-themes-visibly-differ`,async()=>{
        assert(themeColors.dark&&themeColors.light,'both theme cases completed');
        assert.notEqual(themeColors.dark.background,themeColors.light.background);
        assert.notEqual(themeColors.dark.text,themeColors.light.text);
      });
    }
  }
  await matrixContext.close();

  await check('language-theme-persistence-system-and-deep-links',async()=>{
    const value=await context({viewport:{width:1280,height:900},colorScheme:'dark'});
    try {
      const page=await value.newPage();await go(page);
      await page.locator('#language').selectOption('zh');await page.locator('#theme').selectOption('light');
      assert.deepEqual(await page.evaluate(()=>[localStorage.getItem('um.site.language'),localStorage.getItem('um.site.theme')]),['zh','light']);
      await page.reload();await validateLocale(page,'zh');await validateTheme(page,'light');
      await go(page);await validateLocale(page,'zh');await validateTheme(page,'light');
      await go(page,'?lang=ko&theme=dark#screenshots');await validateLocale(page,'ko');await validateTheme(page,'dark');
      assert.deepEqual(await page.evaluate(()=>[localStorage.getItem('um.site.language'),localStorage.getItem('um.site.theme')]),['zh','light'],'deep link overrides do not silently overwrite saved preferences');
      await page.locator('#language').selectOption('de');
      assert.equal(new URL(page.url()).hash,'#screenshots');assert.equal(new URL(page.url()).searchParams.get('lang'),'de');
      await page.locator('#theme').selectOption('system');
      assert.equal(new URL(page.url()).searchParams.has('theme'),false);await validateTheme(page,'dark');
      assert.equal(await page.locator('#theme').inputValue(),'system');
      await page.emulateMedia({colorScheme:'light'});await page.waitForFunction(()=>document.documentElement.dataset.theme==='light');
      await validateTheme(page,'light');
      await go(page);await validateLocale(page,'de');await validateTheme(page,'light');assert.equal(await page.locator('#theme').inputValue(),'system');
      await page.emulateMedia({colorScheme:'dark'});await page.waitForFunction(()=>document.documentElement.dataset.theme==='dark');
      await page.reload();await validateLocale(page,'de');await validateTheme(page,'dark');
      await go(page,'?lang=__proto__&theme=invalid');await validateLocale(page,'en');assert.equal(await page.locator('#theme').inputValue(),'system');await validateTheme(page,'dark');
    } finally {await value.close();}
  });

  await check('keyboard-skip-navigation-and-selects',async()=>{
    const value=await context({viewport:{width:1280,height:900}});
    try {
      const page=await value.newPage();await go(page,'?lang=en&theme=dark');
      await page.keyboard.press('Tab');assert.equal(await page.locator('.skip-link').evaluate(element=>document.activeElement===element),true);
      assert((await page.locator('.skip-link').boundingBox()).y>=0,'skip link becomes visible when focused');
      await page.keyboard.press('Enter');assert.equal(new URL(page.url()).hash,'#main');
      await page.locator('.primary-nav a[href="#features"]').focus();await page.keyboard.press('Enter');assert.equal(new URL(page.url()).hash,'#features');
      assert((await page.locator('#features').boundingBox()).y>=-1,'keyboard anchor brings section into view');
      await page.locator('#language').focus();await page.keyboard.press('Home');await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');await validateLocale(page,'ru');
      await page.locator('#theme').focus();await page.keyboard.press('Home');await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');assert.equal(await page.locator('#theme').inputValue(),'light');await validateTheme(page,'light');
    } finally {await value.close();}
  });

  await check('unavailable-storage-and-system-locale-fallback',async()=>{
    const value=await context({locale:'ja-JP',colorScheme:'dark',viewport:{width:390,height:844}});
    try {
      await value.addInitScript(()=>Object.defineProperty(window,'localStorage',{get(){throw new DOMException('Storage disabled for regression','SecurityError');}}));
      const page=await value.newPage();await go(page);await validateLocale(page,'ja');await validateTheme(page,'dark');
      await page.locator('#language').selectOption('fr');await page.locator('#theme').selectOption('light');
      await validateLocale(page,'fr');await validateTheme(page,'light');
      await page.reload();await validateLocale(page,'fr');await validateTheme(page,'light');
    } finally {await value.close();}
    const unsupported=await context({locale:'pt-BR'});
    try {const page=await unsupported.newPage();await go(page);await validateLocale(page,'en');}
    finally {await unsupported.close();}
  });

  await check('network-and-console-clean',async()=>{
    assert.deepEqual(pageErrors,[]);assert.deepEqual(consoleErrors,[]);assert.deepEqual(failedRequests,[]);assert.deepEqual(badResponses,[]);assert.deepEqual(externalRequests,[]);
    assert(serverRequests.includes(prefix+'app.js'));
    assert(serverRequests.includes(prefix+'translations.js'));
    for(const name of ['engines','library','downloads','settings'])assert(serverRequests.includes(prefix+`assets/screenshots/${name}.webp`));
  });
}

(async()=>{
  let fatalError=null;
  try {await main();}catch(error){fatalError=error.stack;console.error(error);}
  finally {
    if(browser)await browser.close();
    if(server)await new Promise(resolve=>server.close(resolve));
    const report={startedAt:startedAt.toISOString(),finishedAt:new Date().toISOString(),node:process.version,basePath:prefix,passed:!fatalError&&results.every(result=>result.passed),fatalError,results,pageErrors,consoleErrors,failedRequests,badResponses,externalRequests,requestedPaths:[...new Set(serverRequests)].sort(),softwareBrowserArguments:browserLaunchOptions.args,gpuActivity:'Not measured; browser flags are configuration only.'};
    const reportFile=path.join(outputDirectory,'site-report.json');fs.writeFileSync(reportFile,JSON.stringify(report,null,2)+'\n');console.log(`Report: ${reportFile}`);process.exitCode=report.passed?0:1;
  }
})();

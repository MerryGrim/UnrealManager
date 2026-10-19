(() => {
  if (window.top !== window || location.origin !== 'https://store.epicgames.com' || window.__umRedeemObserver) return;
  window.__umRedeemObserver = true;
  const callback = __CALLBACK__;
  let product = '', last = '';
  const clean = value => typeof value === 'string' ? value.replace(/[\u0000-\u001f]/g,' ').slice(0,240) : '';
  function operation(url, body) {
    try {
      const target = new URL(url, location.href);
      if (target.origin !== 'https://store.epicgames.com' || target.pathname !== '/graphql') return '';
      const payload = typeof body === 'string' ? JSON.parse(body) : null;
      const query = payload?.operationName || payload?.query || target.searchParams.get('operationName') || target.searchParams.get('query') || '';
      return ['evaluateCodeQuery','lockCodeMutation','redeemCodeMutation'].find(name=>query.includes(name)) || '';
    } catch { return ''; }
  }
  function report(status, title='', errorCode='') {
    const result = {status,title:clean(title),errorCode:clean(errorCode)};
    const key = JSON.stringify(result); if (key === last) return; last=key;
    // The native navigation handler cancels this request. No result leaves the device.
    location.assign(callback+'?result='+encodeURIComponent(key));
  }
  function response(op, json) {
    const root=json?.data?.CodeRedemption;
    const name={evaluateCodeQuery:'evaluateCode',lockCodeMutation:'lockCode',redeemCodeMutation:'redeemCode'}[op];
    if (!name) return;
    const value=root?.[name];
    if (op !== 'redeemCodeMutation') {
      product=value?.success===true ? clean(value.data?.title) : '';
      if(value?.success===true) { last='';report('ready',product);return; }
    }
    if(op==='redeemCodeMutation' && value?.success===true && !json.errors?.length) {report('success',product);return;}
    if(value?.success===false || json?.errors?.length) {
      // Never forward raw error messages: services can echo the submitted key.
      const codes=JSON.stringify(json.errors||[]).match(/errors\.com\.epicgames\.[a-zA-Z0-9_.]+/);
      report('failed',product,codes?.[0]||'');
    } else if(op==='redeemCodeMutation') report('unknown',product);
  }
  const originalFetch=window.fetch;
  window.fetch=function(input,init) {
    const pending=originalFetch.apply(this,arguments);
    const url=typeof input==='string'?input:input?.url;
    const body=init?.body;
    const op=operation(url,body);
    if(op) pending.then(r=>r.clone().json().then(json=>response(op,json)).catch(()=>{if(op==='redeemCodeMutation')report('unknown',product);}),()=>{if(op==='redeemCodeMutation')report('unknown',product);});
    return pending;
  };
  const open=XMLHttpRequest.prototype.open, send=XMLHttpRequest.prototype.send;
  const requests=new WeakMap();
  XMLHttpRequest.prototype.open=function(method,url){requests.set(this,url);return open.apply(this,arguments);};
  XMLHttpRequest.prototype.send=function(body){
    const op=operation(requests.get(this),body);
    if(op) this.addEventListener('loadend',()=>{
      try {response(op,this.responseType==='json'?this.response:JSON.parse(this.responseText));}
      catch {if(op==='redeemCodeMutation')report('unknown',product);}
    },{once:true});
    return send.apply(this,arguments);
  };
})();

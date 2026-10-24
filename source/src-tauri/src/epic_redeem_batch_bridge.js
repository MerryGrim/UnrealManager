// Runs before the official page's scripts. Keys never enter its DOM, Redux,
// logging helpers, URLs or native response payloads.
(() => {
  if (window.top !== window || location.origin !== 'https://store.epicgames.com') return;
  const callback = __CALLBACK__;
  const transport = window.fetch.bind(window);
  const queries = {
    account: 'query umRedemptionAccount { Account { myAccount { id displayName } } }',
    evaluate: 'query evaluateCodeQuery($codeId: String, $locale: String) { CodeRedemption { evaluateCode(codeId: $codeId, locale: $locale) { success data { namespace offerId title eulaIds entitlementName } } } }',
    lock: 'mutation lockCodeMutation($codeId: String, $locale: String) { CodeRedemption { lockCode(codeId: $codeId, locale: $locale) { success data { namespace title eulaIds entitlementName codeUseId } } } }',
    redeem: 'mutation redeemCodeMutation($codeId: String, $source: String, $codeUseId: String) { CodeRedemption { redeemCode(codeId: $codeId, source: $source, codeUseId: $codeUseId) { success } } }',
    unlock: 'mutation unlockCodeMutation($codeId: String, $codeUseId: String) { CodeRedemption { unlockCode(codeId: $codeId, codeUseId: $codeUseId) { success } } }',
  };
  const clean = (value, limit = 240) => typeof value === 'string' ? value.replace(/[\u0000-\u001f\u007f]/g, '').slice(0, limit) : '';
  const identifier = value => { if (value == null) return ''; if (typeof value !== 'string' || value.length > 128 || /[\u0000-\u001f\u007f]/.test(value)) throw new Error('identifier'); return value; };
  const kind = value => value === undefined ? 'missing' : value === null ? 'null' : Array.isArray(value) ? 'array' : ['object', 'string', 'number', 'boolean'].includes(typeof value) ? typeof value : 'other';
  const send = value => location.assign(callback + '?result=' + encodeURIComponent(JSON.stringify(value)));
  const errorCodes = json => [...JSON.stringify(json?.errors || []).matchAll(/(?:^|[^a-zA-Z0-9_./:-])(errors\.com\.epicgames\.[a-zA-Z0-9_.-]+)(?=$|[^a-zA-Z0-9_./:-])/g)].map(match => match[1]);
  function codeReason(raw) {
    if (/captcha|challenge/i.test(raw)) return 'requiresOfficial';
    if (/unauthoriz|authentication|token_verify|invalid_token|session_invalid|not_authenticated|(?:^|[._])auth(?:[._]|$)/i.test(raw)) return 'authRequired';
    if (/quota|throttl|rate_limit|too_many|max_attempt/i.test(raw) || (/redeem_attempt/i.test(raw) && /exceed|max|too_many|limit/i.test(raw))) return 'rateLimited';
    if (/(?:^|\.)(?:code_used|code_already_used|already_used|codeUse_already_used|code_use_already_used|already_redeemed)(?:$|\.)/i.test(raw)) return 'alreadyUsed';
    if (/(?:^|\.)already_owned(?:$|\.)/i.test(raw)) return 'alreadyOwned';
    if (/code_expired|code_use_expired/i.test(raw)) return 'expired';
    if (/code_not_found|invalid_code/i.test(raw)) return 'notFound';
    if (/region|country|geo_restricted/i.test(raw)) return 'regionRestricted';
    return 'service';
  }
  function safeServiceCode(json, code) {
    const codes = errorCodes(json), category = reason(json);
    const secrets = [code, ...code.split('-')].filter(Boolean).map(part => part.toLowerCase());
    return codes.find(value => codeReason(value) === category && /^errors\.com\.epicgames\.[a-z_.]{1,160}$/.test(value) && !secrets.some(part => value.includes(part)));
  }
  function exactThrottleRefusal(json) {
    const exact = 'errors.com.epicgames.common.throttled';
    function structuredCodes(error) {
      if (!error || typeof error !== 'object' || Array.isArray(error)) return null;
      if (error.extensions != null && (typeof error.extensions !== 'object' || Array.isArray(error.extensions))) return null;
      const codes = [];
      function add(value, wrapper = false) {
        if (value == null) return true;
        if (typeof value !== 'string') return false;
        if (!wrapper || value.startsWith('errors.com.epicgames.')) codes.push(value);
        return true;
      }
      if (!add(error.errorCode) || !add(error.extensions?.errorCode) || !add(error.code, true) || !add(error.extensions?.code, true)) return null;
      for (let service of [error.serviceResponse, error.extensions?.serviceResponse]) {
        if (service == null) continue;
        if (typeof service === 'string') { if (service.length > 8192) return null; try { service = JSON.parse(service); } catch { return null; } }
        if (!service || typeof service !== 'object' || Array.isArray(service) || !add(service.errorCode)) return null;
      }
      return codes;
    }
    if (reason(json) !== 'rateLimited' || !Array.isArray(json?.errors) || !json.errors.length || !json.errors.every(error => { const codes = structuredCodes(error); return codes && codes.length && codes.every(code => code === exact); })) return false;
    const object = value => value && typeof value === 'object' && !Array.isArray(value);
    const data = json.data;
    if (data == null) return true;
    if (!object(data) || Object.keys(data).some(key => key !== 'CodeRedemption')) return false;
    const redemption = data.CodeRedemption;
    if (redemption == null) return true;
    if (!object(redemption) || Object.keys(redemption).some(key => key !== 'redeemCode')) return false;
    const result = redemption.redeemCode;
    if (result == null) return true;
    return object(result) && Object.keys(result).every(key => key === 'success') && (result.success == null || result.success === false);
  }
  function reason(json) {
    // Service errors can echo the key. Return a closed classification only.
    // Official error utilities also inspect nested serviceResponse. Search all
    // prefixed codes (including extensions), returning only a closed category.
    const codes = errorCodes(json);
    // A challenge or authentication error anywhere in the response must never
    // be converted into a quota signal by an earlier generic wrapper code.
    if (codes.some(code => codeReason(code) === 'requiresOfficial')) return 'requiresOfficial';
    if (codes.some(code => codeReason(code) === 'authRequired')) return 'authRequired';
    for (const raw of codes) {
      const category = codeReason(raw);
      if (category !== 'service') return category;
    }
    return 'service';
  }
  Object.defineProperty(window, '__umRedeemBatch', { value: async (id, operation, args) => {
    if (!/^[a-f0-9-]{36}$/.test(id) || !Object.hasOwn(queries, operation)) return;
    let answer = { id, ok: false, reason: 'unknown' };
    // Closed structural metadata only. Never copy response text, IDs, titles,
    // URLs, thrown errors, cookies, or key material into diagnostics.
    const diagnostic = { operation, stage: 'origin' };
    let retryAfterSeconds;
    const abort = new AbortController();
    const deadline = setTimeout(() => abort.abort(), 25000);
    try {
      if (location.origin !== 'https://store.epicgames.com') throw new Error('origin');
      const variables = operation === 'account' ? {} : operation === 'redeem'
        ? { codeId: args.code, codeUseId: args.codeUseId, source: 'DieselWebClient' }
        : operation === 'unlock' ? { codeId: args.code, codeUseId: args.codeUseId }
        : { codeId: args.code, locale: args.locale };
      const headers = { 'Content-Type': 'application/json', 'X-Requested-With': 'XMLHttpRequest' };
      diagnostic.stage = 'headers';
      const xsrf = document.cookie.match(/(?:^|;\s*)XSRF-TOKEN=([^;]*)/)?.[1];
      if (xsrf) headers['X-XSRF-TOKEN'] = decodeURIComponent(xsrf);
      diagnostic.stage = 'request';
      const response = await transport('/graphql', { method: 'POST', headers, credentials: 'include', cache: 'no-store', redirect: 'error', body: JSON.stringify({ query: queries[operation], variables }), signal: abort.signal });
      diagnostic.stage = 'http';
      if (Number.isInteger(response.status) && response.status >= 100 && response.status <= 599) diagnostic.httpStatus = response.status;
      const retryHeader = response.headers?.get?.('Retry-After');
      if (typeof retryHeader === 'string') {
        const value = retryHeader.trim();
        const seconds = /^\d{1,10}$/.test(value) ? Number(value) : Math.ceil((Date.parse(value) - Date.now()) / 1000);
        if (Number.isFinite(seconds) && seconds > 0) retryAfterSeconds = Math.min(4294967295, Math.ceil(seconds));
      }
      if (!response.ok) answer.reason = operation === 'redeem' ? 'unknown' : response.status === 401 ? 'authRequired' : response.status === 403 ? 'requiresOfficial' : response.status === 429 ? 'rateLimited' : 'service';
      else {
        diagnostic.stage = 'bodyShape';
        if (!response.body || typeof response.body.getReader !== 'function') throw new Error('body');
        const reader = response.body.getReader(), decoder = new TextDecoder(); let text = '', bytes = 0;
        diagnostic.stage = 'bodyRead';
        for (;;) { const part = await reader.read(); if (part.done) break; bytes += part.value.length; if (bytes > 65536) { diagnostic.stage = 'bodySize'; await reader.cancel(); throw new Error('size'); } text += decoder.decode(part.value, { stream: true }); }
        diagnostic.stage = 'json';
        text += decoder.decode(); const json = JSON.parse(text);
        const noErrors = json?.errors == null || (Array.isArray(json.errors) && json.errors.length === 0);
        diagnostic.hasErrors = !noErrors;
        const serviceCode = safeServiceCode(json, args.code);
        if (serviceCode) diagnostic.serviceCode = serviceCode;
        if (operation === 'account') {
          diagnostic.stage = 'accountShape';
          const account = json?.data?.Account?.myAccount;
          answer = account && /^[a-fA-F0-9]{32}$/.test(account.id) && noErrors
            ? { id, ok: true, accountId: account.id, accountName: clean(account.displayName) }
            : { id, ok: false, reason: 'authRequired' };
        } else {
          diagnostic.stage = 'resultShape';
          const field = { evaluate: 'evaluateCode', lock: 'lockCode', redeem: 'redeemCode', unlock: 'unlockCode' }[operation];
          const value = json?.data?.CodeRedemption?.[field];
          if (typeof value?.success === 'boolean') diagnostic.success = value.success;
          if (value?.success === true && noErrors) {
            answer = { id, ok: true };
            if (operation === 'evaluate' || operation === 'lock') {
              const p = value.data;
              diagnostic.stage = 'productShape';
              diagnostic.productKind = kind(p);
              diagnostic.eulaIdsKind = kind(p?.eulaIds);
              diagnostic.namespaceKind = kind(p?.namespace);
              diagnostic.entitlementKind = kind(p?.entitlementName);
              diagnostic.titleKind = kind(p?.title);
              diagnostic.codeUseIdKind = kind(p?.codeUseId);
              // The official client forwards success data without an array
              // requirement. An explicit null means no declared EULA IDs;
              // omitted, malformed, or nonempty declarations stay unsupported.
              if (!p || typeof p !== 'object' || Array.isArray(p) || !Object.hasOwn(p, 'eulaIds') || !(p.eulaIds === null || Array.isArray(p.eulaIds)) || (p.eulaIds?.length || 0) > 32) throw new Error('shape');
              // Product confirmation must compare the original full title.
              // Never turn distinct long/control-bearing names into one
              // truncated or stripped namespace/title fallback identity.
              if (typeof p.title !== 'string' || p.title.length === 0 || p.title.length > 240 || /[\u0000-\u001f\u007f-\u009f]/.test(p.title)) throw new Error('title');
              const title = p.title;
              const foldedCode = args.code.toUpperCase().replace(/-/g, '');
              diagnostic.stage = 'echo';
              if (!foldedCode || title.toUpperCase().replace(/-/g, '').includes(foldedCode)) throw new Error('echo');
              diagnostic.stage = 'identifier';
              if (!Object.hasOwn(p, 'entitlementName') || !(p.entitlementName === null || typeof p.entitlementName === 'string')) throw new Error('entitlement');
              answer.product = { namespace: identifier(p.namespace), offerId: identifier(p.offerId), entitlementName: identifier(p.entitlementName), entitlementNameIsNull: p.entitlementName === null, title, eulaIds: (p.eulaIds || []).map(identifier), codeUseId: identifier(p.codeUseId) };
            }
          } else if (value?.success === false) {
            const classified = reason(json);
            answer.reason = classified === 'service' ? operation === 'redeem' ? 'unknown' : 'requiresOfficial' : classified;
          } else { const classified = reason(json); answer.reason = operation === 'redeem' ? 'unknown' : classified === 'service' ? 'requiresOfficial' : classified; }
          if (operation === 'redeem') {
            if (response.status === 200 && exactThrottleRefusal(json)) answer = { id, ok: false, reason: 'rateLimited', throttleRefusal: true };
            else if (answer.reason === 'rateLimited') answer.reason = 'unknown';
          }
        }
      }
    } catch { answer = { id, ok: false, reason: operation === 'redeem' ? 'unknown' : ['request', 'bodyRead'].includes(diagnostic.stage) ? 'network' : 'requiresOfficial' }; }
    finally { clearTimeout(deadline); args = null; }
    answer.diagnostic = diagnostic;
    if (retryAfterSeconds !== undefined) answer.retryAfterSeconds = retryAfterSeconds;
    send(answer);
  }, writable: false, configurable: false });
  const ready = () => send({ id: 'boot', ok: true });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready, { once: true }); else ready();
})();

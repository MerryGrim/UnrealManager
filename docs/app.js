"use strict";

(() => {
  const root = document.documentElement;
  const localeSelect = document.querySelector("#language");
  const themeControl = document.querySelector("#theme");
  const themeButtons = themeControl.querySelectorAll("[data-theme-choice]");
  const translations = globalThis.UM_TRANSLATIONS;
  const attributes = { "data-i18n": null, "data-i18n-alt": "alt", "data-i18n-aria": "aria-label", "data-i18n-title": "title" };
  const english = translations.en;
  for (const [marker, attribute] of Object.entries(attributes)) {
    for (const element of document.querySelectorAll(`[${marker}]`)) {
      english[element.getAttribute(marker)] = attribute ? element.getAttribute(attribute) : element.textContent;
    }
  }
  // Storage can be unavailable in private browsing or restrictive browser settings.
  const read = (key) => { try { return localStorage.getItem(key); } catch { return null; } };
  const write = (key, value) => { try { localStorage.setItem(key, value); } catch { /* Preference still applies for this visit. */ } };
  const parameters = new URLSearchParams(location.search);
  const preferredLocale = parameters.get("lang") || read("um.site.language") || navigator.language.toLowerCase().split("-")[0];
  let locale = Object.hasOwn(translations, preferredLocale) ? preferredLocale : "en";
  let theme = parameters.get("theme") || read("um.site.theme") || "system";
  if (!["system", "light", "dark"].includes(theme)) theme = "system";
  const systemTheme = matchMedia("(prefers-color-scheme: dark)");

  function updateAddress() {
    const address = new URL(location.href);
    address.searchParams.set("lang", locale);
    if (theme === "system") address.searchParams.delete("theme");
    else address.searchParams.set("theme", theme);
    try { history.replaceState(null, "", address); } catch { /* Local file preview may restrict history. */ }
  }

  function applyLanguage() {
    const copy = translations[locale];
    root.lang = locale === "zh" ? "zh-CN" : locale;
    localeSelect.value = locale;
    for (const [marker, attribute] of Object.entries(attributes)) {
      for (const element of document.querySelectorAll(`[${marker}]`)) {
        const key = element.getAttribute(marker);
        const value = copy[key] ?? english[key];
        if (attribute) element.setAttribute(attribute, value);
        else element.textContent = value;
      }
    }
    localeSelect.setAttribute("aria-label", copy.language ?? english.language);
    themeControl.setAttribute("aria-label", copy.theme ?? english.theme);
    document.title = copy.metaTitle;
    document.querySelector('meta[name="description"]').content = copy.metaDescription;
    document.querySelector('meta[property="og:title"]').content = copy.metaTitle;
    document.querySelector('meta[property="og:description"]').content = copy.metaDescription;
  }

  function applyTheme() {
    const resolvedTheme = theme === "system" ? (systemTheme.matches ? "dark" : "light") : theme;
    root.dataset.theme = resolvedTheme;
    for (const button of themeButtons) button.setAttribute("aria-pressed", String(button.dataset.themeChoice === theme));
    document.querySelector('meta[name="theme-color"]').content = resolvedTheme === "dark" ? "#101820" : "#f5f6f2";
  }

  localeSelect.addEventListener("change", () => {
    locale = localeSelect.value;
    write("um.site.language", locale);
    applyLanguage();
    updateAddress();
  });
  for (const button of themeButtons) {
    button.addEventListener("click", () => {
      theme = button.dataset.themeChoice;
      write("um.site.theme", theme);
      applyTheme();
      updateAddress();
    });
  }
  systemTheme.addEventListener("change", () => { if (theme === "system") applyTheme(); });
  applyLanguage();
  applyTheme();
})();

// Injects a language picker into the mdBook menu bar.
// Languages are inferred from the deployed site layout: English at the root,
// translations under /<lang-code>/. Add new languages to LANGS to expose them.
(function () {
  var LANGS = [
    { code: "en", label: "English" },
    { code: "de", label: "Deutsch" },
    { code: "ja", label: "日本語" },
    { code: "ko", label: "한국어" },
    { code: "zh-CN", label: "简体中文" },
  ];

  function detect() {
    var path = window.location.pathname;
    var codes = LANGS.map(function (l) { return l.code; }).filter(function (c) { return c !== "en"; });
    for (var i = 0; i < codes.length; i++) {
      var prefix = "/" + codes[i];
      if (path === prefix || path.indexOf(prefix + "/") === 0) {
        return { current: codes[i], rest: path.slice(prefix.length) || "/" };
      }
    }
    return { current: "en", rest: path };
  }

  function pathFor(code, rest) {
    if (code === "en") return rest;
    var trimmed = rest.startsWith("/") ? rest : "/" + rest;
    return "/" + code + trimmed;
  }

  function build() {
    var bar = document.querySelector(".right-buttons");
    if (!bar || document.getElementById("language-switcher")) return;
    var state = detect();

    var wrap = document.createElement("div");
    wrap.id = "language-switcher";
    wrap.style.cssText = "display:inline-block;margin:0 8px;";

    var sel = document.createElement("select");
    sel.setAttribute("aria-label", "Language");
    sel.style.cssText =
      "background:transparent;color:inherit;border:1px solid var(--sidebar-bg,#ccc);" +
      "border-radius:4px;padding:2px 6px;font:inherit;cursor:pointer;";

    LANGS.forEach(function (l) {
      var opt = document.createElement("option");
      opt.value = l.code;
      opt.textContent = l.label;
      if (l.code === state.current) opt.selected = true;
      sel.appendChild(opt);
    });

    sel.addEventListener("change", function () {
      window.location.pathname = pathFor(sel.value, state.rest);
    });

    wrap.appendChild(sel);
    bar.insertBefore(wrap, bar.firstChild);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", build);
  } else {
    build();
  }
})();

/*
 * Load the ONLYOFFICE plugin SDK (plugins.js) from a local copy when one
 * exists, so LavaTeX works offline, then load LavaTeX itself.
 *
 * Tried in order:
 *  1. ../v1/ next to this plugin (Document Server and built-in plugins);
 *  2. the SDK shipped with ONLYOFFICE Desktop, next to the editor that hosts
 *     this plugin (user-installed plugins live outside the app folder);
 *  3. ONLYOFFICE's website.
 *
 * The SDK starts its handshake with the editor from window.onload; when it is
 * loaded after the page has finished loading, that handler is called here.
 */
(function (window, document) {
  "use strict";

  var SDK_FILE = "plugins.js";
  var APP_SCRIPTS = ["scripts/lavatex-core.js", "scripts/plugin.js"];
  var pageLoaded = false;
  window.addEventListener("load", function () {
    pageLoaded = true;
  });

  function sdkBases() {
    var bases = ["../v1/"];
    try {
      var editor = window.parent.location.href;
      var m = /^(.*\/editors\/)web-apps\//.exec(editor);
      if (m) bases.push(m[1] + "sdkjs-plugins/v1/");
    } catch (e) {
      /* cross-origin editor: not ONLYOFFICE Desktop */
    }
    bases.push("https://onlyoffice.github.io/sdkjs-plugins/v1/");
    return bases;
  }

  function load(src, done) {
    var s = document.createElement("script");
    s.src = src;
    s.onload = function () {
      done(true);
    };
    s.onerror = function () {
      s.parentNode.removeChild(s);
      done(false);
    };
    document.head.appendChild(s);
  }

  function loadInOrder(list, done) {
    if (!list.length) return done();
    load(list[0], function () {
      loadInOrder(list.slice(1), done);
    });
  }

  function start(base) {
    window.LAVATEX_SDK_BASE = base;
    // plugins-ui.js is optional styling support; ignore if missing.
    load(base + "plugins-ui.js", function () {
      loadInOrder(APP_SCRIPTS, function () {
        if (pageLoaded && typeof window.onload === "function") window.onload();
      });
    });
  }

  function tryBases(bases) {
    if (!bases.length) {
      document.body.innerHTML =
        '<p style="font:13px sans-serif;padding:16px">LavaTeX could not load the ONLYOFFICE plugin SDK. ' +
        "Please update ONLYOFFICE, or connect to the internet once and reopen LavaTeX.</p>";
      return;
    }
    load(bases[0] + SDK_FILE, function (ok) {
      if (ok && window.Asc && window.Asc.plugin) start(bases[0]);
      else tryBases(bases.slice(1));
    });
  }

  tryBases(sdkBases());
})(window, document);

/*
 * LavaTeX for the ONLYOFFICE Presentation Editor.
 *
 * Rendering is done by the local `lavatex serve` helper (the user's own TeX).
 * The equation is inserted as an ordinary picture whose description (alt
 * text) carries the LaTeX source, so it stays a normal image everywhere and
 * becomes editable again whenever LavaTeX is available.
 */
(function (window) {
  "use strict";

  var LT = window.LavaTeX;
  var STORAGE_KEY = "lavatex.onlyoffice.settings";
  var DEFAULTS = {
    helperUrl: LT.DEFAULT_HELPER,
    token: "",
    mode: "display",
    fontSize: 20,
    color: "#000000",
    format: "svg",
    pngDpi: 300,
    preamble: "\\usepackage{amsmath}\n\\usepackage{amssymb}",
    toolchain: "",
    engine: "",
  };

  var state = {
    settings: null,
    helper: null,
    connected: false,
    editing: null, // { id, record, widthEmu }
    result: null,
    resultKey: null,
    pending: null,
    seq: 0,
    timer: null,
  };

  function $(id) {
    return document.getElementById(id);
  }

  // --- settings ----------------------------------------------------------

  function loadSettings() {
    var s = {};
    try {
      s = JSON.parse(window.localStorage.getItem(STORAGE_KEY) || "{}") || {};
    } catch (e) {
      s = {};
    }
    var out = {};
    Object.keys(DEFAULTS).forEach(function (k) {
      out[k] = s[k] !== undefined ? s[k] : DEFAULTS[k];
    });
    return out;
  }

  function saveSettings() {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state.settings));
    } catch (e) {
      /* storage unavailable: settings last for this session only */
    }
  }

  // --- UI helpers --------------------------------------------------------

  function setStatus(text, kind, details) {
    var el = $("status");
    el.textContent = text || "";
    el.className = "status" + (kind ? " " + kind : "");
    $("details").textContent = details || "";
    $("detailsBox").hidden = !details;
  }

  function setBanner(text) {
    $("banner").textContent = text || "";
    $("banner").hidden = !text;
  }

  function showSetup(show, message) {
    $("setup").hidden = !show;
    $("editor").hidden = show;
    $("helperUrl").value = state.settings.helperUrl;
    $("helperToken").value = state.settings.token;
    if (message) setBanner(message);
  }

  function formValues() {
    return {
      source: $("source").value,
      preamble: $("preamble").value,
      mode: $("mode").value,
      fontSize: parseFloat($("fontSize").value) || 20,
      color: $("color").value || "#000000",
      format: $("format").value,
      pngDpi: parseInt($("pngDpi").value, 10) || 300,
      toolchain: $("toolchain").value,
      engine: $("engine").value,
    };
  }

  function applyValues(v) {
    if (v.source !== undefined) $("source").value = v.source;
    $("preamble").value = v.preamble;
    $("mode").value = v.mode;
    $("fontSize").value = v.fontSize;
    $("color").value = /^#[0-9a-fA-F]{6}$/.test(v.color) ? v.color : "#000000";
    // ONLYOFFICE cannot place PDF pictures; SVG is the vector equivalent.
    $("format").value = v.format === "png" ? "png" : "svg";
    $("pngDpi").value = v.pngDpi;
    $("engine").value = v.engine || "";
    updateDpiVisibility();
  }

  function updateDpiVisibility() {
    $("dpiLabel").style.visibility = $("format").value === "png" ? "visible" : "hidden";
  }

  function fillToolchains(list, chosen) {
    var sel = $("toolchain");
    sel.innerHTML = "";
    var auto = document.createElement("option");
    auto.value = "";
    auto.textContent = "Automatic (" + (list.length ? list[0].label : "none found") + ")";
    sel.appendChild(auto);
    list.forEach(function (tc) {
      var o = document.createElement("option");
      o.value = tc.id;
      o.textContent = tc.label + " \u2013 " + tc.binDir;
      sel.appendChild(o);
    });
    sel.value = chosen || "";
    if (sel.value !== (chosen || "")) sel.value = "";
  }

  // --- helper connection -------------------------------------------------

  /** The pairing file written next to the plugin by `lavatex setup onlyoffice`. */
  function readPairing() {
    return new Promise(function (resolve) {
      try {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", "lavatex-helper.json?" + Date.now(), true);
        xhr.onload = function () {
          // file:// answers with status 0 in ONLYOFFICE Desktop.
          if (xhr.status !== 200 && xhr.status !== 0) return resolve(null);
          try {
            var p = JSON.parse(xhr.responseText);
            resolve(p && p.url && p.token ? p : null);
          } catch (e) {
            resolve(null);
          }
        };
        xhr.onerror = function () {
          resolve(null);
        };
        xhr.send();
      } catch (e) {
        resolve(null);
      }
    });
  }

  /** Try one helper address and token; resolves with the toolchain list. */
  function tryHelper(url, token) {
    var helper = new LT.Helper(url, token);
    return helper
      .health()
      .catch(function () {
        throw new Error("unreachable");
      })
      .then(function () {
        return helper.toolchains();
      })
      .then(function (res) {
        if (res.httpStatus === 401) throw new Error("token");
        if (!res.ok) throw new Error("helper");
        return { helper: helper, url: url, token: token, toolchains: res.toolchains || [] };
      });
  }

  function connect(manual) {
    $("helperInfo").textContent = state.settings.helperUrl;
    return readPairing().then(function (pairing) {
      // A paired install (lavatex setup onlyoffice) is authoritative; a
      // manually entered helper is tried first only when the user asks.
      var attempts = [];
      var own = { url: state.settings.helperUrl, token: state.settings.token };
      if (manual || !pairing) attempts.push(own);
      if (pairing) attempts.push({ url: pairing.url, token: pairing.token });
      if (!manual && pairing && own.token && own.token !== pairing.token) attempts.push(own);

      var lastError = null;
      var chain = Promise.reject(new Error("start"));
      attempts.forEach(function (a) {
        chain = chain.catch(function (err) {
          if (err.message !== "start") lastError = err;
          return tryHelper(a.url, a.token);
        });
      });
      return chain
        .then(function (ok) {
          state.helper = ok.helper;
          state.connected = true;
          state.settings.helperUrl = ok.url;
          state.settings.token = ok.token;
          saveSettings();
          showSetup(false);
          setBanner(state.editingNote || "");
          fillToolchains(ok.toolchains, state.settings.toolchain);
          $("helperInfo").textContent = "Connected to " + ok.url + (pairing ? " (set up by lavatex setup)" : "");
          if (!ok.toolchains.length) {
            setStatus("No TeX installation was found on this computer. Install TeX Live, MiKTeX or MacTeX, then reopen LavaTeX.", "error");
          }
          schedulePreview(0);
        })
        .catch(function (err) {
          state.connected = false;
          var reason = (err.message === "start" ? lastError : err) || err;
          var msg;
          if (pairing && reason.message === "unreachable") {
            msg = "The LavaTeX helper is not running. It normally starts by itself; run \u201Clavatex setup onlyoffice\u201D again to repair it, then click Try again.";
          } else if (pairing || reason.message === "token") {
            msg = "The LavaTeX helper did not accept this plugin. Run \u201Clavatex setup onlyoffice\u201D again, then click Try again.";
          } else if (manual) {
            msg = "Could not connect to " + state.settings.helperUrl + ".";
          } else {
            msg = "";
          }
          $("setupTitle").textContent = pairing ? "LavaTeX helper not available" : "One-time setup needed";
          showSetup(true, msg);
        });
    });
  }

  // --- rendering -----------------------------------------------------------

  function requestFrom(v) {
    var req = {
      source: v.source,
      preamble: v.preamble,
      mode: v.mode,
      fontSize: v.fontSize,
      color: v.color,
      format: v.format,
      pngDpi: v.pngDpi,
    };
    if (v.toolchain) req.toolchain = v.toolchain;
    if (v.engine) req.engine = v.engine;
    return req;
  }

  function render() {
    var req = requestFrom(formValues());
    var key = JSON.stringify(req);
    if (key === state.resultKey && state.result) return Promise.resolve(state.result);
    if (!req.source.trim()) {
      $("preview").innerHTML = '<span class="placeholder">Type LaTeX above to see a preview.</span>';
      setStatus("");
      return Promise.resolve(null);
    }
    if (state.pending) state.pending.abort();
    var controller = typeof AbortController === "function" ? new AbortController() : null;
    state.pending = controller;
    var seq = ++state.seq;
    setStatus("Rendering\u2026");
    return state.helper
      .render(req, controller && controller.signal)
      .then(function (res) {
        if (seq !== state.seq) return null; // a newer render superseded this one
        if (!res.ok) {
          var e = res.error || {};
          var where = e.line ? " (" + (e.location || "source") + " line " + e.line + ")" : "";
          setStatus((e.summary || "Rendering failed.") + where, "error", e.details);
          if (e.location === "source" && e.line) selectLine($("source"), e.line);
          $("preview").classList.add("stale");
          state.result = null;
          state.resultKey = null;
          return null;
        }
        state.result = res;
        state.resultKey = key;
        var img = document.createElement("img");
        img.alt = req.source;
        img.src = LT.dataUrl(res);
        // Show at natural size (96 px per inch), scaled down only if needed.
        img.style.width = Math.round((res.widthBp * 96) / 72) + "px";
        $("preview").innerHTML = "";
        $("preview").classList.remove("stale");
        $("preview").appendChild(img);
        var note = (res.warnings || []).join(" ");
        setStatus(
          Math.round(res.widthBp) + " \u00D7 " + Math.round(res.heightBp) + " pt \u00B7 " + res.engine + " via " + res.toolchain + (note ? ". " + note : ""),
          "ok"
        );
        return res;
      })
      .catch(function (err) {
        if (err && err.name === "AbortError") return null;
        setStatus("Lost connection to the LavaTeX helper.", "error", String(err));
        return null;
      });
  }

  function schedulePreview(delay) {
    clearTimeout(state.timer);
    state.timer = setTimeout(function () {
      if (state.connected) render();
    }, delay === undefined ? 500 : delay);
  }

  function selectLine(textarea, line) {
    var lines = textarea.value.split("\n");
    if (line < 1 || line > lines.length) return;
    var start = 0;
    for (var i = 0; i < line - 1; i++) start += lines[i].length + 1;
    textarea.focus();
    textarea.setSelectionRange(start, start + lines[line - 1].length);
  }

  // --- editor interaction ------------------------------------------------

  /** Read the selected picture (if exactly one) and its LavaTeX record. */
  function readSelection() {
    return new Promise(function (resolve) {
      try {
        window.Asc.plugin.callCommand(
          function () {
            if (typeof Api.GetSelection !== "function") return null;
            var sel = Api.GetSelection();
            if (!sel || sel.IsEmpty()) return null;
            var shapes = sel.GetShapes();
            if (!shapes || shapes.length !== 1) return null;
            var d = shapes[0];
            if (d.GetClassType() !== "image") return null;
            return {
              id: d.GetInternalId(),
              description: typeof d.GetDescription === "function" ? d.GetDescription() : null,
              widthEmu: d.GetWidth(),
            };
          },
          false,
          false,
          function (info) {
            resolve(info || null);
          }
        );
      } catch (e) {
        resolve(null);
      }
    });
  }

  function insert() {
    if (!state.connected) {
      setStatus("Connect to the LavaTeX helper first.", "error");
      return;
    }
    // A pending preview would start a newer render and supersede this one.
    clearTimeout(state.timer);
    render().then(function (res) {
      if (!res) return;
      var size = LT.sizeEmu(res, state.editing);
      window.Asc.scope.lavatex = {
        url: LT.dataUrl(res),
        width: size.width,
        height: size.height,
        alt: res.altText,
        editId: state.editing ? state.editing.id : null,
      };
      window.Asc.plugin.callCommand(
        function () {
          var s = Asc.scope.lavatex;
          var slide = Api.GetPresentation().GetCurrentSlide();
          if (!slide) return false;
          var img = Api.CreateImage(s.url, s.width, s.height);
          var old = s.editId ? Api.GetByInternalId(s.editId) : null;
          if (old) {
            img.SetPosition(old.GetPosX(), old.GetPosY());
            if (typeof old.GetRotation === "function" && typeof img.SetRotation === "function") {
              img.SetRotation(old.GetRotation());
            }
          } else {
            img.SetPosition(Math.max(0, (slide.GetWidth() - s.width) / 2), Math.max(0, (slide.GetHeight() - s.height) / 2));
          }
          slide.AddObject(img);
          if (typeof img.SetDescription === "function") img.SetDescription(s.alt);
          if (typeof img.SetTitle === "function") img.SetTitle("LaTeX equation");
          if (typeof img.SetName === "function") img.SetName("LavaTeX equation");
          if (old) old.Delete();
          if (typeof img.Select === "function") img.Select();
          return typeof img.SetDescription === "function";
        },
        false,
        true,
        function (savedSource) {
          var v = formValues();
          Object.keys(DEFAULTS).forEach(function (k) {
            if (v[k] !== undefined && k !== "source") state.settings[k] = v[k];
          });
          saveSettings();
          if (savedSource === false) {
            // Older editor (< 10.0): the picture is fine, but cannot carry its source.
            window.alert("The equation was inserted, but this ONLYOFFICE version cannot store its LaTeX source, so it will not be editable later. ONLYOFFICE 10.0 or newer is needed for editing.");
          }
          window.Asc.plugin.executeCommand("close", "");
        }
      );
    });
  }

  // --- plugin lifecycle --------------------------------------------------

  window.Asc.plugin.init = function () {
    state.settings = loadSettings();
    applyValues(state.settings);

    ["source", "preamble", "mode", "fontSize", "color", "format", "pngDpi", "toolchain", "engine"].forEach(function (id) {
      $(id).addEventListener(id === "source" || id === "preamble" ? "input" : "change", function () {
        updateDpiVisibility();
        schedulePreview();
      });
    });
    $("source").addEventListener("keydown", function (e) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        insert();
      }
    });
    $("connect").addEventListener("click", function () {
      state.settings.helperUrl = $("helperUrl").value.trim() || LT.DEFAULT_HELPER;
      state.settings.token = $("helperToken").value.trim();
      saveSettings();
      setBanner("");
      connect(true);
    });
    $("retry").addEventListener("click", function () {
      setBanner("");
      connect(false);
    });
    $("changeHelper").addEventListener("click", function () {
      showSetup(true, "");
    });

    readSelection().then(function (info) {
      var record = info ? LT.fromAltText(info.description) : null;
      if (record) {
        state.editing = { id: info.id, record: record, widthEmu: info.widthEmu };
        applyValues({
          source: record.source,
          preamble: record.preamble,
          mode: record.mode,
          fontSize: record.fontSize,
          color: record.color,
          format: record.format,
          pngDpi: state.settings.pngDpi,
          engine: record.engine,
        });
        var notes = ["Editing the selected equation."];
        if (record.preamble !== state.settings.preamble) {
          notes.push("It carries its own preamble from the document; review it if the file came from someone else.");
        }
        state.editingNote = notes.join(" ");
        setBanner(state.editingNote);
      }
      $("source").focus();
      connect();
    });
  };

  window.Asc.plugin.button = function (id) {
    if (id === 0) {
      insert();
    } else {
      window.Asc.plugin.executeCommand("close", "");
    }
  };

  window.Asc.plugin.onThemeChanged = function (theme) {
    if (typeof window.Asc.plugin.onThemeChangedBase === "function") {
      window.Asc.plugin.onThemeChangedBase(theme);
    }
    var dark = theme && (theme.type === "dark" || /dark/.test(theme.name || ""));
    document.body.classList.toggle("theme-dark", !!dark);
  };
})(window);

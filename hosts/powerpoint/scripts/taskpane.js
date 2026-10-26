/*
 * LavaTeX task pane for PowerPoint.
 *
 * The page is served by the local LavaTeX helper (https://localhost:7355),
 * which also renders, so it talks to its own origin with the access token
 * the helper wrote into the page.
 */
(function (window) {
  "use strict";

  var LT = window.LavaTeX;
  var STORAGE_KEY = "lavatex.powerpoint.settings";
  var DEFAULTS = {
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
    ppt: null,
    editing: null, // readSelection() result for the equation being edited
    dirty: false, // the source was changed since it was loaded
    result: null,
    resultKey: null,
    pending: null,
    seq: 0,
    timer: null,
    busy: false,
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
    $("status").textContent = text || "";
    $("status").className = "status" + (kind ? " " + kind : "");
    $("details").textContent = details || "";
    $("detailsBox").hidden = !details;
  }

  function setBanner(text, actionLabel, action) {
    $("bannerText").textContent = text || "";
    $("banner").hidden = !text;
    var btn = $("bannerAction");
    btn.hidden = !actionLabel;
    btn.textContent = actionLabel || "";
    btn.onclick = action || null;
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
    $("preamble").value = v.preamble || "";
    $("mode").value = v.mode || "display";
    $("fontSize").value = v.fontSize || 20;
    $("color").value = /^#[0-9a-fA-F]{6}$/.test(v.color || "") ? v.color : "#000000";
    // PowerPoint cannot place PDF pictures; SVG is the vector equivalent.
    $("format").value = v.format === "png" ? "png" : "svg";
    $("pngDpi").value = v.pngDpi || 300;
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

  function showMode() {
    var editing = !!state.editing;
    $("modeTitle").textContent = editing ? "Edit equation" : "New equation";
    $("insert").textContent = editing ? "Update" : "Insert";
    $("newEquation").hidden = !editing;
  }

  // --- editing state -----------------------------------------------------

  function startNew(keepSource) {
    state.editing = null;
    var v = {};
    Object.keys(state.settings).forEach(function (k) {
      v[k] = state.settings[k];
    });
    if (!keepSource) v.source = "";
    applyValues(v);
    state.dirty = false;
    setBanner("");
    showMode();
    schedulePreview(0);
  }

  function startEditing(sel) {
    var r = sel.record;
    state.editing = sel;
    applyValues({
      source: r.source,
      preamble: r.preamble,
      mode: r.mode,
      fontSize: r.fontSize,
      color: r.color,
      format: r.format,
      pngDpi: state.settings.pngDpi,
      engine: r.engine,
    });
    state.dirty = false;
    if ((r.preamble || "") !== state.settings.preamble) {
      setBanner("This equation has its own preamble; review it if the file came from someone else.");
    } else {
      setBanner("");
    }
    showMode();
    schedulePreview(0);
  }

  /** Follow the PowerPoint selection without losing unsaved typing. */
  function onSelectionChanged() {
    if (state.busy) return;
    state.ppt.readSelection().then(function (sel) {
      if (!sel.record) return;
      if (state.editing && state.editing.shape.id === sel.shape.id) return;
      if (!state.dirty) {
        startEditing(sel);
      } else {
        setBanner("Another equation is selected.", "Edit it instead", function () {
          startEditing(sel);
        });
      }
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

  function selectLine(textarea, line) {
    var lines = textarea.value.split("\n");
    if (line < 1 || line > lines.length) return;
    var start = 0;
    for (var i = 0; i < line - 1; i++) start += lines[i].length + 1;
    textarea.focus();
    textarea.setSelectionRange(start, start + lines[line - 1].length);
  }

  function render() {
    var req = requestFrom(formValues());
    var key = JSON.stringify(req);
    if (key === state.resultKey && state.result) return Promise.resolve(state.result);
    if (!req.source.trim()) {
      $("preview").innerHTML = '<span class="placeholder">Type LaTeX above to see a preview.</span>';
      $("preview").classList.remove("stale");
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
        if (res.httpStatus === 401) {
          setStatus("The LavaTeX helper did not accept this add-in. Run \u201Clavatex setup powerpoint\u201D again and restart PowerPoint.", "error");
          return null;
        }
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
        setStatus("Lost connection to the LavaTeX helper. Check that it is running (lavatex setup powerpoint starts it at login).", "error", String(err));
        return null;
      });
  }

  function schedulePreview(delay) {
    clearTimeout(state.timer);
    state.timer = setTimeout(render, delay === undefined ? 500 : delay);
  }

  // --- insert / update -----------------------------------------------------

  function rememberSettings() {
    var v = formValues();
    Object.keys(DEFAULTS).forEach(function (k) {
      if (v[k] !== undefined) state.settings[k] = v[k];
    });
    saveSettings();
  }

  function insert() {
    if (state.busy) return;
    state.busy = true;
    $("insert").disabled = true;
    // A pending preview would start a newer render and supersede this one.
    clearTimeout(state.timer);
    render()
      .then(function (res) {
        if (!res) return null;
        setStatus(state.editing ? "Updating the equation\u2026" : "Inserting\u2026");
        var wasEditing = !!state.editing;
        return state.ppt.place(res, state.editing).then(function (placed) {
          rememberSettings();
          // Keep editing the picture just placed.
          return state.ppt.readSelection().then(function (sel) {
            if (sel.record) {
              state.editing = sel;
              state.dirty = false;
              showMode();
            }
            var note = placed.altText ? "" : " This PowerPoint cannot set alt text; the LaTeX source is stored in the picture's tags.";
            setStatus((wasEditing ? "Equation updated." : "Equation inserted.") + note, "ok");
          });
        });
      })
      .catch(function (err) {
        setStatus(err.message || String(err), "error");
      })
      .then(function () {
        state.busy = false;
        $("insert").disabled = false;
      });
  }

  // --- start -------------------------------------------------------------

  function action() {
    var m = /[?&]action=([a-z]+)/.exec(window.location.search);
    return m ? m[1] : "new";
  }

  function init() {
    state.settings = loadSettings();
    state.ppt = window.LavaTeXPowerPoint;
    var meta = document.querySelector('meta[name="lavatex-token"]');
    state.helper = new LT.Helper(window.location.origin, meta ? meta.content : "");
    applyValues(state.settings);
    showMode();

    ["source", "preamble", "mode", "fontSize", "color", "format", "pngDpi", "toolchain", "engine"].forEach(function (id) {
      $(id).addEventListener(id === "source" || id === "preamble" ? "input" : "change", function () {
        if (id === "source" || id === "preamble") state.dirty = true;
        updateDpiVisibility();
        schedulePreview();
      });
    });
    ["format", "pngDpi", "toolchain", "engine"].forEach(function (id) {
      $(id).addEventListener("change", rememberSettings);
    });
    $("source").addEventListener("keydown", function (e) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        insert();
      }
    });
    $("insert").addEventListener("click", insert);
    $("newEquation").addEventListener("click", function () {
      startNew(false);
      $("source").focus();
    });

    var act = action();
    if (act === "settings") $("settingsBox").open = true;

    state.helper
      .toolchains()
      .then(function (res) {
        var list = (res && res.toolchains) || [];
        fillToolchains(list, state.settings.toolchain);
        $("helperInfo").textContent = "Equations are rendered by the LavaTeX helper on this computer.";
        if (!list.length) {
          setStatus("No TeX installation was found on this computer. Install TeX Live, MiKTeX or MacTeX, then reopen LavaTeX.", "error");
        }
      })
      .catch(function () {
        setStatus("Cannot reach the LavaTeX helper.", "error");
      });

    var caps = state.ppt.capabilities();
    if (!caps.selection) {
      setBanner("This PowerPoint is too old for LavaTeX to insert equations (PowerPoint API 1.5 or newer is needed). Update Office.");
    }

    state.ppt
      .readSelection()
      .catch(function () {
        return { count: 0, record: null };
      })
      .then(function (sel) {
        if (sel.record && act !== "new") {
          startEditing(sel);
        } else {
          startNew(false);
          if (act === "edit") {
            setBanner(sel.count ? "The selected object is not a LavaTeX equation. Select an equation, or type a new one." : "Select an equation on the slide to edit it, or type a new one.");
          }
        }
        $("source").focus();
      });

    try {
      window.Office.context.document.addHandlerAsync(window.Office.EventType.DocumentSelectionChanged, onSelectionChanged);
    } catch (e) {
      /* no selection events: the pane still works on its initial selection */
    }
  }

  /** Run `fn` once the page's elements exist (Office may be ready earlier). */
  function whenDomReady(fn) {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", fn);
    } else {
      fn();
    }
  }

  window.Office.onReady(function (info) {
    whenDomReady(function () {
      if (info && info.host && info.host !== window.Office.HostType.PowerPoint) {
        setBanner("LavaTeX works in PowerPoint only.");
        return;
      }
      init();
    });
  });
})(window);

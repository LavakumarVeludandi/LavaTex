/*
 * LavaTeX editor window (used by the PowerPoint add-in on Windows).
 *
 * The page runs inside the `lavatex-helper editor` window. It asks the
 * program to render through window.ipc.postMessage and gets answers through
 * window.__lavatexReply(id, data). Starting values are in window.__lavatexInit.
 */
(function (window) {
  "use strict";

  var LT = window.LavaTeX;
  var state = { seq: 0, timer: null, busy: false, pending: {}, nextId: 1, renderId: 0 };

  function $(id) {
    return document.getElementById(id);
  }

  // --- talking to the program ------------------------------------------------

  function call(cmd, data) {
    return new Promise(function (resolve) {
      var id = state.nextId++;
      state.pending[id] = resolve;
      var msg = { id: id, cmd: cmd };
      Object.keys(data || {}).forEach(function (k) {
        msg[k] = data[k];
      });
      window.ipc.postMessage(JSON.stringify(msg));
    });
  }

  window.__lavatexReply = function (id, data) {
    var resolve = state.pending[id];
    delete state.pending[id];
    if (resolve) resolve(data);
  };

  // --- form ------------------------------------------------------------------

  function setStatus(text, kind, details) {
    $("status").textContent = text || "";
    $("status").className = "status" + (kind ? " " + kind : "");
    $("details").textContent = details || "";
    $("detailsBox").hidden = !details;
  }

  function values() {
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

  function apply(v) {
    $("source").value = v.source || "";
    $("preamble").value = v.preamble || "";
    $("mode").value = v.mode || "display";
    $("fontSize").value = v.fontSize || 20;
    $("color").value = /^#[0-9a-fA-F]{6}$/.test(v.color || "") ? v.color : "#000000";
    $("format").value = v.format === "png" ? "png" : "svg";
    $("pngDpi").value = v.pngDpi || 300;
    $("engine").value = v.engine || "";
    state.wantedToolchain = v.toolchain || "";
    updateDpi();
  }

  function updateDpi() {
    $("dpiLabel").style.visibility = $("format").value === "png" ? "visible" : "hidden";
  }

  function fillToolchains(list) {
    var sel = $("toolchain");
    sel.innerHTML = "";
    var auto = document.createElement("option");
    auto.value = "";
    auto.textContent = "Automatic (" + (list.length ? list[0].label : "none found") + ")";
    sel.appendChild(auto);
    list.forEach(function (tc) {
      var o = document.createElement("option");
      o.value = tc.id;
      o.textContent = tc.label + " – " + tc.binDir;
      sel.appendChild(o);
    });
    sel.value = state.wantedToolchain || "";
    if (sel.value !== (state.wantedToolchain || "")) sel.value = "";
  }

  function selectLine(textarea, line) {
    var lines = textarea.value.split("\n");
    if (line < 1 || line > lines.length) return;
    var start = 0;
    for (var i = 0; i < line - 1; i++) start += lines[i].length + 1;
    textarea.focus();
    textarea.setSelectionRange(start, start + lines[line - 1].length);
  }

  function showError(e) {
    e = e || {};
    var where = e.line ? " (" + (e.location || "source") + " line " + e.line + ")" : "";
    setStatus((e.summary || "Rendering failed.") + where, "error", e.details);
    if (e.location === "source" && e.line) selectLine($("source"), e.line);
    $("preview").classList.add("stale");
  }

  // --- preview -----------------------------------------------------------------

  function preview() {
    var v = values();
    if (!v.source.trim()) {
      $("preview").innerHTML = '<span class="placeholder">Type LaTeX above to see a preview.</span>';
      $("preview").classList.remove("stale");
      setStatus("");
      return;
    }
    var seq = ++state.seq;
    setStatus("Rendering…");
    call("render", { values: v }).then(function (res) {
      if (seq !== state.seq) return; // superseded by newer typing
      if (!res.ok) return showError(res.error);
      var img = document.createElement("img");
      img.alt = v.source;
      img.src = LT.dataUrl(res);
      img.style.width = Math.round((res.widthBp * 96) / 72) + "px";
      $("preview").innerHTML = "";
      $("preview").classList.remove("stale");
      $("preview").appendChild(img);
      var note = (res.warnings || []).join(" ");
      setStatus(
        Math.round(res.widthBp) + " × " + Math.round(res.heightBp) + " pt · " + res.engine + " via " + res.toolchain + (note ? ". " + note : ""),
        "ok"
      );
    });
  }

  function schedulePreview(delay) {
    clearTimeout(state.timer);
    state.timer = setTimeout(preview, delay === undefined ? 400 : delay);
  }

  // --- insert / cancel ------------------------------------------------------

  function insert() {
    if (state.busy) return;
    if (!$("source").value.trim()) {
      setStatus("Type some LaTeX first.", "error");
      return;
    }
    clearTimeout(state.timer);
    state.seq++; // ignore previews still on their way
    state.busy = true;
    $("insert").disabled = true;
    setStatus("Rendering…");
    // On success the program closes the window.
    call("insert", { values: values() }).then(function (res) {
      state.busy = false;
      $("insert").disabled = false;
      if (res && !res.ok) showError(res.error);
    });
  }

  function cancel() {
    call("cancel");
  }

  // --- start -------------------------------------------------------------------

  function init() {
    var initial = window.__lavatexInit || { editing: false, values: {} };
    apply(initial.values || {});
    $("insert").textContent = initial.editing ? "Update" : "Insert";
    if (initial.note) {
      $("banner").textContent = initial.note;
      $("banner").hidden = false;
    }
    ["source", "preamble", "mode", "fontSize", "color", "format", "pngDpi", "toolchain", "engine"].forEach(function (id) {
      $(id).addEventListener(id === "source" || id === "preamble" ? "input" : "change", function () {
        updateDpi();
        schedulePreview();
      });
    });
    $("insert").addEventListener("click", insert);
    $("cancel").addEventListener("click", cancel);
    document.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        insert();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      }
    });
    call("toolchains").then(function (res) {
      var list = (res && res.toolchains) || [];
      fillToolchains(list);
      if (!list.length) {
        setStatus("No TeX installation was found on this computer. Install TeX Live, MiKTeX or MacTeX, then try again.", "error");
      }
    });
    var src = $("source");
    src.focus();
    src.setSelectionRange(src.value.length, src.value.length);
    schedulePreview(0);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})(window);

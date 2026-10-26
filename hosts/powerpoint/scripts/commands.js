/*
 * Ribbon commands without a task pane: Regenerate and Help.
 */
(function (window) {
  "use strict";

  var LT = window.LavaTeX;
  var HELP_URL = "https://github.com/LavakumarVeludandi/LavaTex/blob/master/docs/usage.md";

  function settings() {
    try {
      return JSON.parse(window.localStorage.getItem("lavatex.powerpoint.settings") || "{}") || {};
    } catch (e) {
      return {};
    }
  }

  function helper() {
    var meta = document.querySelector('meta[name="lavatex-token"]');
    return new LT.Helper(window.location.origin, meta ? meta.content : "");
  }

  /** Show a message in a small dialog; resolves when it is closed. */
  function showMessage(text) {
    return new Promise(function (resolve) {
      var url = window.location.origin + "/powerpoint/dialog.html?m=" + encodeURIComponent(text);
      try {
        window.Office.context.ui.displayDialogAsync(url, { height: 30, width: 35, displayInIframe: true }, function (r) {
          if (r.status !== window.Office.AsyncResultStatus.Succeeded) return resolve();
          var dialog = r.value;
          var done = function () {
            try {
              dialog.close();
            } catch (e) {
              /* already closed */
            }
            resolve();
          };
          dialog.addEventHandler(window.Office.EventType.DialogMessageReceived, done);
          dialog.addEventHandler(window.Office.EventType.DialogEventReceived, done);
        });
      } catch (e) {
        resolve();
      }
    });
  }

  function describe(error) {
    var e = error || {};
    return (e.summary || "Rendering failed.") + (e.line ? " (line " + e.line + ")" : "");
  }

  /** Render the selected equations again from their stored LaTeX. */
  function regenerateSelection(event) {
    var ppt = window.LavaTeXPowerPoint;
    var h = helper();
    var s = settings();
    var updated = 0;
    var problems = [];
    ppt
      .readSelectedEquations()
      .then(function (items) {
        if (!items.length) {
          problems.push("Select one or more LavaTeX equations, then click Regenerate.");
          return;
        }
        // One at a time: each replacement changes the slide.
        return items.reduce(function (chain, item) {
          return chain.then(function () {
            var preview = item.record.source.split("\n")[0].slice(0, 40);
            return h
              .render(ppt.requestFromRecord(item.record, s))
              .then(function (res) {
                if (!res.ok) {
                  problems.push(preview + ": " + describe(res.error));
                  return;
                }
                return ppt.place(res, item).then(function () {
                  updated++;
                });
              })
              .catch(function (err) {
                problems.push(preview + ": " + (err.message || String(err)));
              });
          });
        }, Promise.resolve());
      })
      .catch(function (err) {
        problems.push(err.message || String(err));
      })
      .then(function () {
        if (!problems.length) return;
        var head = updated ? updated + " equation(s) regenerated. Some could not be:\n\n" : "";
        return showMessage(head + problems.join("\n"));
      })
      .then(function () {
        if (event && typeof event.completed === "function") event.completed();
      });
  }

  function openHelp(event) {
    try {
      window.Office.context.ui.openBrowserWindow(HELP_URL);
    } catch (e) {
      window.open(HELP_URL, "_blank", "noopener");
    }
    if (event && typeof event.completed === "function") event.completed();
  }

  // Older Office versions call these globals by their manifest names.
  window.regenerateSelection = regenerateSelection;
  window.openHelp = openHelp;

  window.Office.onReady(function () {
    if (window.Office.actions && typeof window.Office.actions.associate === "function") {
      window.Office.actions.associate("regenerateSelection", regenerateSelection);
      window.Office.actions.associate("openHelp", openHelp);
    }
  });
})(window);

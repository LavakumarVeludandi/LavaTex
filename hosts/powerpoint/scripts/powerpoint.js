/*
 * LavaTeX operations on a PowerPoint presentation (Office.js), shared by the
 * task pane and the ribbon commands.
 *
 * An equation is an ordinary picture. Its LavaTeX record is stored twice:
 * in the shape tag LAVATEX and in the alt text (PowerPoint 1.10+ API), using
 * the same `lavatex:1:` format as the other hosts. So equations move freely
 * between PowerPoint, LibreOffice and ONLYOFFICE.
 *
 * PowerPoint has no "replace picture" call. Editing inserts the new picture,
 * gives it the old one's position, rotation and stacking order, and then
 * deletes the old one.
 */
(function (root, factory) {
  if (typeof module === "object" && module.exports) {
    module.exports = factory;
  } else {
    root.LavaTeXPowerPoint = factory(root);
  }
})(typeof self !== "undefined" ? self : this, function (env) {
  "use strict";

  var TAG = "LAVATEX";
  var SHAPE_NAME = "LavaTeX equation";
  var ALT_TITLE = "LaTeX equation";
  // 16:9 default, used only when the slide size cannot be read.
  var DEFAULT_SLIDE = { width: 960, height: 540 };

  function O() {
    return env.Office;
  }
  function PP() {
    return env.PowerPoint;
  }
  function LT() {
    return env.LavaTeX;
  }

  function supports(version) {
    try {
      return !!O().context.requirements.isSetSupported("PowerPointApi", version);
    } catch (e) {
      return false;
    }
  }

  /** What the add-in can do in this PowerPoint. */
  function capabilities() {
    return {
      selection: supports("1.5"), // read and set the selected shapes
      tags: supports("1.3"),
      zOrder: supports("1.8"),
      altText: supports("1.10"), // alt text, rotation, slide size
    };
  }

  function recordFrom(tagValue, altText) {
    return LT().fromAltText(tagValue) || LT().fromAltText(altText);
  }

  /**
   * The selection: { count, shape, record }. `shape` is set when exactly one
   * shape is selected; `record` when that shape is a LavaTeX equation.
   */
  function readSelection() {
    var caps = capabilities();
    if (!caps.selection) return Promise.resolve({ count: 0, shape: null, record: null });
    return PP().run(function (ctx) {
      var shapes = ctx.presentation.getSelectedShapes();
      shapes.load("items/id,items/name,items/type,items/left,items/top,items/width,items/height");
      return ctx.sync().then(function () {
        var count = shapes.items.length;
        if (count !== 1) return { count: count, shape: null, record: null };
        var s = shapes.items[0];
        var tag = s.tags.getItemOrNullObject(TAG);
        tag.load("value");
        if (caps.altText) s.load("altTextDescription,rotation");
        return ctx.sync().then(function () {
          var alt = caps.altText ? s.altTextDescription : null;
          var shape = {
            id: s.id,
            name: s.name,
            type: s.type,
            left: s.left,
            top: s.top,
            width: s.width,
            height: s.height,
            rotation: caps.altText ? s.rotation || 0 : 0,
          };
          return { count: 1, shape: shape, record: recordFrom(tag.isNullObject ? null : tag.value, alt) };
        });
      });
    });
  }

  /** All selected shapes that are LavaTeX equations. */
  function readSelectedEquations() {
    var caps = capabilities();
    if (!caps.selection) return Promise.resolve([]);
    return PP().run(function (ctx) {
      var shapes = ctx.presentation.getSelectedShapes();
      shapes.load("items/id,items/left,items/top,items/width,items/height");
      return ctx.sync().then(function () {
        var tags = shapes.items.map(function (s) {
          var t = s.tags.getItemOrNullObject(TAG);
          t.load("value");
          if (caps.altText) s.load("altTextDescription,rotation");
          return t;
        });
        return ctx.sync().then(function () {
          var out = [];
          shapes.items.forEach(function (s, i) {
            var record = recordFrom(tags[i].isNullObject ? null : tags[i].value, caps.altText ? s.altTextDescription : null);
            if (record) {
              out.push({
                shape: { id: s.id, left: s.left, top: s.top, width: s.width, height: s.height, rotation: caps.altText ? s.rotation || 0 : 0 },
                record: record,
              });
            }
          });
          return out;
        });
      });
    });
  }

  /** Size in points: natural size, or the edited equation's scale kept. */
  function sizePt(result, editing) {
    var scale = 1;
    if (editing && editing.record && editing.record.widthBp > 0 && editing.shape && editing.shape.width > 0) {
      scale = editing.shape.width / editing.record.widthBp;
      if (!(scale > 0.01 && scale < 100)) scale = 1;
    }
    return { width: result.widthBp * scale, height: result.heightBp * scale };
  }

  function base64ToUtf8(b64) {
    var binary = typeof atob === "function" ? atob(b64) : Buffer.from(b64, "base64").toString("binary");
    var bytes = new Uint8Array(binary.length);
    for (var i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return new TextDecoder("utf-8").decode(bytes);
  }

  function insertPicture(result, box) {
    return new Promise(function (resolve, reject) {
      var svg = result.format === "svg";
      var data = svg ? base64ToUtf8(result.dataBase64) : result.dataBase64;
      O().context.document.setSelectedDataAsync(
        data,
        {
          coercionType: svg ? O().CoercionType.XmlSvg : O().CoercionType.Image,
          imageLeft: box.left,
          imageTop: box.top,
          imageWidth: box.width,
          imageHeight: box.height,
        },
        function (r) {
          if (r.status === O().AsyncResultStatus.Succeeded) resolve();
          else reject(new Error((r.error && r.error.message) || "PowerPoint did not insert the picture."));
        }
      );
    });
  }

  /** The slide being edited and the ids of its shapes. */
  function currentSlideShapeIds(ctx) {
    var slide = ctx.presentation.getSelectedSlides().getItemAt(0);
    slide.load("id");
    slide.shapes.load("items/id");
    return ctx.sync().then(function () {
      return {
        slide: slide,
        ids: slide.shapes.items.map(function (s) {
          return s.id;
        }),
      };
    });
  }

  function slideSize(ctx) {
    if (!capabilities().altText) return Promise.resolve(DEFAULT_SLIDE);
    var setup = ctx.presentation.pageSetup;
    setup.load("slideWidth,slideHeight");
    return ctx.sync().then(
      function () {
        return { width: setup.slideWidth || DEFAULT_SLIDE.width, height: setup.slideHeight || DEFAULT_SLIDE.height };
      },
      function () {
        return DEFAULT_SLIDE;
      }
    );
  }

  /** Move `shape` down until it sits just above `below` (both on one slide). */
  function restackAbove(ctx, shape, below) {
    var steps = 0;
    function step() {
      shape.load("zOrderPosition");
      below.load("zOrderPosition");
      return ctx.sync().then(function () {
        if (shape.zOrderPosition <= below.zOrderPosition + 1 || steps++ > 500) return;
        shape.setZOrder("SendBackward");
        return ctx.sync().then(step);
      });
    }
    return step();
  }

  /**
   * Put a rendered equation on the current slide. With `editing` (from
   * readSelection), it replaces that equation, keeping its scale, position,
   * rotation and stacking order. Resolves with { shapeId, altText }.
   */
  function place(result, editing) {
    var caps = capabilities();
    if (!caps.selection) {
      return Promise.reject(new Error("This version of PowerPoint is too old for LavaTeX (PowerPoint API 1.5 or newer is needed)."));
    }
    var size = sizePt(result, editing);
    var before;
    return PP()
      .run(function (ctx) {
        return currentSlideShapeIds(ctx).then(function (info) {
          before = info.ids;
          // Inserting while the old picture is selected must not replace it.
          if (editing) info.slide.setSelectedShapes([]);
          return slideSize(ctx).then(function (slide) {
            return ctx.sync().then(function () {
              return slide;
            });
          });
        });
      })
      .then(function (slide) {
        var box = editing
          ? { left: editing.shape.left, top: editing.shape.top, width: size.width, height: size.height }
          : { left: Math.max(0, (slide.width - size.width) / 2), top: Math.max(0, (slide.height - size.height) / 2), width: size.width, height: size.height };
        return insertPicture(result, box);
      })
      .then(function () {
        return PP().run(function (ctx) {
          return currentSlideShapeIds(ctx).then(function (info) {
            var added = info.ids.filter(function (id) {
              return before.indexOf(id) < 0;
            });
            if (added.length !== 1) {
              throw new Error("The equation was inserted, but LavaTeX could not find it on the slide to store its LaTeX source.");
            }
            var shape = info.slide.shapes.getItem(added[0]);
            shape.name = SHAPE_NAME;
            shape.tags.add(TAG, result.altText);
            if (caps.altText) {
              shape.altTextTitle = ALT_TITLE;
              shape.altTextDescription = result.altText;
              if (editing && editing.shape.rotation) shape.rotation = editing.shape.rotation;
            }
            var chain = ctx.sync();
            if (editing) {
              var old = info.slide.shapes.getItemOrNullObject(editing.shape.id);
              chain = chain
                .then(function () {
                  old.load("id");
                  return ctx.sync();
                })
                .then(function () {
                  if (old.isNullObject) return;
                  return (caps.zOrder ? restackAbove(ctx, shape, old) : Promise.resolve()).then(function () {
                    old.delete();
                    return ctx.sync();
                  });
                });
            }
            return chain
              .then(function () {
                info.slide.setSelectedShapes([added[0]]);
                return ctx.sync();
              })
              .then(function () {
                return { shapeId: added[0], altText: caps.altText };
              });
          });
        });
      });
  }

  /** Request to render an equation again from its record. */
  function requestFromRecord(record, settings) {
    var req = {
      source: record.source,
      preamble: record.preamble || "",
      mode: record.mode || "display",
      fontSize: record.fontSize || 20,
      color: record.color || "#000000",
      format: record.format === "png" ? "png" : "svg",
      pngDpi: (settings && settings.pngDpi) || 300,
    };
    if (record.engine) req.engine = record.engine;
    if (settings && settings.toolchain) req.toolchain = settings.toolchain;
    return req;
  }

  return {
    TAG: TAG,
    capabilities: capabilities,
    readSelection: readSelection,
    readSelectedEquations: readSelectedEquations,
    sizePt: sizePt,
    place: place,
    requestFromRecord: requestFromRecord,
  };
});

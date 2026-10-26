/*
 * A small stand-in for Office.js in PowerPoint, for the end-to-end tests.
 *
 * It follows the rules of the real API that are easy to get wrong:
 * - property values are only readable after load() and sync(), and are a
 *   snapshot taken at that sync;
 * - writes and method calls are queued and applied at sync();
 * - APIs newer than the simulated PowerPoint (window.__pptInit.api) throw.
 *
 * The presentation lives in window.__ppt so tests can set it up and inspect it.
 */
(function () {
  "use strict";

  var init = window.__pptInit || {};
  var model = (window.__ppt = {
    api: init.api || "1.10",
    slideWidth: 960,
    slideHeight: 540,
    nextId: init.nextId || 100,
    // One slide; shapes in z-order (index 0 at the back).
    shapes: init.shapes || [],
    selection: init.selection || [],
    handlers: [],
    dialogs: [],
    browserWindows: [],
    errors: [],
    inserts: 0,
  });

  // Shape fields: id, name, type, left, top, width, height, rotation,
  // altTextDescription, altTextTitle, tags {KEY: value}, data, coercion.

  var REQUIRES = {
    altTextDescription: "1.10",
    altTextTitle: "1.10",
    rotation: "1.10",
    slideWidth: "1.10",
    slideHeight: "1.10",
    zOrderPosition: "1.8",
    setZOrder: "1.8",
    getSelectedShapes: "1.5",
    getSelectedSlides: "1.5",
    setSelectedShapes: "1.5",
  };

  function versionAtLeast(have, need) {
    var a = have.split(".").map(Number);
    var b = need.split(".").map(Number);
    for (var i = 0; i < Math.max(a.length, b.length); i++) {
      var x = a[i] || 0;
      var y = b[i] || 0;
      if (x !== y) return x > y;
    }
    return true;
  }

  function requireApi(name) {
    var need = REQUIRES[name];
    if (need && !versionAtLeast(model.api, need)) {
      throw new Error("ApiNotFound: " + name + " needs PowerPointApi " + need + " (have " + model.api + ")");
    }
  }

  function fail(msg) {
    model.errors.push(msg);
    throw new Error(msg);
  }

  function findShape(id) {
    for (var i = 0; i < model.shapes.length; i++) if (model.shapes[i].id === id) return model.shapes[i];
    return null;
  }

  // --- client objects --------------------------------------------------------

  function ClientObject(ctx, resolve, props) {
    this._ctx = ctx;
    this._resolve = resolve; // () => model object or null
    this._loaded = null; // snapshot after load + sync
    this._wanted = null;
    var self = this;
    props.forEach(function (p) {
      Object.defineProperty(self, p, {
        get: function () {
          requireApi(p);
          if (!self._loaded || !(p in self._loaded)) fail("PropertyNotLoaded: " + p + " was read without load() and sync()");
          return self._loaded[p];
        },
        set: function (v) {
          requireApi(p);
          ctx._queue.push(function () {
            var target = self._resolve();
            if (!target) throw new Error("ItemNotFound");
            target[p] = v;
          });
        },
      });
    });
    Object.defineProperty(this, "isNullObject", {
      get: function () {
        if (!self._loaded) fail("PropertyNotLoaded: isNullObject was read without load() and sync()");
        return self._loaded.__null;
      },
    });
  }
  ClientObject.prototype.load = function (names) {
    var list = typeof names === "string" ? names.split(",") : names || [];
    list = list.map(function (s) {
      return s.trim();
    });
    list.forEach(requireApi);
    this._wanted = (this._wanted || []).concat(list);
    this._ctx._loads.push(this);
    return this;
  };
  ClientObject.prototype._snapshot = function () {
    var target = this._resolve();
    // zOrderPosition is the index in the slide's stacking order.
    if (target && model.shapes.indexOf(target) >= 0) target.zOrderPosition = model.shapes.indexOf(target);
    // Like Office.js, earlier loaded values stay readable.
    var snap = Object.assign({}, this._loaded || {}, { __null: !target });
    if (target) {
      (this._wanted || []).forEach(function (p) {
        snap[p] = typeof target[p] === "function" ? target[p]() : target[p];
      });
    }
    this._loaded = snap;
    this._wanted = null;
  };

  function Tag(ctx, shapeResolve, key) {
    ClientObject.call(this, ctx, function () {
      var s = shapeResolve();
      return s && s.tags && key in s.tags ? { key: key, value: s.tags[key] } : null;
    }, ["key", "value"]);
  }
  Tag.prototype = Object.create(ClientObject.prototype);

  function Tags(ctx, shapeResolve) {
    this.getItemOrNullObject = function (key) {
      return new Tag(ctx, shapeResolve, key.toUpperCase());
    };
    this.add = function (key, value) {
      ctx._queue.push(function () {
        var s = shapeResolve();
        if (!s) throw new Error("ItemNotFound");
        s.tags = s.tags || {};
        s.tags[key.toUpperCase()] = String(value);
      });
    };
  }

  var SHAPE_PROPS = ["id", "name", "type", "left", "top", "width", "height", "rotation", "altTextDescription", "altTextTitle", "zOrderPosition"];

  function Shape(ctx, resolve) {
    ClientObject.call(this, ctx, resolve, SHAPE_PROPS);
    this.tags = new Tags(ctx, resolve);
  }
  Shape.prototype = Object.create(ClientObject.prototype);
  Shape.prototype.delete = function () {
    var resolve = this._resolve;
    this._ctx._queue.push(function () {
      var s = resolve();
      if (!s) throw new Error("ItemNotFound");
      model.shapes.splice(model.shapes.indexOf(s), 1);
      model.selection = model.selection.filter(function (id) {
        return id !== s.id;
      });
    });
  };
  Shape.prototype.setZOrder = function (where) {
    requireApi("setZOrder");
    var resolve = this._resolve;
    this._ctx._queue.push(function () {
      var s = resolve();
      var i = model.shapes.indexOf(s);
      var j = { SendBackward: i - 1, BringForward: i + 1, SendToBack: 0, BringToFront: model.shapes.length - 1 }[where];
      if (j === undefined) throw new Error("InvalidArgument: " + where);
      j = Math.max(0, Math.min(model.shapes.length - 1, j));
      model.shapes.splice(i, 1);
      model.shapes.splice(j, 0, s);
    });
  };

  function ShapeCollection(ctx, list) {
    // list: () => array of model shapes
    var self = this;
    this._ctx = ctx;
    this._list = list;
    this._wanted = null;
    this._items = null;
    Object.defineProperty(this, "items", {
      get: function () {
        if (!self._items) fail("PropertyNotLoaded: items was read without load() and sync()");
        return self._items;
      },
    });
  }
  ShapeCollection.prototype.load = function (spec) {
    var props = (spec || "")
      .split(",")
      .map(function (s) {
        return s.trim().replace(/^items\//, "");
      })
      .filter(Boolean);
    props.forEach(requireApi);
    this._wanted = props;
    this._ctx._loads.push(this);
    return this;
  };
  ShapeCollection.prototype._snapshot = function () {
    var ctx = this._ctx;
    var wanted = this._wanted || [];
    this._items = this._list().map(function (s) {
      var id = s.id;
      var obj = new Shape(ctx, function () {
        return findShape(id);
      });
      obj._wanted = wanted;
      obj._snapshot();
      return obj;
    });
  };
  ShapeCollection.prototype.getItem = function (id) {
    var ctx = this._ctx;
    ctx._queue.push(function () {
      if (!findShape(id)) throw new Error("ItemNotFound: shape " + id);
    });
    return new Shape(ctx, function () {
      return findShape(id);
    });
  };
  ShapeCollection.prototype.getItemOrNullObject = function (id) {
    return new Shape(this._ctx, function () {
      return findShape(id);
    });
  };

  function Slide(ctx) {
    ClientObject.call(this, ctx, function () {
      return { id: "256#" };
    }, ["id"]);
    this.shapes = new ShapeCollection(ctx, function () {
      return model.shapes.slice();
    });
  }
  Slide.prototype = Object.create(ClientObject.prototype);
  Slide.prototype.setSelectedShapes = function (ids) {
    requireApi("setSelectedShapes");
    this._ctx._queue.push(function () {
      ids.forEach(function (id) {
        if (!findShape(id)) throw new Error("ItemNotFound: shape " + id);
      });
      model.selection = ids.slice();
    });
  };

  function Context() {
    var ctx = this;
    this._queue = [];
    this._loads = [];
    var pageSetup = new ClientObject(ctx, function () {
      return { slideWidth: model.slideWidth, slideHeight: model.slideHeight };
    }, ["slideWidth", "slideHeight"]);
    this.presentation = {
      getSelectedShapes: function () {
        requireApi("getSelectedShapes");
        return new ShapeCollection(ctx, function () {
          return model.shapes.filter(function (s) {
            return model.selection.indexOf(s.id) >= 0;
          });
        });
      },
      getSelectedSlides: function () {
        requireApi("getSelectedSlides");
        return {
          getItemAt: function (i) {
            if (i !== 0) throw new Error("only one slide in the mock");
            return new Slide(ctx);
          },
        };
      },
    };
    Object.defineProperty(this.presentation, "pageSetup", {
      get: function () {
        requireApi("slideWidth");
        return pageSetup;
      },
    });
  }
  Context.prototype.sync = function () {
    var ctx = this;
    return new Promise(function (resolve, reject) {
      setTimeout(function () {
        try {
          var queue = ctx._queue;
          ctx._queue = [];
          queue.forEach(function (op) {
            op();
          });
          var loads = ctx._loads;
          ctx._loads = [];
          loads.forEach(function (o) {
            o._snapshot();
          });
          resolve();
        } catch (e) {
          model.errors.push(String(e.message || e));
          reject(e);
        }
      }, 0);
    });
  };

  window.PowerPoint = {
    run: function (fn) {
      var ctx = new Context();
      return Promise.resolve()
        .then(function () {
          return fn(ctx);
        })
        .then(function (result) {
          return ctx.sync().then(function () {
            return result;
          });
        });
    },
  };

  // --- common API --------------------------------------------------------------

  var Office = {
    HostType: { PowerPoint: "PowerPoint" },
    CoercionType: { Image: "image", XmlSvg: "xmlSvg", Text: "text" },
    AsyncResultStatus: { Succeeded: "succeeded", Failed: "failed" },
    EventType: {
      DocumentSelectionChanged: "documentSelectionChanged",
      DialogMessageReceived: "dialogMessageReceived",
      DialogEventReceived: "dialogEventReceived",
    },
    onReady: function (cb) {
      var info = { host: "PowerPoint", platform: "PC" };
      setTimeout(function () {
        if (cb) cb(info);
      }, 0);
      return Promise.resolve(info);
    },
    actions: {
      associate: function (name, fn) {
        (window.__actions = window.__actions || {})[name] = fn;
      },
    },
    context: {
      requirements: {
        isSetSupported: function (name, version) {
          if (name !== "PowerPointApi") return true;
          return versionAtLeast(model.api, version || "1.1");
        },
      },
      document: {
        setSelectedDataAsync: function (data, options, cb) {
          setTimeout(function () {
            try {
              if (options.coercionType === "xmlSvg") {
                if (typeof data !== "string" || !/^\s*(<\?xml|<svg)/.test(data)) throw new Error("not SVG markup");
              } else if (options.coercionType === "image") {
                if (!/^[A-Za-z0-9+/=]+$/.test(data)) throw new Error("not base64 image data");
              } else {
                throw new Error("unexpected coercion " + options.coercionType);
              }
              if (options.imageLeft === undefined || options.imageTop === undefined) {
                throw new Error("no position: PowerPoint would put the picture into a placeholder");
              }
              var id = String(model.nextId++);
              model.shapes.push({
                id: id,
                name: "Picture " + id,
                type: "Image",
                left: options.imageLeft,
                top: options.imageTop,
                width: options.imageWidth,
                height: options.imageHeight,
                rotation: 0,
                altTextDescription: "",
                altTextTitle: "",
                tags: {},
                data: data,
                coercion: options.coercionType,
              });
              model.inserts++;
              model.selection = [id];
              cb({ status: "succeeded" });
            } catch (e) {
              model.errors.push("setSelectedDataAsync: " + e.message);
              cb({ status: "failed", error: { message: e.message } });
            }
          }, 0);
        },
        addHandlerAsync: function (type, handler, cb) {
          model.handlers.push({ type: type, handler: handler });
          if (cb) cb({ status: "succeeded" });
        },
      },
      ui: {
        displayDialogAsync: function (url, options, cb) {
          var handlers = {};
          var dialog = {
            addEventHandler: function (type, h) {
              handlers[type] = h;
            },
            close: function () {
              dialog.closed = true;
            },
            closed: false,
          };
          model.dialogs.push({ url: url, close: function () { if (handlers.dialogMessageReceived) handlers.dialogMessageReceived({ message: "close" }); } });
          setTimeout(function () {
            cb({ status: "succeeded", value: dialog });
          }, 0);
        },
        openBrowserWindow: function (url) {
          model.browserWindows.push(url);
        },
        messageParent: function () {},
      },
    },
  };
  window.Office = Office;

  window.__select = function (ids) {
    model.selection = ids.slice();
    model.handlers.forEach(function (h) {
      if (h.type === "documentSelectionChanged") h.handler({ type: h.type });
    });
  };
})();

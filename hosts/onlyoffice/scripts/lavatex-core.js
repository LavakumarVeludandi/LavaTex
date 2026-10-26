/*
 * LavaTeX shared browser logic: metadata (alt text) parsing, sizing, and the
 * client for the local `lavatex serve` helper. No ONLYOFFICE dependencies, so
 * it can be reused by other HTML-based hosts and tested in Node.
 */
(function (root, factory) {
  if (typeof module === "object" && module.exports) {
    module.exports = factory();
  } else {
    root.LavaTeX = factory();
  }
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  var SCHEMA = "lavatex-equation/1";
  var ALT_TEXT_MARKER = "lavatex:1:";
  var MAX_RECORD_BYTES = 256 * 1024;
  var EMU_PER_BP = 12700; // 914400 EMU per inch / 72
  var DEFAULT_HELPER = "http://127.0.0.1:7354";

  function decodeBase64Url(text) {
    var b64 = text.replace(/-/g, "+").replace(/_/g, "/");
    while (b64.length % 4) b64 += "=";
    var binary = typeof atob === "function" ? atob(b64) : Buffer.from(b64, "base64").toString("binary");
    var bytes = new Uint8Array(binary.length);
    for (var i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  }

  function validate(record) {
    if (!record || typeof record !== "object") return null;
    if (record.schema !== SCHEMA || typeof record.source !== "string") return null;
    return record;
  }

  /** Parse the record from a picture's alt text / description. */
  function fromAltText(text) {
    if (typeof text !== "string") return null;
    var pos = text.lastIndexOf(ALT_TEXT_MARKER);
    if (pos < 0) return null;
    var m = /^[A-Za-z0-9_-]+/.exec(text.slice(pos + ALT_TEXT_MARKER.length));
    if (!m || m[0].length > (MAX_RECORD_BYTES * 4) / 3 + 4) return null;
    try {
      return validate(JSON.parse(decodeBase64Url(m[0])));
    } catch (e) {
      return null;
    }
  }

  /**
   * Size in EMU for a rendered result. When editing, keep the user's scale
   * relative to the previously rendered size (not the old box, which would
   * distort a longer equation).
   */
  function sizeEmu(result, previous) {
    var scale = 1;
    if (previous && previous.record && previous.record.widthBp > 0 && previous.widthEmu > 0) {
      scale = previous.widthEmu / (previous.record.widthBp * EMU_PER_BP);
      if (!(scale > 0.01 && scale < 100)) scale = 1;
    }
    return {
      width: Math.max(1, Math.round(result.widthBp * EMU_PER_BP * scale)),
      height: Math.max(1, Math.round(result.heightBp * EMU_PER_BP * scale)),
    };
  }

  function dataUrl(result) {
    return "data:" + result.mime + ";base64," + result.dataBase64;
  }

  /** Client for `lavatex serve`. */
  function Helper(baseUrl, token) {
    this.baseUrl = (baseUrl || DEFAULT_HELPER).replace(/\/+$/, "");
    this.token = token || "";
  }

  Helper.prototype._fetch = function (path, options) {
    options = options || {};
    var headers = { "Content-Type": "application/json" };
    if (this.token) headers.Authorization = "Bearer " + this.token;
    return fetch(this.baseUrl + path, {
      method: options.method || "GET",
      headers: headers,
      body: options.body ? JSON.stringify(options.body) : undefined,
      signal: options.signal,
    }).then(function (res) {
      return res.json().then(function (body) {
        body.httpStatus = res.status;
        return body;
      });
    });
  };

  Helper.prototype.health = function () {
    return this._fetch("/v1/health");
  };
  Helper.prototype.toolchains = function (refresh) {
    return this._fetch("/v1/toolchains" + (refresh ? "?refresh=1" : ""));
  };
  Helper.prototype.render = function (request, signal) {
    return this._fetch("/v1/render", { method: "POST", body: request, signal: signal });
  };

  return {
    SCHEMA: SCHEMA,
    ALT_TEXT_MARKER: ALT_TEXT_MARKER,
    EMU_PER_BP: EMU_PER_BP,
    DEFAULT_HELPER: DEFAULT_HELPER,
    fromAltText: fromAltText,
    sizeEmu: sizeEmu,
    dataUrl: dataUrl,
    Helper: Helper,
  };
});

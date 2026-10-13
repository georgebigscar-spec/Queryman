// Окружение скрипта: объект pm (подмножество API Postman), console и pm.expect.
// Данные приходят строкой JSON в __input, текст скрипта — в __script,
// результат забирается вызовом __finish(). Rust видит только строки.
(function () {
  "use strict";
  const input = JSON.parse(globalThis.__input);
  const out = { ops: [], tests: [], logs: [], error: null };

  const show = (v) => {
    if (typeof v === "string") return v;
    if (v === undefined) return "undefined";
    if (typeof v === "function") return "[Function]";
    try {
      return JSON.stringify(v);
    } catch (e) {
      return String(v);
    }
  };
  const asString = (v) => {
    if (v === undefined || v === null) return "";
    if (typeof v === "object") return JSON.stringify(v);
    return String(v);
  };

  const console = {};
  for (const level of ["log", "info", "warn", "error", "debug"]) {
    console[level] = (...args) =>
      out.logs.push({ level: level === "debug" ? "log" : level, text: args.map(show).join(" ") });
  }
  globalThis.console = console;

  class AssertionError extends Error {
    constructor(message) {
      super(message);
      this.name = "AssertionError";
    }
  }

  // ---- переменные -------------------------------------------------------

  const environment = Object.assign({}, input.environment);
  const globals = Object.assign({}, input.globals);
  const locals = {};

  const lookup = (key) => {
    for (const store of [locals, environment, globals]) {
      if (Object.prototype.hasOwnProperty.call(store, key)) return store[key];
    }
    return undefined;
  };
  const replaceIn = (template) =>
    String(template).replace(/\{\{([^{}]+)\}\}/g, (match, name) => {
      const value = lookup(name.trim());
      return value === undefined ? match : value;
    });

  function scope(name, store, record) {
    return {
      name: name === "environment" ? input.environment_name : name,
      get: (key) => store[key],
      has: (key) => Object.prototype.hasOwnProperty.call(store, key),
      set: (key, value) => {
        const text = asString(value);
        store[String(key)] = text;
        if (record) out.ops.push({ scope: name, key: String(key), value: text });
      },
      unset: (key) => {
        delete store[key];
        if (record) out.ops.push({ scope: name, key: String(key), value: null });
      },
      clear: () => {
        for (const key of Object.keys(store)) {
          delete store[key];
          if (record) out.ops.push({ scope: name, key, value: null });
        }
      },
      toObject: () => Object.assign({}, store),
      replaceIn,
    };
  }

  // ---- заголовки --------------------------------------------------------

  function headerList(list) {
    const same = (h, name) => h.key.toLowerCase() === String(name).toLowerCase();
    return {
      get: (name) => {
        const h = list.find((h) => same(h, name));
        return h ? h.value : undefined;
      },
      has: (name) => list.some((h) => same(h, name)),
      add: (h) => {
        list.push({ key: String(h.key), value: asString(h.value) });
      },
      upsert: (h) => {
        const found = list.find((x) => same(x, h.key));
        if (found) found.value = asString(h.value);
        else list.push({ key: String(h.key), value: asString(h.value) });
      },
      remove: (name) => {
        for (let i = list.length - 1; i >= 0; i--) if (same(list[i], name)) list.splice(i, 1);
      },
      toObject: () => Object.fromEntries(list.map((h) => [h.key, h.value])),
      all: () => list.map((h) => ({ key: h.key, value: h.value })),
      each: (fn) => list.forEach((h) => fn({ key: h.key, value: h.value })),
      count: () => list.length,
    };
  }

  // ---- pm.expect: небольшое подмножество chai ---------------------------

  const typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : typeof v);
  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => Object.prototype.hasOwnProperty.call(b, k) && deepEqual(a[k], b[k]));
  }

  class Assertion {
    constructor(actual) {
      this.actual = actual;
      this.negate = false;
      this.isDeep = false;
    }
    check(ok, message, negated) {
      if (this.negate ? ok : !ok) throw new AssertionError(this.negate ? negated : message);
      return this;
    }
    get to() { return this; }
    get be() { return this; }
    get been() { return this; }
    get is() { return this; }
    get that() { return this; }
    get which() { return this; }
    get and() { return this; }
    get has() { return this; }
    get have() { return this; }
    get with() { return this; }
    get at() { return this; }
    get of() { return this; }
    get same() { return this; }
    get not() { this.negate = !this.negate; return this; }
    get deep() { this.isDeep = true; return this; }

    equal(v) {
      const ok = this.isDeep ? deepEqual(this.actual, v) : this.actual === v;
      return this.check(ok, `expected ${show(this.actual)} to equal ${show(v)}`,
        `expected ${show(this.actual)} to not equal ${show(v)}`);
    }
    equals(v) { return this.equal(v); }
    eq(v) { return this.equal(v); }
    eql(v) {
      return this.check(deepEqual(this.actual, v), `expected ${show(this.actual)} to deeply equal ${show(v)}`,
        `expected ${show(this.actual)} to not deeply equal ${show(v)}`);
    }
    get true() { return this.check(this.actual === true, `expected ${show(this.actual)} to be true`, "expected true to be false"); }
    get false() { return this.check(this.actual === false, `expected ${show(this.actual)} to be false`, "expected false to be true"); }
    get null() { return this.check(this.actual === null, `expected ${show(this.actual)} to be null`, "expected null not to be null"); }
    get undefined() { return this.check(this.actual === undefined, `expected ${show(this.actual)} to be undefined`, "expected value not to be undefined"); }
    get ok() { return this.check(!!this.actual, `expected ${show(this.actual)} to be truthy`, `expected ${show(this.actual)} to be falsy`); }
    get exist() { return this.check(this.actual != null, "expected value to exist", `expected ${show(this.actual)} to not exist`); }
    get empty() {
      const a = this.actual;
      const ok = typeof a === "string" || Array.isArray(a) ? a.length === 0 : a && typeof a === "object" ? Object.keys(a).length === 0 : false;
      return this.check(ok, `expected ${show(a)} to be empty`, `expected ${show(a)} not to be empty`);
    }
    a(type) {
      return this.check(typeOf(this.actual) === String(type).toLowerCase(),
        `expected ${show(this.actual)} to be a ${type}`, `expected ${show(this.actual)} not to be a ${type}`);
    }
    an(type) { return this.a(type); }
    above(n) { return this.check(this.actual > n, `expected ${show(this.actual)} to be above ${n}`, `expected ${show(this.actual)} to be at most ${n}`); }
    gt(n) { return this.above(n); }
    greaterThan(n) { return this.above(n); }
    below(n) { return this.check(this.actual < n, `expected ${show(this.actual)} to be below ${n}`, `expected ${show(this.actual)} to be at least ${n}`); }
    lt(n) { return this.below(n); }
    lessThan(n) { return this.below(n); }
    least(n) { return this.check(this.actual >= n, `expected ${show(this.actual)} to be at least ${n}`, `expected ${show(this.actual)} to be below ${n}`); }
    gte(n) { return this.least(n); }
    most(n) { return this.check(this.actual <= n, `expected ${show(this.actual)} to be at most ${n}`, `expected ${show(this.actual)} to be above ${n}`); }
    lte(n) { return this.most(n); }
    within(lo, hi) {
      return this.check(this.actual >= lo && this.actual <= hi, `expected ${show(this.actual)} to be within ${lo}..${hi}`,
        `expected ${show(this.actual)} to not be within ${lo}..${hi}`);
    }
    include(v) {
      const a = this.actual;
      let ok = false;
      if (typeof a === "string") ok = a.includes(String(v));
      else if (Array.isArray(a)) ok = a.some((x) => (this.isDeep ? deepEqual(x, v) : x === v));
      else if (a && typeof a === "object" && v && typeof v === "object")
        ok = Object.keys(v).every((k) => deepEqual(a[k], v[k]));
      return this.check(ok, `expected ${show(a)} to include ${show(v)}`, `expected ${show(a)} to not include ${show(v)}`);
    }
    includes(v) { return this.include(v); }
    contain(v) { return this.include(v); }
    contains(v) { return this.include(v); }
    lengthOf(n) {
      const len = this.actual == null ? undefined : this.actual.length;
      return this.check(len === n, `expected ${show(this.actual)} to have length ${n} but got ${len}`,
        `expected ${show(this.actual)} to not have length ${n}`);
    }
    property(name, ...value) {
      const a = this.actual;
      const has = a != null && typeof a === "object" && name in a;
      if (value.length === 0)
        return this.check(has, `expected ${show(a)} to have property '${name}'`, `expected ${show(a)} to not have property '${name}'`);
      return this.check(has && deepEqual(a[name], value[0]),
        `expected ${show(a)} to have property '${name}' of ${show(value[0])}`,
        `expected ${show(a)} to not have property '${name}' of ${show(value[0])}`);
    }
    keys(...names) {
      const list = names.length === 1 && Array.isArray(names[0]) ? names[0] : names;
      const a = this.actual;
      const ok = a != null && typeof a === "object" && list.every((k) => Object.prototype.hasOwnProperty.call(a, k));
      return this.check(ok, `expected ${show(a)} to have keys ${show(list)}`, `expected ${show(a)} to not have keys ${show(list)}`);
    }
    match(re) {
      return this.check(re.test(String(this.actual)), `expected ${show(this.actual)} to match ${re}`,
        `expected ${show(this.actual)} not to match ${re}`);
    }
    oneOf(list) {
      return this.check(list.some((x) => deepEqual(x, this.actual)), `expected ${show(this.actual)} to be one of ${show(list)}`,
        `expected ${show(this.actual)} to not be one of ${show(list)}`);
    }
  }

  // ---- pm ----------------------------------------------------------------

  const request = input.request;
  const pm = {
    info: {
      eventName: input.phase === "pre" ? "prerequest" : "test",
      requestName: input.request_name,
    },
    environment: scope("environment", environment, true),
    globals: scope("globals", globals, true),
    variables: Object.assign(scope("variables", locals, false), {
      get: (key) => lookup(key),
      has: (key) => lookup(key) !== undefined,
      toObject: () => Object.assign({}, globals, environment, locals),
    }),
    request: {
      get method() { return request.method; },
      set method(v) { request.method = String(v).toUpperCase(); },
      get url() { return request.url; },
      set url(v) { request.url = String(v); },
      headers: headerList(request.headers),
      get body() { return request.body; },
      set body(v) { request.body = v === null || v === undefined ? null : asString(v); },
    },
    test(name, fn) {
      try {
        fn();
        out.tests.push({ name: String(name), passed: true });
      } catch (e) {
        out.tests.push({ name: String(name), passed: false, error: e && e.message !== undefined ? String(e.message) : String(e) });
      }
    },
    expect: (actual) => new Assertion(actual),
  };

  if (input.response) {
    const r = input.response;
    const status = (code) => {
      if (r.code !== code) throw new AssertionError(`expected response to have status code ${code} but got ${r.code}`);
    };
    const range = (lo, hi, what) => {
      if (r.code < lo || r.code > hi) throw new AssertionError(`expected response to be ${what} but got ${r.code}`);
      return true;
    };
    pm.response = {
      code: r.code,
      status: r.status,
      responseTime: r.response_time,
      responseSize: r.size,
      headers: headerList(r.headers),
      text: () => r.body,
      json: () => JSON.parse(r.body),
      to: {
        have: {
          status,
          header: (name, value) => {
            const found = r.headers.find((h) => h.key.toLowerCase() === String(name).toLowerCase());
            if (!found) throw new AssertionError(`expected response to have header '${name}'`);
            if (value !== undefined && found.value !== String(value))
              throw new AssertionError(`expected header '${name}' to be '${value}' but got '${found.value}'`);
          },
          body: (text) => {
            if (text !== undefined && r.body !== String(text)) throw new AssertionError("expected response body to match");
          },
          jsonBody: (path) => {
            let value;
            try { value = JSON.parse(r.body); } catch (e) { throw new AssertionError("expected response body to be JSON"); }
            if (path !== undefined && !(value && typeof value === "object" && path in value))
              throw new AssertionError(`expected response JSON to have property '${path}'`);
          },
        },
        be: {
          get ok() { return range(200, 299, "ok"); },
          get success() { return range(200, 299, "success"); },
          get redirection() { return range(300, 399, "a redirection"); },
          get clientError() { return range(400, 499, "a client error"); },
          get serverError() { return range(500, 599, "a server error"); },
          get error() { return range(400, 599, "an error"); },
          get notFound() { status(404); return true; },
          get unauthorized() { status(401); return true; },
          get forbidden() { status(403); return true; },
        },
      },
    };
  }

  globalThis.pm = pm;
  globalThis.postman = { setEnvironmentVariable: pm.environment.set, setGlobalVariable: pm.globals.set };

  globalThis.__run = function () {
    try {
      new Function("pm", "console", globalThis.__script)(pm, console);
    } catch (e) {
      out.error = e && e.name ? `${e.name}: ${e.message}` : String(e);
    }
  };
  globalThis.__finish = () => JSON.stringify({ request, ops: out.ops, tests: out.tests, logs: out.logs, error: out.error });
})();

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "../static/makibishi-loader.js"), "utf8")
    .replace('import("/static/makibishi/makibishi.js")', 'importModule("/static/makibishi/makibishi.js")');

async function load(importModule) {
    const status = { hidden: false, dataset: {}, textContent: "Loading Makibishi…" };
    const events = {};
    const canvas = {
        addEventListener(name, callback) { events[name] = callback; },
        focus(options) { this.focused = options; },
    };
    const errors = [];
    const document = {
        getElementById(id) { return id === "makibishi-status" ? status : canvas; },
    };
    await vm.runInNewContext(source, {
        document, importModule, console: { error(...args) { errors.push(args); } },
    });
    return { status, canvas, events, errors };
}

test("loads the web build and WASM from the deployment directory", async () => {
    let wasm;
    const result = await load(async (url) => {
        assert.equal(url, "/static/makibishi/makibishi.js");
        return { default: async (options) => { wasm = options.module_or_path; } };
    });
    assert.equal(wasm, "/static/makibishi/makibishi_bg.wasm");
    assert.equal(result.status.hidden, true);
    assert.equal(result.errors.length, 0);
    result.events.pointerdown();
    assert.equal(result.canvas.focused.preventScroll, true);
    let prevented = false;
    result.events.contextmenu({ preventDefault() { prevented = true; } });
    assert.equal(prevented, true);
});

test("missing JS reports build/deployment instructions visibly", async () => {
    const { status, errors } = await load(async () => { throw new Error("404"); });
    assert.equal(status.hidden, false);
    assert.equal(status.dataset.error, "true");
    assert.match(status.textContent, /browser build is missing/);
    assert.match(status.textContent, /wasm-bindgen/);
    assert.match(status.textContent, /\/static\/makibishi\//);
    assert.equal(errors.length, 1);
});

test("WASM or renderer startup failure stays visible without injecting HTML", async () => {
    const { status } = await load(async () => ({
        default: async () => { throw new Error("<script>bad</script>"); },
    }));
    assert.equal(status.hidden, false);
    assert.match(status.textContent, /could not start/);
    assert.match(status.textContent, /makibishi_bg.wasm/);
    assert.match(status.textContent, /WebAssembly/);
    assert.ok(!status.textContent.includes("<script>"));
});

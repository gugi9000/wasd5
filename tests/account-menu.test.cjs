const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

async function menu(account, ok = true) {
    const elements = {
        "account-login": { hidden: false },
        "account-admin": { hidden: true },
        "account-username": { textContent: "" },
        "account-menu-error": { hidden: true, textContent: "" },
    };
    await vm.runInNewContext(
        fs.readFileSync(path.join(__dirname, "..", "static", "account-menu.js"), "utf8"),
        {
            document: { getElementById: (id) => elements[id] },
            fetch: async (url, options) => {
                assert.equal(url, "/api/navigation/account");
                assert.equal(options.cache, "no-store");
                return { ok, json: async () => account };
            },
        }
    );
    return elements;
}

test("guests see Login without the admin dropdown", async () => {
    const elements = await menu({ username: null });
    assert.equal(elements["account-login"].hidden, false);
    assert.equal(elements["account-admin"].hidden, true);
    assert.equal(elements["account-menu-error"].hidden, true);
});

test("authenticated users see Admin and their username as text", async () => {
    const elements = await menu({ username: "<Alice>" });
    assert.equal(elements["account-login"].hidden, true);
    assert.equal(elements["account-admin"].hidden, false);
    assert.equal(elements["account-username"].textContent, "Signed in as <Alice>");
});

test("failed or malformed account responses show an explicit error", async () => {
    for (const [account, ok] of [[{}, true], [{ username: null }, false]]) {
        const elements = await menu(account, ok);
        assert.equal(elements["account-menu-error"].hidden, false);
        assert.ok(elements["account-menu-error"].textContent);
        assert.equal(elements["account-admin"].hidden, true);
    }
});

test("dashboard and navigation share all admin menu links", () => {
    const read = (name) => fs.readFileSync(path.join(__dirname, "..", "templates", name), "utf8");
    assert.ok(read("base.html.tera").includes('{% include "admin/menu" %}'));
    assert.ok(read(path.join("admin", "index.html.tera")).includes('{% include "admin/menu" %}'));
    const shared = read(path.join("admin", "menu.html.tera"));
    for (const href of [
        "/admin/pages", "/admin/landing", "/admin/users", "/admin/files",
        "/admin/pictures", "/creatine", "/admin/calendar", "/calendar",
        "/packages", "/admin/logout",
    ]) assert.ok(shared.includes(`href="${href}"`), href);
});

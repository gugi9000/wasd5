const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { spawnSync } = require("node:child_process");

test("account menu script is referenced by the base template and not ignored by Git", () => {
    const root = path.join(__dirname, "..");
    const template = fs.readFileSync(path.join(root, "templates", "base.html.tera"), "utf8");
    assert.ok(template.includes('<script src="/static/account-menu.js" defer></script>'));
    assert.ok(fs.existsSync(path.join(root, "static", "account-menu.js")));
    const ignored = spawnSync("git", ["check-ignore", "--no-index", "--quiet", "static/account-menu.js"], {
        cwd: root,
        encoding: "utf8",
    });
    assert.ifError(ignored.error);
    assert.equal(ignored.status, 1, `Account menu script must be deployable through Git: ${ignored.stderr}`);
});

async function menu(account, ok = true) {
    const elements = {
        "account-login": { hidden: false },
        "account-admin": { hidden: true },
        "account-username": { textContent: "" },
        "account-menu-error": { hidden: true, textContent: "" },
        "account-admin-toggle": { textContent: "Admin" },
        "account-admin-links": { hidden: false },
        "account-game-links": { hidden: true },
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

test("gamers see game links and Logout rather than admin links", async () => {
    const elements = await menu({ username: "Gamer", gamer: true });
    assert.equal(elements["account-admin"].hidden, false);
    assert.equal(elements["account-admin-toggle"].textContent, "Games");
    assert.equal(elements["account-admin-links"].hidden, true);
    assert.equal(elements["account-game-links"].hidden, false);
    assert.equal(elements["account-username"].textContent, "Signed in as Gamer");
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

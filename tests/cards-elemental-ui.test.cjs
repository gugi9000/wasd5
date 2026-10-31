const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

function element() {
    return {
        hidden: false,
        disabled: false,
        textContent: "",
        value: "",
        children: [],
        attributes: {},
        listeners: {},
        setAttribute(name, value) { this.attributes[name] = value; },
        addEventListener(name, listener) { this.listeners[name] = listener; },
        replaceChildren(...children) { this.children = children; },
        append(...children) { this.children.push(...children); },
        querySelectorAll() { return this.children.flatMap((child) => child.children.filter((entry) => entry.listeners.click)); },
        focus() { this.focused = true; },
    };
}

async function loadUI() {
    const elements = new Map();
    const get = (id) => {
        if (!elements.has(id)) elements.set(id, element());
        return elements.get(id);
    };
    get("cards-elemental").dataset = { csrf: "token" };
    get("cards-opponent").value = "tosser";
    const state = { games: [], current: null, requests: [], intervals: new Map() };
    const game = (status, id = "created") => ({
        mode: "online", status, game_id: id, your_hand: [], your_winnings: [],
        server_winnings: [], deck_size: 110, round: 1, starter: "you",
    });
    vm.runInNewContext(
        fs.readFileSync(path.join(__dirname, "..", "static", "cards-elemental.js"), "utf8"),
        {
            document: { getElementById: get, createElement: element },
            window: { setInterval(callback, delay) { state.intervals.set(delay, callback); } },
            fetch: async (url, options) => {
                state.requests.push({ url, options });
                let response = null;
                if (url.endsWith("/open")) response = { games: state.games };
                else if (url.endsWith("/current")) response = state.current;
                else if (url.endsWith("/join")) response = game("playing", "joined");
                else if (url.endsWith("/online") && options.method === "POST") response = game("waiting");
                return { ok: true, json: async () => response };
            },
        }
    );
    const flush = () => new Promise((resolve) => setImmediate(resolve));
    await flush();
    return { get, state, flush, game };
}

test("join panel starts collapsed and toggles the ID input and public games", async () => {
    const { get, state, flush } = await loadUI();
    assert.equal(get("cards-online-lobby").hidden, true);
    assert.equal(get("cards-toggle-join").attributes["aria-expanded"], "false");
    assert.equal(get("cards-open-games-count").hidden, true);
    state.games = [{ id: "public-one" }, { id: "public-two" }];
    get("cards-toggle-join").listeners.click();
    await flush();
    assert.equal(get("cards-online-lobby").hidden, false);
    assert.equal(get("cards-toggle-join").attributes["aria-expanded"], "true");
    assert.equal(get("cards-join-id").focused, true);
    assert.equal(get("cards-open-games").children.length, 2);
    assert.equal(get("cards-open-games-count").textContent, "2");
    assert.equal(get("cards-open-games-count").hidden, false);
    get("cards-toggle-join").listeners.click();
    assert.equal(get("cards-online-lobby").hidden, true);
    state.games = [];
    state.intervals.get(10000)();
    await flush();
    assert.equal(get("cards-open-games-count").hidden, true);
});

test("dropdown choices preserve visibility and exclude your own game from the count", async () => {
    const { get, state, flush } = await loadUI();
    state.games = [{ id: "created" }, { id: "other" }];
    for (const visibility of ["public", "private"]) {
        get(`cards-start-${visibility}`).listeners.click();
        assert.equal(get("cards-create-game").disabled, true);
        await flush();
        const request = state.requests.findLast((entry) => entry.url.endsWith("/online"));
        assert.equal(JSON.parse(request.options.body).visibility, visibility);
        assert.equal(get("cards-create-game").disabled, false);
        assert.equal(get("cards-open-games-count").textContent, "1");
    }
    assert.equal(get("cards-error").textContent, "");
});

test("joining by ID or public list closes the panel and preserves join behavior", async () => {
    const { get, state, flush } = await loadUI();
    get("cards-toggle-join").listeners.click();
    get("cards-join-id").value = " private-id ";
    get("cards-join-button").listeners.click();
    await flush();
    assert.ok(state.requests.some((entry) => entry.url.endsWith("/private-id/join")));
    assert.equal(get("cards-online-lobby").hidden, true);
    assert.equal(get("cards-toggle-join").attributes["aria-expanded"], "false");
    state.games = [{ id: "another-public" }];
    get("cards-toggle-join").listeners.click();
    await flush();
    get("cards-open-games").children[0].children[1].listeners.click();
    await flush();
    assert.ok(state.requests.some((entry) => entry.url.endsWith("/another-public/join")));
    assert.equal(get("cards-online-lobby").hidden, true);
    assert.equal(get("cards-toggle-join").attributes["aria-expanded"], "false");
    assert.equal(get("cards-error").textContent, "");
});

test("opponent joining closes an expanded lobby when the game starts", async () => {
    const { get, state, flush, game } = await loadUI();
    get("cards-start-public").listeners.click();
    await flush();
    get("cards-toggle-join").listeners.click();
    await flush();
    state.current = game("playing");
    state.intervals.get(2500)();
    await flush();
    assert.equal(get("cards-online-lobby").hidden, true);
    assert.equal(get("cards-toggle-join").attributes["aria-expanded"], "false");
});

test("status is below the playing board and immediately above your hand", () => {
    const template = fs.readFileSync(path.join(__dirname, "..", "templates", "games", "cards_elemental.html.tera"), "utf8");
    const board = template.indexOf('id="cards-round-result"');
    const status = template.indexOf('id="cards-status"');
    const handTitle = template.indexOf('<h2 id="your-hand-title"');
    assert.ok(board < status && status < handTitle);
    assert.match(template, /class="dropdown-menu" aria-labelledby="cards-create-game"/);
    assert.match(template, /id="cards-online-lobby"[^>]* hidden/);
});

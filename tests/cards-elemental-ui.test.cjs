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

async function loadUI(username = "") {
    const elements = new Map();
    const get = (id) => {
        if (!elements.has(id)) elements.set(id, element());
        return elements.get(id);
    };
    get("cards-elemental").dataset = { csrf: "token", username };
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
            Audio: class {
                play() { return Promise.resolve(); }
                pause() {}
            },
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

test("round outcomes are green for wins, red for losses and neutral for ties", async () => {
    const { get, state, flush, game } = await loadUI();
    get("cards-start-public").listeners.click();
    await flush();
    const card = { element: "fire", colour: "red", value: 2 };
    for (const [winner, colour, text] of [
        ["you", "text-success-emphasis", "You win the round!"],
        ["server", "text-danger-emphasis", "Your opponent wins the round."],
        [null, "text-body-secondary", "Draw — both cards are discarded."],
    ]) {
        state.current = {
            ...game("playing"),
            last_round: { round: (state.current?.last_round?.round || 0) + 1, you_played: card, server_played: card, winner },
        };
        state.intervals.get(2500)();
        await flush();
        const result = get("cards-round-result").children;
        assert.equal(result[0].children[0].textContent, "You");
        assert.equal(result[2].children[0].textContent, "Opponent");
        assert.ok(result[3].className.includes(colour));
        assert.equal(result[3].textContent, text);
    }
});

test("played-card labels use PvP usernames, including a pending card", async () => {
    const { get, state, flush, game } = await loadUI();
    get("cards-start-public").listeners.click();
    await flush();
    const card = { element: "ice", colour: "blue", value: 3 };
    state.current = {
        ...game("playing"), your_name: "Alice <3", opponent_name: "Bob",
        last_round: { round: 1, you_played: card, server_played: card, winner: "you" },
    };
    state.intervals.get(2500)();
    await flush();
    assert.equal(get("cards-round-result").children[0].children[0].textContent, "Alice <3");
    assert.equal(get("cards-round-result").children[2].children[0].textContent, "Bob");
    state.current.your_played_card = card;
    state.intervals.get(2500)();
    await flush();
    assert.equal(get("cards-round-result").children[0].children[0].textContent, "Alice <3");
});

test("solo labels use the signed-in username and each bot's name", async () => {
    for (const [opponent, name] of [["tosser", "Tosser"], ["pro", "Pro"], ["pro2", "Pro2"]]) {
        const { get, state, flush, game } = await loadUI("Alice");
        get("cards-start-public").listeners.click();
        await flush();
        const card = { element: "water", colour: "green", value: 4 };
        state.current = {
            ...game("playing"), mode: "solo", opponent,
            last_round: { round: 1, you_played: card, server_played: card, winner: "server" },
        };
        state.intervals.get(2500)();
        await flush();
        assert.equal(get("cards-round-result").children[0].children[0].textContent, "Alice");
        assert.equal(get("cards-round-result").children[2].children[0].textContent, name);
    }
});

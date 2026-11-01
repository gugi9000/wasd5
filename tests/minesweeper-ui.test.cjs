const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const solver = require("../static/minesweeper-solver.js");

function board(rows, columns, mineCount, cells = {}) {
    return {
        id: "test-game",
        status: "playing",
        mode: "advanced",
        rows,
        columns,
        mine_count: mineCount,
        elapsed_ms: 100,
        cells: Array.from({ length: rows * columns }, (_, index) => ({
            revealed: false,
            flagged: false,
            count: 0,
            ...cells[index],
        })),
    };
}

test("solver flags calculated mines and reveals cells cleared by flags", () => {
    const game = board(1, 3, 1, { 0: { revealed: true, count: 1 } });
    assert.deepEqual(solver.nextMove(game), { action: "flag", index: 1, guessed: false });
    game.cells[1].flagged = true;
    assert.deepEqual(solver.nextMove(game), { action: "reveal", index: 2, guessed: false });
});

test("solver uses subset clues without reading hidden mine values", () => {
    const game = board(2, 3, 2, {
        0: { revealed: true, count: 1 },
        1: { revealed: true, count: 1 },
    });
    const move = solver.nextMove(game);
    assert.deepEqual(move, { action: "reveal", index: 2, guessed: false });
    for (const cell of game.cells) {
        if (!cell.revealed) Object.defineProperty(cell, "mine", {
            get() { throw new Error("Hidden mine accessed"); },
        });
    }
    assert.deepEqual(solver.nextMove(game), move);
});

test("solver guesses randomly among unflagged covered cells only", () => {
    const game = board(1, 4, 2, { 0: { flagged: true } });
    assert.deepEqual(solver.nextMove(game, () => 0), {
        action: "reveal", index: 1, guessed: true,
    });
    assert.deepEqual(solver.nextMove(game, () => 0.99), {
        action: "reveal", index: 3, guessed: true,
    });
    game.status = "lost";
    assert.equal(solver.nextMove(game), null);
});

test("solver deductions are sound across generated boards", () => {
    for (let seed = 0; seed < 40; seed += 1) {
        let state = seed + 1;
        const random = () => {
            state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
            return state / 0x100000000;
        };
        const indices = Array.from({ length: 48 }, (_, index) => index);
        for (let index = indices.length - 1; index > 0; index -= 1) {
            const swap = Math.floor(random() * (index + 1));
            [indices[index], indices[swap]] = [indices[swap], indices[index]];
        }
        const mines = new Set(indices.slice(0, 10));
        const game = board(6, 8, mines.size);
        for (let turn = 0; turn < game.cells.length; turn += 1) {
            const next = solver.nextMove(game, random);
            if (!next) break;
            if (next.action === "flag") {
                assert.ok(mines.has(next.index), `Incorrect flag on seed ${seed}`);
                game.cells[next.index].flagged = true;
                continue;
            }
            if (mines.has(next.index)) {
                assert.equal(next.guessed, true, `Incorrect safe deduction on seed ${seed}`);
                break;
            }
            const cell = game.cells[next.index];
            cell.revealed = true;
            const row = Math.floor(next.index / game.columns);
            const column = next.index % game.columns;
            cell.count = [...mines].filter((index) => {
                const r = Math.floor(index / game.columns);
                const c = index % game.columns;
                return Math.max(Math.abs(r - row), Math.abs(c - column)) === 1;
            }).length;
        }
    }
});

function harness(responses, random = () => 0.5) {
    let now = 0;
    let nextTimer = 0;
    const timers = new Map();
    const listeners = new Map();
    const elements = new Map();
    const calls = [];
    function element(id) {
        return {
            dataset: id === "minesweeper" ? { csrf: "test", signedIn: "true" } : {},
            value: "easy",
            hidden: false,
            disabled: false,
            textContent: "",
            classList: { add() {}, remove() {} },
            addEventListener(type, handler) { listeners.set(`${id}:${type}`, handler); },
            replaceChildren() {},
            setAttribute() {},
            append() {},
            querySelector() { return null; },
            insertRow() { return { insertCell: () => element("cell") }; },
            focus() {},
        };
    }
    const document = {
        hidden: false,
        getElementById(id) {
            if (!elements.has(id)) elements.set(id, element(id));
            return elements.get(id);
        },
        createElement: element,
        addEventListener(type, handler) { listeners.set(`document:${type}`, handler); },
    };
    vm.runInNewContext(
        fs.readFileSync(path.join(__dirname, "..", "static", "minesweeper.js"), "utf8"),
        {
            document,
            window: { MinesweeperSolver: solver },
            Date,
            Math: Object.assign(Object.create(Math), { random }),
            setTimeout(handler, delay) {
                const id = ++nextTimer;
                timers.set(id, { handler, due: now + delay });
                return id;
            },
            clearTimeout(id) { timers.delete(id); },
            setInterval() { return 0; },
            clearInterval() {},
            async fetch(url, options) {
                calls.push({ url, body: JSON.parse(options.body) });
                const response = responses.shift();
                assert.ok(response, `Unexpected request: ${url}`);
                return typeof response === "function"
                    ? response()
                    : { ok: true, json: async () => response };
            },
        },
    );
    async function flush() {
        await new Promise((resolve) => setImmediate(resolve));
    }
    async function advance(milliseconds) {
        const end = now + milliseconds;
        while (true) {
            const entry = [...timers.entries()]
                .filter(([, timer]) => timer.due <= end)
                .sort((a, b) => a[1].due - b[1].due)[0];
            if (!entry) break;
            now = entry[1].due;
            timers.delete(entry[0]);
            entry[1].handler();
            await flush();
        }
        now = end;
    }
    function activity(type = "pointermove") {
        listeners.get(`document:${type}`)();
    }
    return { calls, document, elements, listeners, advance, flush, activity };
}

test("idle countdown resets on activity and starts a demo at exactly ten seconds", async () => {
    const ui = harness([board(1, 3, 1)]);
    await ui.advance(9_999);
    assert.equal(ui.calls.length, 0);
    ui.activity();
    await ui.advance(9_999);
    assert.equal(ui.calls.length, 0);
    await ui.advance(1);
    assert.equal(ui.calls.length, 1);
    assert.equal(ui.calls[0].body.mode, "intermediate");
    assert.equal(ui.elements.get("minesweeper-mode").value, "intermediate");
    ui.activity();
    await ui.advance(20_000);
    assert.equal(ui.calls.length, 1, "interaction must stop solving an ongoing demo");
});

test("idle demos randomly select each of the three ranked modes", async () => {
    for (const [random, mode] of [[0, "easy"], [0.5, "intermediate"], [0.99, "advanced"]]) {
        const game = board(1, 3, 1);
        game.mode = mode;
        const ui = harness([game], () => random);
        await ui.advance(10_000);
        assert.equal(ui.calls[0].body.mode, mode);
        assert.equal(ui.elements.get("minesweeper-mode").value, mode);
        assert.match(ui.elements.get("game-status").textContent, new RegExp(`Auto-solving ${mode}`));
    }
});

test("demo flags a calculated mine, reveals safe cells, and never submits scores", async () => {
    const initial = board(1, 3, 1, { 0: { revealed: true, count: 1 } });
    const flagged = structuredClone(initial);
    flagged.cells[1].flagged = true;
    const won = structuredClone(flagged);
    won.cells[2].revealed = true;
    won.status = "won";
    const ui = harness([initial, flagged, won]);
    await ui.advance(10_499);
    assert.equal(ui.calls.length, 1);
    await ui.advance(1);
    assert.equal(ui.calls.length, 2);
    await ui.advance(499);
    assert.equal(ui.calls.length, 2);
    await ui.advance(1);
    assert.deepEqual(ui.calls.map((call) => call.body.action), [undefined, "flag", "reveal"]);
    assert.match(ui.elements.get("game-status").textContent, /not ranked/);
    assert.equal(ui.elements.get("guest-score").hidden, true);
    assert.ok(ui.calls.every((call) => !call.url.endsWith("/score")));
});

test("manual games do not start auto-solving after long inactivity", async () => {
    const ui = harness([board(1, 3, 1)]);
    ui.activity("click");
    ui.listeners.get("new-game:click")();
    await ui.flush();
    await ui.advance(60_000);
    assert.equal(ui.calls.length, 1);
    assert.equal(ui.calls[0].body.mode, "easy");
});

test("New game waits for an in-flight solver move and starts a fresh board", async () => {
    let completeMove;
    const initial = board(1, 3, 1);
    const next = structuredClone(initial);
    next.cells[0].revealed = true;
    const ui = harness([
        initial,
        () => new Promise((resolve) => { completeMove = resolve; }),
        board(1, 3, 1),
    ]);
    await ui.advance(10_500);
    assert.equal(ui.calls.length, 2);
    ui.activity("click");
    ui.listeners.get("new-game:click")();
    assert.equal(ui.calls.length, 2);
    completeMove({ ok: true, json: async () => next });
    await ui.flush();
    assert.equal(ui.calls.length, 3);
    assert.equal(ui.calls[2].url, "/games/minesweeper/api/games");
    await ui.advance(60_000);
    assert.equal(ui.calls.length, 3);
});

test("interaction cancels an in-flight demo without launching another solver move", async () => {
    let completeMove;
    const initial = board(1, 3, 1);
    const next = structuredClone(initial);
    next.cells[0].revealed = true;
    const ui = harness([
        initial,
        () => new Promise((resolve) => { completeMove = resolve; }),
    ]);
    await ui.advance(10_500);
    ui.activity();
    completeMove({ ok: true, json: async () => next });
    await ui.flush();
    await ui.advance(60_000);
    assert.equal(ui.calls.length, 2);
    assert.match(ui.elements.get("game-status").textContent, /Auto-solving stopped/);
});

test("hidden pages do not start demos and resume the countdown when visible", async () => {
    const ui = harness([board(1, 3, 1)]);
    ui.document.hidden = true;
    ui.listeners.get("document:visibilitychange")();
    await ui.advance(20_000);
    assert.equal(ui.calls.length, 0);
    ui.document.hidden = false;
    ui.listeners.get("document:visibilitychange")();
    await ui.advance(10_000);
    assert.equal(ui.calls.length, 1);
});

test("a solver move failure is shown and stops automatic requests", async () => {
    const ui = harness([
        board(1, 3, 1),
        () => ({ ok: false, text: async () => "Move failed" }),
    ]);
    await ui.advance(10_500);
    assert.equal(ui.calls.length, 2);
    assert.equal(ui.elements.get("game-status").textContent, "Move failed");
    await ui.advance(60_000);
    assert.equal(ui.calls.length, 2);
});

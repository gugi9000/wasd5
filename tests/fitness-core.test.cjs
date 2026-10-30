const { test } = require("node:test");
const assert = require("node:assert/strict");
const { expand, preset, Runner } = require("../static/fitness-core.js");

test("EMOM X and Tabata have exact intervals and duration", () => {
    const emom = expand(preset("emom", 12));
    assert.equal(emom.length, 12);
    assert.equal(emom.reduce((sum, s) => sum + s.seconds, 0), 720);
    const tabata = expand(preset("tabata", 8));
    assert.deepEqual(tabata.map(s => s.seconds), Array(8).fill([20, 10]).flat());
    assert.equal(tabata.reduce((sum, s) => sum + s.seconds, 0), 240);
});

test("whole-workout, group, and step repeats preserve ordering", () => {
    const workout = { repeats: 2, blocks: [
        { repeats: 2, steps: [{ label: "A", seconds: 1, repeats: 2 }, { label: "B", seconds: null, repeats: 1 }] },
        { repeats: 1, steps: [{ label: "C", seconds: 1, repeats: 1 }] },
    ] };
    assert.deepEqual(expand(workout).map(s => s.label), ["A", "A", "B", "A", "A", "B", "C", "A", "A", "B", "A", "A", "B", "C"]);
});

test("pause preserves sub-second remaining time and restart resets", () => {
    const runner = new Runner(expand(preset("emom", 2)));
    runner.toggle(1000);
    runner.toggle(2500);
    assert.equal(runner.state, "paused");
    assert.equal(runner.remaining, 58500);
    runner.tick(100000);
    assert.equal(runner.remaining, 58500);
    runner.toggle(100000);
    runner.tick(158499);
    assert.equal(runner.index, 0);
    runner.tick(158500);
    assert.equal(runner.index, 1);
    assert.equal(runner.remaining, 60000);
    runner.reset();
    assert.equal(runner.state, "ready");
    assert.equal(runner.index, 0);
});

test("delayed ticks catch up timed steps but never consume a manual rest", () => {
    const runner = new Runner(expand({ repeats: 1, blocks: [{ repeats: 1, steps: [
        { label: "Work", seconds: 1, repeats: 2 },
        { label: "Rest", seconds: null, repeats: 2 },
        { label: "Finish", seconds: 2, repeats: 1 },
    ] }] }));
    runner.toggle(0);
    runner.tick(100000);
    assert.equal(runner.index, 2);
    assert.equal(runner.state, "waiting");
    runner.tick(200000);
    assert.equal(runner.index, 2);
    runner.toggle(300000);
    assert.equal(runner.index, 3);
    assert.equal(runner.state, "waiting");
    runner.toggle(400000);
    assert.equal(runner.state, "running");
    assert.equal(runner.remaining, 2000);
    runner.tick(402000);
    assert.equal(runner.state, "done");
    runner.toggle(500000);
    assert.equal(runner.state, "done");
});

test("pause at manual boundary does not skip the rest", () => {
    const runner = new Runner(expand(preset("custom", 1)));
    runner.toggle(0);
    runner.toggle(30000);
    assert.equal(runner.state, "waiting");
    assert.equal(runner.index, 1);
});

test("manual first steps and all-timed completion work", () => {
    const runner = new Runner([{ label: "Ready?", seconds: null }, { label: "Go", seconds: 1 }]);
    runner.toggle(0);
    assert.equal(runner.state, "waiting");
    runner.toggle(5000);
    runner.tick(6000);
    assert.equal(runner.state, "done");
    const timed = new Runner(expand(preset("tabata", 8)));
    timed.toggle(0);
    timed.tick(240000);
    assert.equal(timed.state, "done");
});

test("reject empty, invalid, and oversized expanded workouts", () => {
    for (const n of [0, -1, 101, 1.5, NaN]) assert.throws(() => expand(preset("emom", n)));
    assert.throws(() => expand({ repeats: 1, blocks: [] }));
    const oversized = preset("emom", 100);
    oversized.repeats = 100;
    oversized.blocks[0].steps[0].repeats = 2;
    assert.throws(() => expand(oversized), /10,000/);
    const boundary = preset("emom", 100);
    boundary.repeats = 100;
    assert.equal(expand(boundary).length, 10000);
    const invalid = preset("custom", 1);
    invalid.blocks[0].steps[0].seconds = 0;
    assert.throws(() => expand(invalid));
});

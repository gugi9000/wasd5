const { test } = require("node:test");
const assert = require("node:assert/strict");
const { WIDTH, HEIGHT, TYPES, createGame, step, cells, rotate } = require("../static/tetris-core.js");

function fixedGame(type) {
    const game = createGame(() => 0);
    game.bag = [type];
    const shape = {
        I: ["....", "####", "....", "...."],
        O: [".##.", ".##.", "....", "...."],
        T: [".#..", "###.", "....", "...."],
        S: [".##.", "##..", "....", "...."],
        Z: ["##..", ".##.", "....", "...."],
        J: ["#...", "###.", "....", "...."],
        L: ["..#.", "###.", "....", "...."],
    }[type];
    game.piece = { type, shape: shape.map((row) => [...row].map((value) => value === "#")), x: 3, y: 0 };
    game.next = { ...game.piece, shape: game.piece.shape.map((row) => [...row]) };
    return game;
}

test("board dimensions and piece bag use all seven tetrominoes", () => {
    const game = createGame(() => 0.5);
    assert.equal(game.board.length, HEIGHT);
    assert.ok(game.board.every((row) => row.length === WIDTH));
    const bag = game.bag.concat(game.piece.type, game.next.type);
    assert.deepEqual(new Set(bag).size, TYPES.length);
});

test("movement is bounded and soft drop awards points", () => {
    const game = fixedGame("I");
    for (let index = 0; index < 8; index += 1) step(game, "left");
    assert.equal(game.piece.x, 0);
    assert.equal(step(game, "soft-drop").score, 1);
    assert.equal(game.piece.y, 1);
});

test("hard drop locks the piece immediately and awards distance points", () => {
    const game = fixedGame("O");
    const startY = game.piece.y;
    step(game, "hard-drop");
    assert.equal(game.score, (HEIGHT - 2 - startY) * 2);
    assert.equal(game.board[HEIGHT - 2][4], "O");
    assert.equal(game.board[HEIGHT - 1][5], "O");
    assert.equal(game.piece.y, 0);
});

test("clockwise and counter-clockwise rotations are inverse operations", () => {
    const game = fixedGame("T");
    const original = JSON.stringify(game.piece.shape);
    rotate(game, -1);
    rotate(game, 1);
    assert.equal(JSON.stringify(game.piece.shape), original);
});

test("completed rows clear and score with the current level", () => {
    const game = fixedGame("O");
    game.piece = { ...game.piece, x: 3, y: 18 };
    for (let row = 18; row < HEIGHT; row += 1) {
        game.board[row].fill("J");
        game.board[row][4] = null;
        game.board[row][5] = null;
    }
    step(game, "fall");
    assert.equal(game.lines, 2);
    assert.equal(game.score, 300);
    assert.equal(game.board[0].every((cell) => cell === null), true);
});

test("top out ends the run", () => {
    const game = fixedGame("O");
    game.piece = { ...game.piece, x: 3, y: 0 };
    game.board[2][4] = "I";
    game.board[2][5] = "I";
    step(game, "fall");
    assert.equal(game.status, "over");
});

test("rendered cells include the falling piece without mutating the board", () => {
    const game = createGame(() => 0.25);
    const rendered = cells(game);
    assert.ok(rendered.flat().some((cell) => cell === game.piece.type));
    assert.ok(game.board.flat().every((cell) => cell === null));
});

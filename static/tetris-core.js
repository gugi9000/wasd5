(function (root, factory) {
    const api = factory();
    if (typeof module === "object" && module.exports) module.exports = api;
    else root.TetrisCore = api;
})(typeof globalThis === "undefined" ? this : globalThis, function () {
    "use strict";

    const WIDTH = 10;
    const HEIGHT = 20;
    const TYPES = ["I", "O", "T", "S", "Z", "J", "L"];
    const SHAPES = {
        I: ["....", "####", "....", "...."],
        O: [".##.", ".##.", "....", "...."],
        T: [".#..", "###.", "....", "...."],
        S: [".##.", "##..", "....", "...."],
        Z: ["##..", ".##.", "....", "...."],
        J: ["#...", "###.", "....", "...."],
        L: ["..#.", "###.", "....", "...."],
    };

    function matrix(type) {
        return SHAPES[type].map((row) => [...row].map((cell) => cell === "#"));
    }

    function shuffledBag(random) {
        const bag = [...TYPES];
        for (let index = bag.length - 1; index > 0; index -= 1) {
            const swap = Math.floor(random() * (index + 1));
            [bag[index], bag[swap]] = [bag[swap], bag[index]];
        }
        return bag;
    }

    function takePiece(game) {
        if (game.bag.length === 0) game.bag = shuffledBag(game.random);
        const type = game.bag.pop();
        return { type, shape: matrix(type), x: 3, y: -1 };
    }

    function createGame(random = Math.random) {
        const game = {
            board: Array.from({ length: HEIGHT }, () => Array(WIDTH).fill(null)),
            bag: [],
            random,
            piece: null,
            next: null,
            score: 0,
            lines: 0,
            level: 1,
            status: "playing",
        };
        game.piece = takePiece(game);
        game.next = takePiece(game);
        if (collides(game, game.piece)) game.status = "over";
        return game;
    }

    function collides(game, piece) {
        for (let y = 0; y < 4; y += 1) {
            for (let x = 0; x < 4; x += 1) {
                if (!piece.shape[y][x]) continue;
                const boardX = piece.x + x;
                const boardY = piece.y + y;
                if (
                    boardX < 0 ||
                    boardX >= WIDTH ||
                    boardY >= HEIGHT ||
                    (boardY >= 0 && game.board[boardY][boardX] !== null)
                ) return true;
            }
        }
        return false;
    }

    function move(game, dx, dy) {
        if (game.status !== "playing") return false;
        const next = { ...game.piece, x: game.piece.x + dx, y: game.piece.y + dy };
        if (collides(game, next)) return false;
        game.piece = next;
        return true;
    }

    function rotate(game, direction) {
        if (game.status !== "playing" || game.piece.type === "O") return false;
        const shape = game.piece.shape.map((_, row) =>
            game.piece.shape.map((line, column) =>
                direction < 0
                    ? game.piece.shape[column][3 - row]
                    : game.piece.shape[3 - column][row],
            ),
        );
        for (const offset of [0, -1, 1, -2, 2]) {
            const next = { ...game.piece, shape, x: game.piece.x + offset };
            if (!collides(game, next)) {
                game.piece = next;
                return true;
            }
        }
        return false;
    }

    function lock(game) {
        const piece = game.piece;
        for (let y = 0; y < 4; y += 1) {
            for (let x = 0; x < 4; x += 1) {
                if (!piece.shape[y][x]) continue;
                const boardY = piece.y + y;
                const boardX = piece.x + x;
                if (boardY < 0) {
                    game.status = "over";
                    return;
                }
                game.board[boardY][boardX] = piece.type;
            }
        }
        const remaining = game.board.filter((row) => row.some((cell) => cell === null));
        const cleared = HEIGHT - remaining.length;
        if (cleared > 0) {
            game.board = [
                ...Array.from({ length: cleared }, () => Array(WIDTH).fill(null)),
                ...remaining,
            ];
            game.score += [0, 100, 300, 500, 800][cleared] * game.level;
            game.lines += cleared;
            game.level = Math.floor(game.lines / 10) + 1;
        }
        game.piece = game.next;
        game.next = takePiece(game);
        if (collides(game, game.piece)) game.status = "over";
    }

    function step(game, action = "fall") {
        if (game.status !== "playing") return game;
        if (action === "left") move(game, -1, 0);
        else if (action === "right") move(game, 1, 0);
        else if (action === "soft-drop") {
            if (move(game, 0, 1)) game.score += 1;
            else lock(game);
        } else if (action === "hard-drop") {
            let distance = 0;
            while (move(game, 0, 1)) distance += 1;
            game.score += distance * 2;
            lock(game);
        } else if (action === "rotate-ccw") rotate(game, -1);
        else if (action === "rotate-cw") rotate(game, 1);
        else if (action === "fall" && !move(game, 0, 1)) lock(game);
        else if (action !== "fall") throw new Error(`Unknown Tetris action: ${action}`);
        return game;
    }

    function cells(game) {
        const board = game.board.map((row) => [...row]);
        if (game.piece && game.status === "playing") {
            for (let y = 0; y < 4; y += 1) {
                for (let x = 0; x < 4; x += 1) {
                    const boardY = game.piece.y + y;
                    const boardX = game.piece.x + x;
                    if (game.piece.shape[y][x] && boardY >= 0 && boardY < HEIGHT) {
                        board[boardY][boardX] = game.piece.type;
                    }
                }
            }
        }
        return board;
    }

    return { WIDTH, HEIGHT, TYPES, createGame, step, cells, rotate, collides };
});

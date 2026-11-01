(function (root, factory) {
    const solver = factory();
    if (typeof module === "object" && module.exports) module.exports = solver;
    else root.MinesweeperSolver = solver;
})(typeof globalThis === "object" ? globalThis : this, function () {
    "use strict";

    function nextMove(game, random = Math.random) {
        if (game.status !== "playing") return null;
        const covered = [];
        const constraints = [];
        let flags = 0;
        for (let index = 0; index < game.cells.length; index += 1) {
            const cell = game.cells[index];
            if (cell.flagged) flags += 1;
            if (!cell.revealed && !cell.flagged) covered.push(index);
            if (!cell.revealed) continue;
            const row = Math.floor(index / game.columns);
            const column = index % game.columns;
            const unknown = [];
            let adjacentFlags = 0;
            for (let r = Math.max(0, row - 1); r <= Math.min(game.rows - 1, row + 1); r += 1) {
                for (let c = Math.max(0, column - 1); c <= Math.min(game.columns - 1, column + 1); c += 1) {
                    if (r === row && c === column) continue;
                    const neighborIndex = r * game.columns + c;
                    const neighbor = game.cells[neighborIndex];
                    if (neighbor.flagged) adjacentFlags += 1;
                    else if (!neighbor.revealed) unknown.push(neighborIndex);
                }
            }
            if (unknown.length) constraints.push({
                cells: unknown,
                mines: cell.count - adjacentFlags,
            });
        }
        if (!covered.length) return null;
        constraints.push({ cells: covered, mines: game.mine_count - flags });

        for (const constraint of constraints) {
            if (constraint.mines === constraint.cells.length) {
                return { action: "flag", index: constraint.cells[0], guessed: false };
            }
            if (constraint.mines === 0) {
                return { action: "reveal", index: constraint.cells[0], guessed: false };
            }
        }
        for (const smaller of constraints) {
            for (const larger of constraints) {
                if (smaller.cells.length >= larger.cells.length) continue;
                if (!smaller.cells.every((index) => larger.cells.includes(index))) continue;
                const difference = larger.cells.filter((index) => !smaller.cells.includes(index));
                const mines = larger.mines - smaller.mines;
                if (mines === difference.length) {
                    return { action: "flag", index: difference[0], guessed: false };
                }
                if (mines === 0) {
                    return { action: "reveal", index: difference[0], guessed: false };
                }
            }
        }
        return {
            action: "reveal",
            index: covered[Math.floor(random() * covered.length)],
            guessed: true,
        };
    }

    return { nextMove };
});

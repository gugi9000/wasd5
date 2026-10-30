(function () {
    "use strict";

    const root = document.getElementById("tetris");
    if (!root || !window.TetrisCore) return;

    const { WIDTH, HEIGHT, createGame, step, cells } = window.TetrisCore;
    const csrf = root.dataset.csrf;
    const signedIn = root.dataset.signedIn === "true";
    const username = root.dataset.username || "";
    const boardElement = document.getElementById("tetris-board");
    const nextElement = document.getElementById("tetris-next");
    const statusElement = document.getElementById("tetris-status");
    const startButton = document.getElementById("tetris-start");
    const scoreForm = document.getElementById("tetris-score-form");
    const scoreMessage = document.getElementById("tetris-score-message");
    let game = null;
    let runId = null;
    let loop = null;
    let lastFrame = 0;
    let fallAccumulator = 0;
    let scoreSubmitted = false;
    let busy = false;

    async function post(url, data) {
        const response = await fetch(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ ...data, csrf }),
        });
        if (!response.ok) {
            const message = await response.text();
            throw new Error(message || `Request failed (${response.status})`);
        }
        return response.json();
    }

    function render() {
        if (!game) return;
        const board = cells(game);
        boardElement.replaceChildren();
        for (let row = 0; row < HEIGHT; row += 1) {
            for (let column = 0; column < WIDTH; column += 1) {
                const cell = document.createElement("div");
                cell.className = "tetris-cell";
                cell.setAttribute("role", "gridcell");
                if (board[row][column]) {
                    cell.classList.add("filled", `block-${board[row][column]}`);
                }
                boardElement.append(cell);
            }
        }
        nextElement.replaceChildren();
        for (const row of game.next.shape) {
            for (const occupied of row) {
                const cell = document.createElement("span");
                cell.className = "tetris-mini-cell";
                if (occupied) cell.classList.add(`block-${game.next.type}`);
                nextElement.append(cell);
            }
        }
        document.getElementById("tetris-score").textContent = String(game.score);
        document.getElementById("tetris-lines").textContent = String(game.lines);
        document.getElementById("tetris-level").textContent = String(game.level);
    }

    function stopGame() {
        if (loop !== null) cancelAnimationFrame(loop);
        loop = null;
    }

    function onGameOver() {
        stopGame();
        startButton.disabled = false;
        startButton.textContent = "Play again";
        statusElement.textContent = `Game over — ${game.score} points and ${game.lines} lines.`;
        scoreForm.hidden = signedIn;
        if (signedIn) void submitScore(null);
        else document.getElementById("tetris-player-name").focus();
    }

    async function submitScore(playerName) {
        if (scoreSubmitted || busy || !runId || !game) return;
        busy = true;
        try {
            await post(`/games/tetris/api/runs/${runId}/score`, {
                player_name: playerName,
                score: game.score,
                lines: game.lines,
                level: game.level,
            });
            scoreSubmitted = true;
            scoreForm.hidden = true;
            scoreMessage.textContent = signedIn
                ? `Score saved for trusted account ${username}.`
                : "Score saved as an unverified guest name.";
            await refreshLeaderboard();
        } catch (error) {
            scoreMessage.textContent = error.message;
            scoreForm.hidden = !signedIn;
        } finally {
            busy = false;
        }
    }

    async function refreshLeaderboard() {
        try {
            const response = await fetch("/games/tetris/api/scores", {
                headers: { Accept: "application/json" },
            });
            if (!response.ok) throw new Error("Could not load the leaderboard.");
            const scores = await response.json();
            const body = document.getElementById("tetris-leaderboard");
            body.replaceChildren();
            if (scores.length === 0) {
                const row = body.insertRow();
                const cell = row.insertCell();
                cell.colSpan = 5;
                cell.className = "text-muted";
                cell.textContent = "No scores yet.";
                return;
            }
            scores.forEach((score, index) => {
                const row = body.insertRow();
                row.insertCell().textContent = String(index + 1);
                const player = row.insertCell();
                player.append(document.createTextNode(`${score.player_name} `));
                if (score.user_id !== null) {
                    const badge = document.createElement("span");
                    badge.className = "badge text-bg-success";
                    badge.title = "Signed-in account";
                    badge.textContent = "Account";
                    player.append(badge);
                }
                row.insertCell().textContent = String(score.score);
                row.insertCell().textContent = String(score.lines);
                row.insertCell().textContent = String(score.level);
            });
        } catch (error) {
            scoreMessage.textContent = error.message;
        }
    }

    function animate(timestamp) {
        if (!game || game.status !== "playing") return;
        if (lastFrame === 0) lastFrame = timestamp;
        fallAccumulator += timestamp - lastFrame;
        lastFrame = timestamp;
        const interval = Math.max(80, 800 * Math.pow(0.82, game.level - 1));
        if (fallAccumulator >= interval) {
            fallAccumulator %= interval;
            step(game, "fall");
            render();
            if (game.status === "over") {
                onGameOver();
                return;
            }
        }
        loop = requestAnimationFrame(animate);
    }

    function applyAction(action) {
        if (!game || game.status !== "playing") return;
        step(game, action);
        render();
        if (game.status === "over") onGameOver();
    }

    startButton.addEventListener("click", async () => {
        if (busy) return;
        busy = true;
        startButton.disabled = true;
        stopGame();
        game = null;
        runId = null;
        scoreSubmitted = false;
        scoreForm.hidden = true;
        scoreMessage.textContent = "";
        statusElement.textContent = "Starting a new game…";
        try {
            const result = await post("/games/tetris/api/runs", {});
            runId = result.id;
            game = createGame();
            fallAccumulator = 0;
            lastFrame = 0;
            render();
            statusElement.textContent = "Game in progress. Use H/J/K/L or the arrow keys.";
            startButton.textContent = "Restart game";
            loop = requestAnimationFrame(animate);
        } catch (error) {
            statusElement.textContent = error.message;
            startButton.disabled = false;
        } finally {
            busy = false;
        }
    });

    document.addEventListener("keydown", (event) => {
        if (!game || game.status !== "playing" || event.altKey || event.ctrlKey || event.metaKey) return;
        if (["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName)) return;
        const actions = {
            h: "left",
            ArrowLeft: "left",
            j: "soft-drop",
            ArrowDown: "soft-drop",
            l: "right",
            ArrowRight: "right",
            k: "rotate-ccw",
            ArrowUp: "rotate-ccw",
            i: "rotate-cw",
            " ": "hard-drop",
            Enter: "rotate-cw",
        };
        const action = actions[event.key];
        if (!action) return;
        event.preventDefault();
        applyAction(action);
    });

    document.querySelectorAll(".tetris-touch-controls [data-action]").forEach((button) => {
        button.addEventListener("click", () => applyAction(button.dataset.action));
    });

    scoreForm.addEventListener("submit", (event) => {
        event.preventDefault();
        void submitScore(document.getElementById("tetris-player-name").value);
    });
})();

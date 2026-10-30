(function () {
    "use strict";

    const root = document.getElementById("minesweeper");
    if (!root) return;

    const csrf = root.dataset.csrf;
    const signedIn = root.dataset.signedIn === "true";
    const username = root.dataset.username || "";
    const modeSelect = document.getElementById("minesweeper-mode");
    const customOptions = document.getElementById("custom-options");
    const boardHost = document.getElementById("board-scroll");
    const status = document.getElementById("game-status");
    const mineCounter = document.getElementById("mine-counter");
    const timeLabel = document.getElementById("game-time");
    const guestForm = document.getElementById("guest-score");
    const scoreMessage = document.getElementById("score-message");
    const flagModeButton = document.getElementById("flag-mode");
    let game = null;
    let startedAt = 0;
    let timer = null;
    let busy = false;
    let flagMode = false;
    let scoreSent = false;

    modeSelect.addEventListener("change", () => {
        customOptions.hidden = modeSelect.value !== "custom";
    });

    function setStatus(message) {
        status.textContent = message;
    }

    async function request(url, body) {
        const response = await fetch(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ ...body, csrf }),
        });
        if (!response.ok) {
            const message = await response.text();
            throw new Error(message || `Request failed (${response.status})`);
        }
        return response.json();
    }

    function elapsedText(ms) {
        return `${(ms / 1000).toFixed(1)} s`;
    }

    function drawBoard() {
        if (!game) {
            boardHost.replaceChildren();
            return;
        }
        const table = document.createElement("table");
        table.className = "minesweeper-board";
        table.setAttribute("role", "grid");
        table.setAttribute("aria-label", `${game.mode} Minesweeper board`);
        for (let row = 0; row < game.rows; row += 1) {
            const tr = table.insertRow();
            for (let column = 0; column < game.columns; column += 1) {
                const index = row * game.columns + column;
                const cell = game.cells[index];
                const td = tr.insertCell();
                const button = document.createElement("button");
                button.type = "button";
                button.className = "minesweeper-cell";
                button.dataset.index = String(index);
                button.setAttribute("role", "gridcell");
                if (cell.revealed) {
                    button.classList.add("revealed");
                    if (cell.mine) {
                        button.textContent = "✹";
                        button.classList.add("mine-hit");
                    } else if (cell.count > 0) {
                        button.textContent = String(cell.count);
                        button.classList.add(`number-${cell.count}`);
                    }
                } else if (cell.flagged) {
                    button.textContent = "⚑";
                    button.classList.add("flagged");
                }
                const description = cell.mine && cell.revealed
                    ? "mine"
                    : cell.revealed
                      ? cell.count > 0 ? `number ${cell.count}` : "empty"
                      : cell.flagged ? "flagged" : "covered";
                button.setAttribute("aria-label", `Row ${row + 1}, column ${column + 1}: ${description}`);
                button.disabled = game.status !== "playing" || busy;
                td.append(button);
            }
        }
        boardHost.replaceChildren(table);
        const flags = game.cells.filter((cell) => cell.flagged).length;
        mineCounter.textContent = String(game.mine_count - flags);
    }

    async function refreshLeaderboards() {
        try {
            const response = await fetch("/games/minesweeper/api/scores", {
                headers: { Accept: "application/json" },
            });
            if (!response.ok) throw new Error("Could not load leaderboards.");
            const boards = await response.json();
            for (const mode of ["easy", "intermediate", "advanced"]) {
                const body = document.getElementById(`scores-${mode}`);
                body.replaceChildren();
                if (boards[mode].length === 0) {
                    const row = body.insertRow();
                    const empty = row.insertCell();
                    empty.colSpan = 3;
                    empty.className = "text-muted";
                    empty.textContent = "No scores yet.";
                    continue;
                }
                boards[mode].forEach((score, index) => {
                    const row = body.insertRow();
                    row.insertCell().textContent = String(index + 1);
                    const player = row.insertCell();
                    player.append(document.createTextNode(score.player_name + " "));
                    if (score.user_id !== null) {
                        const badge = document.createElement("span");
                        badge.className = "badge text-bg-success";
                        badge.title = "Signed-in account";
                        badge.textContent = "Account";
                        player.append(badge);
                    }
                    row.insertCell().textContent = elapsedText(score.elapsed_ms);
                });
            }
        } catch (error) {
            scoreMessage.textContent = error.message;
        }
    }

    async function sendScore(playerName) {
        if (scoreSent || !game) return;
        try {
            await request(`/games/minesweeper/api/games/${game.id}/score`, {
                player_name: playerName,
            });
            scoreSent = true;
            guestForm.hidden = true;
            scoreMessage.textContent = signedIn
                ? `Score saved for trusted account ${username}.`
                : "Score saved as an unverified guest name.";
            await refreshLeaderboards();
        } catch (error) {
            scoreMessage.textContent = error.message;
        }
    }

    function finishGame() {
        clearInterval(timer);
        if (game.status === "lost") {
            setStatus("Mine hit — game over. Start a new game to try again.");
            guestForm.hidden = true;
        } else if (game.status === "won") {
            timeLabel.textContent = elapsedText(game.elapsed_ms);
            if (game.mode === "custom") {
                setStatus("Board cleared! Custom games are not ranked.");
                guestForm.hidden = true;
            } else if (signedIn) {
                setStatus(`Board cleared! Saving score for trusted account ${username}…`);
                void sendScore(null);
            } else {
                setStatus("Board cleared! Enter a name to submit your score as a guest.");
                guestForm.hidden = false;
                document.getElementById("guest-name").focus();
            }
        }
    }

    async function move(action, index) {
        if (!game || busy || game.status !== "playing") return;
        const row = Math.floor(index / game.columns);
        const column = index % game.columns;
        busy = true;
        drawBoard();
        try {
            game = await request(`/games/minesweeper/api/games/${game.id}/move`, {
                action,
                row,
                column,
            });
            drawBoard();
            if (game.status === "won" || game.status === "lost") {
                finishGame();
            }
        } catch (error) {
            setStatus(error.message);
        } finally {
            busy = false;
            drawBoard();
        }
    }

    const newGameButton = document.getElementById("new-game");
    newGameButton.addEventListener("click", async () => {
        if (busy) return;
        busy = true;
        newGameButton.disabled = true;
        clearInterval(timer);
        game = null;
        drawBoard();
        scoreSent = false;
        guestForm.hidden = true;
        scoreMessage.textContent = "";
        timeLabel.textContent = "0.0 s";
        setStatus("Starting a new board…");
        const body = { mode: modeSelect.value };
        if (body.mode === "custom") {
            body.rows = Number(document.getElementById("custom-rows").value);
            body.columns = Number(document.getElementById("custom-columns").value);
            body.mine_count = Number(document.getElementById("custom-mines").value);
        }
        try {
            game = await request("/games/minesweeper/api/games", body);
            startedAt = Date.now();
            setStatus("Good luck! Left-click to reveal, right-click to flag, middle-click to chord.");
            drawBoard();
            timer = setInterval(() => {
                if (game?.status === "playing") {
                    timeLabel.textContent = elapsedText(Date.now() - startedAt);
                }
            }, 100);
        } catch (error) {
            setStatus(error.message);
            drawBoard();
        } finally {
            busy = false;
            newGameButton.disabled = false;
            drawBoard();
        }
    });

    boardHost.addEventListener("pointerdown", (event) => {
        const button = event.target.closest("button[data-index]");
        if (!button) return;
        if (event.pointerType !== "mouse") {
            if (event.button === 0) {
                event.preventDefault();
                void move(flagMode ? "flag" : "reveal", Number(button.dataset.index));
            }
            return;
        }
        event.preventDefault();
        const action = event.button === 2 ? "flag" : event.button === 1 ? "chord" : flagMode ? "flag" : "reveal";
        void move(action, Number(button.dataset.index));
    });
    boardHost.addEventListener("click", (event) => {
        const button = event.target.closest("button[data-index]");
        if (!button || event.detail !== 0) return;
        void move(flagMode ? "flag" : "reveal", Number(button.dataset.index));
    });
    boardHost.addEventListener("contextmenu", (event) => {
        if (event.target.closest("button[data-index]")) event.preventDefault();
    });

    flagModeButton.addEventListener("click", () => {
        flagMode = !flagMode;
        flagModeButton.setAttribute("aria-pressed", String(flagMode));
        flagModeButton.textContent = `Tap to flag: ${flagMode ? "on" : "off"}`;
    });

    guestForm.addEventListener("submit", (event) => {
        event.preventDefault();
        void sendScore(document.getElementById("guest-name").value);
    });
})();

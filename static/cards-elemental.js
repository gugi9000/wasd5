(function () {
    "use strict";

    const root = document.getElementById("cards-elemental");
    if (!root) return;

    const csrf = root.dataset.csrf;
    const status = document.getElementById("cards-status");
    const hand = document.getElementById("cards-hand");
    const result = document.getElementById("cards-round-result");
    const error = document.getElementById("cards-error");
    const newGameButton = document.getElementById("cards-new-game");
    const opponentSelect = document.getElementById("cards-opponent");
    const publicGameButton = document.getElementById("cards-start-public");
    const privateGameButton = document.getElementById("cards-start-private");
    const joinButton = document.getElementById("cards-join-button");
    const joinId = document.getElementById("cards-join-id");
    const openGames = document.getElementById("cards-open-games");
    const noOpenGames = document.getElementById("cards-no-open-games");
    const lobby = document.getElementById("cards-online-lobby");
    const gameIdSection = document.getElementById("cards-game-id-section");
    const gameIdDisplay = document.getElementById("cards-game-id");
    const copyGameIdButton = document.getElementById("cards-copy-game-id");
    let game = null;
    let busy = false;

    async function request(url, body) {
        const options = { headers: { Accept: "application/json" } };
        if (body) {
            options.method = "POST";
            options.headers["Content-Type"] = "application/json";
            options.body = JSON.stringify({ ...body, csrf });
        }
        const response = await fetch(url, options);
        if (!response.ok) {
            throw new Error((await response.text()) || `Request failed (${response.status})`);
        }
        return response.json();
    }

    async function refreshStats() {
        try {
            const statistics = await request("/games/cards-elemental/api/stats");
            for (const stat of statistics) {
                document.getElementById(`cards-stat-${stat.category}`).textContent = String(stat.completed);
            }
        } catch (reason) {
            error.textContent = reason.message;
        }
    }

    function cardElement(card, compact = false) {
        const element = document.createElement("div");
        element.className = `elemental-card ${card.element}${compact ? " compact" : ""}`;
        element.setAttribute("aria-label", `${card.value} of ${card.colour} ${card.element}`);
        const value = document.createElement("span");
        value.className = "card-value";
        value.textContent = String(card.value);
        const kind = document.createElement("img");
        kind.className = "card-element";
        const icons = { fire: "ild", ice: "is", water: "vand" };
        kind.src = `/static/cards-elemental/${icons[card.element]}.svg`;
        kind.alt = card.element;
        kind.width = 48;
        kind.height = 48;
        const colour = document.createElement("span");
        colour.className = `card-colour ${card.colour}`;
        colour.textContent = card.colour;
        element.append(value, kind, colour);
        return element;
    }

    function renderWinnings(containerId, countId, cards) {
        const container = document.getElementById(containerId);
        container.replaceChildren(...cards.map((card) => cardElement(card, true)));
        document.getElementById(countId).textContent = String(cards.length);
    }

    function playedCard(label, card) {
        const container = document.createElement("div");
        container.className = "cards-played-card";
        const heading = document.createElement("strong");
        heading.textContent = label;
        container.append(heading, cardElement(card, true));
        return container;
    }

    function winningRule(cards) {
        if (new Set(cards.map((card) => card.element)).size === 3) {
            return "collected all three elements: fire, ice, and water";
        }
        for (const card of cards) {
            if (cards.filter((other) => other.element === card.element && other.colour === card.colour).length >= 3) {
                return `collected three ${card.colour} ${card.element} cards`;
            }
        }
        return "collected a winning set";
    }

    function renderResult(round, pendingCard) {
        result.replaceChildren();
        if (pendingCard) {
            result.hidden = false;
            result.append(playedCard("You played", pendingCard));
            const waiting = document.createElement("p");
            waiting.className = "w-100 text-center mb-0";
            waiting.textContent = "Waiting for your opponent to respond.";
            result.append(waiting);
            return;
        }
        if (!round) {
            result.hidden = true;
            return;
        }
        result.hidden = false;
        const you = playedCard("You played", round.you_played);
        const versus = document.createElement("strong");
        versus.className = "text-muted";
        versus.textContent = "vs";
        const opponent = playedCard("Opponent played", round.server_played);
        const outcome = document.createElement("strong");
        outcome.className = "w-100 text-center";
        if (round.winner === "you") outcome.textContent = "You win the round!";
        else if (round.winner === "server") outcome.textContent = "Your opponent wins the round.";
        else outcome.textContent = "Draw — both cards are discarded.";
        const explanation = document.createElement("p");
        explanation.className = "w-100 text-center mb-0";
        const winningCard = round.winner === "you" ? round.you_played : round.server_played;
        const losingCard = round.winner === "you" ? round.server_played : round.you_played;
        if (!round.winner) {
            explanation.textContent = `Same element (${round.you_played.element}) and equal values (${round.you_played.value}): a draw.`;
        } else if (winningCard.element === losingCard.element) {
            explanation.textContent = `Same element (${winningCard.element}): ${winningCard.value} beats ${losingCard.value}.`;
        } else {
            explanation.textContent = `${winningCard.element[0].toUpperCase()}${winningCard.element.slice(1)} beats ${losingCard.element}.`;
        }
        result.append(you, versus, opponent, outcome, explanation);
    }

    function setControlsDisabled(disabled) {
        opponentSelect.disabled = disabled;
        for (const button of [newGameButton, publicGameButton, privateGameButton, joinButton]) {
            button.disabled = disabled;
        }
        openGames.querySelectorAll("button").forEach((button) => {
            button.disabled = disabled;
        });
    }

    function render() {
        lobby.hidden = game?.status === "playing";
        gameIdSection.hidden = game?.mode !== "online" || game?.status !== "waiting" || !game?.game_id;
        if (!game) {
            hand.replaceChildren();
            setControlsDisabled(busy);
            return;
        }
        hand.replaceChildren();
        const canPlay = game.status === "playing"
            && !game.waiting_for_opponent
            && !game.your_card_pending
            && (game.server_has_led || game.starter === "you");
        game.your_hand.forEach((card, index) => {
            const cardDisplay = cardElement(card);
            const playButton = document.createElement("button");
            playButton.type = "button";
            playButton.className = cardDisplay.className;
            playButton.setAttribute("aria-label", `Play ${card.value} of ${card.colour} ${card.element}`);
            playButton.disabled = busy || !canPlay;
            for (const child of [...cardDisplay.childNodes]) {
                playButton.append(child.cloneNode(true));
            }
            playButton.addEventListener("click", () => void playCard(index));
            hand.append(playButton);
        });
        renderWinnings("your-winnings", "your-winnings-count", game.your_winnings);
        renderWinnings("server-winnings", "server-winnings-count", game.server_winnings);
        renderResult(game.last_round, game.your_played_card);
        document.getElementById("cards-deck-count").textContent = String(game.deck_size);
        if (game.game_id) gameIdDisplay.textContent = game.game_id;
        if (game.mode === "solo") opponentSelect.value = game.opponent;

        const hint = document.getElementById("cards-turn-hint");
        if (["won", "lost", "draw"].includes(game.status)) void refreshStats();
        if (game.status === "waiting") {
            status.className = "alert alert-info";
            status.textContent = "Waiting for an opponent. Share the game ID above to invite someone.";
            hint.textContent = "The game starts when another player joins.";
        } else if (game.status === "won") {
            status.className = "alert alert-success";
            status.textContent = `You ${winningRule(game.your_winnings)}! You win the game.`;
            hint.textContent = "Start another game to play again.";
        } else if (game.status === "lost") {
            status.className = "alert alert-danger";
            status.textContent = `Your opponent ${winningRule(game.server_winnings)}. Game over.`;
            hint.textContent = "Start another game to play again.";
        } else if (game.status === "draw") {
            status.className = "alert alert-secondary";
            status.textContent = "No cards remain to draw and neither player has won. The game is a draw.";
            hint.textContent = "Start another game to play again.";
        } else {
            status.className = "alert alert-secondary";
            if (game.mode === "online") {
                if (game.your_card_pending) {
                    status.textContent = `Round ${game.round}: your card is hidden while your opponent chooses a response.`;
                    hint.textContent = "Waiting for your opponent.";
                } else if (game.server_has_led) {
                    status.textContent = `Round ${game.round}: your opponent has played a hidden card. Choose your response.`;
                    hint.textContent = "Choose one card to resolve the round.";
                } else if (game.starter === "you") {
                    status.textContent = `Round ${game.round}: you lead. Choose a card to play.`;
                    hint.textContent = "Your card will stay hidden until your opponent responds.";
                } else {
                    status.textContent = `Round ${game.round}: your opponent leads.`;
                    hint.textContent = "Waiting for your opponent to play.";
                }
            } else {
                const opponentName = { tosser: "Tosser", pro: "Pro", pro2: "Pro2" }[game.opponent];
                status.textContent = game.server_has_led
                    ? `Round ${game.round}: ${opponentName} has led with a hidden card. Choose your response.`
                    : `Round ${game.round}: you lead against ${opponentName}. Choose a card to play.`;
                hint.textContent = `Choose one card. ${opponentName} will play, then the round is revealed.`;
            }
        }
        setControlsDisabled(busy);
    }

    async function playCard(index) {
        if (busy || !game || game.status !== "playing") return;
        busy = true;
        error.textContent = "";
        render();
        try {
            const url = game.mode === "online"
                ? "/games/cards-elemental/api/online/play"
                : "/games/cards-elemental/api/game/play";
            game = await request(url, { card_index: index });
            render();
        } catch (reason) {
            error.textContent = reason.message;
        } finally {
            busy = false;
            render();
            void refreshOpenGames();
        }
    }

    async function createGame(visibility) {
        if (busy) return;
        busy = true;
        error.textContent = "";
        status.textContent = visibility ? "Creating online game…" : "Shuffling and dealing…";
        setControlsDisabled(true);
        try {
            game = visibility
                ? await request("/games/cards-elemental/api/online", { visibility })
                : await request("/games/cards-elemental/api/games", { opponent: opponentSelect.value });
            render();
        } catch (reason) {
            error.textContent = reason.message;
            status.textContent = "Could not start a game.";
        } finally {
            busy = false;
            render();
            void refreshOpenGames();
        }
    }

    async function joinGame(id) {
        if (busy) return;
        const normalizedId = id.trim();
        if (!normalizedId) {
            error.textContent = "Enter a game ID.";
            return;
        }
        busy = true;
        error.textContent = "";
        setControlsDisabled(true);
        try {
            game = await request(
                `/games/cards-elemental/api/online/${encodeURIComponent(normalizedId)}/join`,
                {}
            );
            render();
        } catch (reason) {
            error.textContent = reason.message;
        } finally {
            busy = false;
            render();
            void refreshOpenGames();
        }
    }

    async function refreshOpenGames() {
        try {
            const response = await request("/games/cards-elemental/api/online/open");
            const games = response.games.filter((entry) => entry.id !== game?.game_id);
            openGames.replaceChildren(...games.map((entry) => {
                const item = document.createElement("li");
                item.className = "list-group-item d-flex align-items-center justify-content-between gap-2";
                const label = document.createElement("code");
                label.textContent = entry.id;
                const button = document.createElement("button");
                button.type = "button";
                button.className = "btn btn-sm btn-outline-primary";
                button.textContent = "Join";
                button.disabled = busy;
                button.addEventListener("click", () => void joinGame(entry.id));
                item.append(label, button);
                return item;
            }));
            noOpenGames.hidden = games.length > 0;
        } catch (reason) {
            error.textContent = reason.message;
        }
    }

    async function refreshOnlineGame() {
        if (busy || game?.mode !== "online") return;
        try {
            const latest = await request("/games/cards-elemental/api/online/current");
            if (latest && JSON.stringify(latest) !== JSON.stringify(game)) {
                game = latest;
                render();
                void refreshOpenGames();
            }
        } catch (reason) {
            error.textContent = reason.message;
        }
    }

    newGameButton.addEventListener("click", () => void createGame(null));
    publicGameButton.addEventListener("click", () => void createGame("public"));
    privateGameButton.addEventListener("click", () => void createGame("private"));
    joinButton.addEventListener("click", () => void joinGame(joinId.value));
    joinId.addEventListener("keydown", (event) => {
        if (event.key === "Enter") void joinGame(joinId.value);
    });
    copyGameIdButton.addEventListener("click", async () => {
        if (!game?.game_id) return;
        try {
            await navigator.clipboard.writeText(game.game_id);
            copyGameIdButton.textContent = "Copied";
        } catch (reason) {
            error.textContent = `Could not copy the game ID: ${reason.message}`;
        }
    });

    (async () => {
        try {
            game = await request("/games/cards-elemental/api/online/current");
            if (!game) game = await request("/games/cards-elemental/api/game");
            render();
        } catch (reason) {
            error.textContent = reason.message;
        }
        await refreshOpenGames();
    })();

    window.setInterval(() => void refreshOnlineGame(), 2500);
    window.setInterval(() => void refreshOpenGames(), 10000);
})();

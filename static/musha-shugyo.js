(function () {
    "use strict";

    const root = document.getElementById("musha-shugyo");
    if (!root || !window.MushaShugyoCore) return;

    const core = window.MushaShugyoCore;
    const board = document.getElementById("musha-board");
    const turn = document.getElementById("musha-turn");
    const roomLabel = document.getElementById("musha-room");
    const status = document.getElementById("musha-status");
    const positionNote = document.getElementById("musha-position-note");
    const zone = document.getElementById("musha-zone");
    const journal = document.getElementById("musha-journal");
    const knowledge = document.getElementById("musha-player-knowledge");
    const mood = document.getElementById("musha-creature-mood");
    const inventory = document.getElementById("musha-inventory");
    const checkpoint = document.getElementById("musha-checkpoint");
    const combatStats = document.getElementById("musha-combat-stats");
    const combatResult = document.getElementById("musha-combat-result");
    const gamePanel = document.getElementById("musha-game");
    const journeyStatus = document.getElementById("musha-journey-status");
    const startButton = document.getElementById("musha-start-button");
    const actionButtons = [...document.querySelectorAll("#musha-shugyo [data-action]")];
    const signedIn = root.dataset.signedIn === "true";
    let game = null;
    let busy = false;

    async function request(url, body) {
        const response = await fetch(url, {
            method: body ? "POST" : "GET",
            credentials: "same-origin",
            headers: body ? { "Content-Type": "application/json" } : {},
            body: body
                ? JSON.stringify({ ...body, csrf: root.dataset.csrf })
                : undefined,
        });
        const text = await response.text();
        let payload = text;
        try {
            payload = text ? JSON.parse(text) : null;
        } catch {
            payload = text;
        }
        if (!response.ok) {
            const message =
                typeof payload === "string"
                    ? payload
                    : payload?.message || `Serveren svarede med fejl ${response.status}.`;
            throw new Error(message);
        }
        return payload;
    }

    function setJourneyStatus(message, canStart = false) {
        if (journeyStatus) journeyStatus.textContent = message;
        if (startButton) startButton.hidden = !canStart;
    }

    function setBusy(value) {
        busy = value;
        for (const button of actionButtons) {
            const action = button.dataset.action;
            button.disabled =
                value ||
                !game ||
                (action === "attack" && !core.inCombatZone(game)) ||
                (action === "flee" && !core.canFlee(game)) ||
                (action === "train" && !core.canTrain(game));
        }
        if (startButton) startButton.disabled = value;
    }

    async function restoreJourney(journey) {
        game = core.createGame(journey.world_seed, journey.generator_version);
        for (const action of journey.actions) core.step(game, action);
        if (game.turn !== journey.actions.length) {
            throw new Error("Rejsens handlinger kunne ikke afspilles til den gemte tur.");
        }
        if (game.generatorVersion < core.GENERATOR_VERSION) {
            const result = await request("/games/musha-shugyo/api/action", {
                action: "upgrade_rules",
                expected_turn: game.turn,
            });
            core.step(game, "upgrade_rules");
            if (result.turn !== game.turn) {
                throw new Error("Rejsens kampopdatering blev ikke gemt korrekt.");
            }
        }
        gamePanel.hidden = false;
        render();
        setBusy(false);
        setJourneyStatus(`Din rejse fra ${journey.world_month} er klar.`);
    }

    async function loadJourney() {
        setBusy(true);
        setJourneyStatus("Indlæser din rejse …");
        const journey = await request("/games/musha-shugyo/api/state");
        if (journey) {
            await restoreJourney(journey);
        } else {
            setBusy(false);
            setJourneyStatus("Du har endnu ikke en rejse. Månedens verden venter.", true);
        }
    }

    function render() {
        if (!game) return;
        const room = core.currentRoom(game);
        const visibleCreatures = core.visibleCreatures(game);
        board.replaceChildren();
        board.style.setProperty("--musha-columns", String(core.WIDTH));
        for (let y = 0; y < core.HEIGHT; y += 1) {
            for (let x = 0; x < core.WIDTH; x += 1) {
                const tile = document.createElement("div");
                const terrain = core.tileAt(game, x, y);
                const resource = room.resources.find(
                    (entry) => entry.available && entry.x === x && entry.y === y,
                );
                const treasure =
                    room.treasure?.available && room.treasure.x === x && room.treasure.y === y;
                tile.className = "musha-cell";
                tile.setAttribute("role", "gridcell");

                if (terrain === "#") tile.classList.add("is-tree");
                if (terrain === "b") tile.classList.add("is-bush");
                if (core.isDoor(room, x, y)) {
                    tile.classList.add("is-door");
                    tile.textContent = "門";
                    tile.setAttribute("aria-label", "Åben dør til et andet rum");
                }
                if (room.hokora?.x === x && room.hokora?.y === y) {
                    tile.classList.add("is-hokora");
                    tile.textContent = "祠";
                    tile.setAttribute("aria-label", "Hokora, lille helligdom");
                }
                if (resource) {
                    tile.classList.add("is-resource");
                    tile.textContent = resource.type === "bær" ? "実" : "茸";
                    tile.setAttribute("aria-label", resource.type);
                }
                if (treasure) {
                    tile.classList.add("is-treasure");
                    tile.textContent = "宝";
                    tile.setAttribute("aria-label", "Fund");
                }
                if (game.player.x === x && game.player.y === y) {
                    tile.classList.add("is-player");
                    tile.textContent = "人";
                    tile.setAttribute("aria-label", "Dig");
                } else if (visibleCreatures.some((creature) => creature.x === x && creature.y === y)) {
                    tile.classList.add("is-creature");
                    tile.textContent = "獣";
                    tile.setAttribute("aria-label", "Væsen");
                } else if (terrain === "#") {
                    tile.setAttribute("aria-label", "Træ");
                } else if (terrain === "b") {
                    tile.setAttribute("aria-label", "Buskads");
                }
                board.append(tile);
            }
        }

        turn.textContent = String(game.turn);
        roomLabel.textContent = `Rum ${room.x},${room.y}`;
        status.textContent = game.message;
        const standingResource = room.resources.find(
            (entry) =>
                entry.available &&
                entry.x === game.player.x &&
                entry.y === game.player.y,
        );
        const standingTreasure =
            room.treasure?.available &&
            room.treasure.x === game.player.x &&
            room.treasure.y === game.player.y;
        const standingAtHokora =
            room.hokora?.x === game.player.x && room.hokora?.y === game.player.y;
        const positionItems = [];
        if (standingResource) positionItems.push(standingResource.type);
        if (standingTreasure) positionItems.push("et fund");
        if (standingAtHokora) positionItems.push("en hokora");
        positionNote.textContent = positionItems.length
            ? `Du står ved ${positionItems.join(" og ")}.`
            : "";
        for (const [action, isRelevant] of [
            ["collect", Boolean(standingResource || standingTreasure)],
            ["activate_checkpoint", Boolean(standingAtHokora)],
            ["train", core.canTrain(game)],
        ]) {
            const button = actionButtons.find((entry) => entry.dataset.action === action);
            button?.classList.toggle("btn-success", isRelevant);
            button?.classList.toggle("btn-outline-secondary", !isRelevant);
        }
        zone.textContent = core.zone(game);
        knowledge.textContent = core.playerKnowledge(game);
        mood.textContent = core.mood(game);
        inventory.textContent = core.inventorySummary(game);
        combatStats.textContent = core.combatSummary(game);
        const trainingButton = actionButtons.find((button) => button.dataset.action === "train");
        const trainingCost = core.trainingCost(game.player.combatSkill);
        trainingButton.textContent = trainingCost
            ? `Træn kamp · ${trainingCost.experience} XP + ${trainingCost.treasure} ${trainingCost.treasure === 1 ? "skat" : "skatte"}`
            : "Kampfærdighed maksimeret";
        combatResult.textContent = game.lastCombat
            ? `Sidste slag: dig [${game.lastCombat.playerRolls.join(", ")}] = ${game.lastCombat.playerAttack}; væsenet [${game.lastCombat.creatureRolls.join(", ")}] = ${game.lastCombat.creatureAttack}. ${core.combatResult(game)}`
            : core.combatResult(game);
        checkpoint.textContent = game.checkpoint
            ? `Aktiv ved rum ${game.rooms[game.checkpoint.roomId].x},${game.rooms[game.checkpoint.roomId].y}`
            : "Ingen aktiv hokora.";
        journal.replaceChildren();
        for (const entry of game.journal.slice(-30)) {
            const item = document.createElement("li");
            item.textContent = entry;
            journal.append(item);
        }
            setBusy(busy);
    }

    async function startJourney() {
        setBusy(true);
        setJourneyStatus("Forbereder månedens verden …");
        try {
            const journey = await request("/games/musha-shugyo/api/journey", {});
            await restoreJourney(journey);
        } catch (error) {
            setBusy(false);
            setJourneyStatus(error.message, true);
        }
    }

    async function act(action) {
        if (!game || busy) return;
        setBusy(true);
        try {
            const result = await request("/games/musha-shugyo/api/action", {
                action,
                expected_turn: game.turn,
            });
            core.step(game, action);
            if (result.turn !== game.turn) {
                throw new Error("Serveren og spillet er kommet ud af takt. Genindlæs siden.");
            }
            render();
            setBusy(false);
        } catch (error) {
            try {
                await loadJourney();
                setJourneyStatus(`${error.message} Den seneste gemte tilstand er indlæst.`);
            } catch (reloadError) {
                game = null;
                gamePanel.hidden = true;
                setBusy(false);
                setJourneyStatus(`${error.message} Genindlæsning fejlede: ${reloadError.message}`);
            }
        }
    }

    for (const button of actionButtons) {
        button.disabled = true;
        button.addEventListener("click", () => act(button.dataset.action));
    }
    startButton?.addEventListener("click", startJourney);

    document.addEventListener("keydown", (event) => {
        if (!game || busy || event.altKey || event.ctrlKey || event.metaKey || event.repeat) return;
        if (["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName)) return;
        const keys = {
            ArrowUp: "north",
            ArrowRight: "east",
            ArrowDown: "south",
            ArrowLeft: "west",
        };
        const actionKeys = {
            a: "attack",
            o: "observe",
            s: "collect",
            c: "collect",
            h: "activate_checkpoint",
            v: "wait",
            w: "wait",
        };
        const action = keys[event.key] || actionKeys[event.key.toLowerCase()];
        if (action) {
            event.preventDefault();
            act(action);
        } else if (event.key === " ") {
            event.preventDefault();
            act("wait");
        }
    });

    if (signedIn) {
        loadJourney().catch((error) => {
            setBusy(false);
            setJourneyStatus(error.message);
        });
    }
})();

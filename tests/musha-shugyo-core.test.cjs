const { test } = require("node:test");
const assert = require("node:assert/strict");
const {
    WIDTH,
    HEIGHT,
    createGame,
    createRoom,
    combatResult,
    combatSummary,
    currentRoom,
    edgeDoors,
    hasLineOfSight,
    inCombatZone,
    isDoor,
    canFlee,
    canTrain,
    perception,
    step,
    trainingCost,
} = require("../static/musha-shugyo-core.js");

function doorCell(direction, position) {
    if (direction === "north") return { x: position, y: 0 };
    if (direction === "east") return { x: WIDTH - 1, y: position };
    if (direction === "south") return { x: position, y: HEIGHT - 1 };
    return { x: 0, y: position };
}

function roomKeys(doors) {
    return Object.entries(doors).flatMap(([direction, positions]) =>
        positions.map((position) => ({ direction, position })),
    );
}

test("the starting room is deterministic and contains distinct actors", () => {
    const first = createGame();
    const second = createGame();
    const room = currentRoom(first);
    assert.equal(room.terrain.length, HEIGHT);
    assert.ok(room.terrain.every((row) => row.length === WIDTH));
    assert.deepEqual(room, currentRoom(second));
    assert.notDeepEqual(first.player, room.creature);
    assert.equal(first.player.combatSkill, 2);
    assert.equal(first.player.health, 10);
    assert.equal(first.player.maxHealth, 10);
    assert.equal(room.creature.combatSkill, 2);
    assert.equal(room.creature.health, 4);
    assert.equal(room.creature.maxHealth, 4);
    assert.match(combatSummary(first), /Dig: kamp 2, liv 10\/10/);
    assert.equal(createGame("previous-world", 2).player.combatSkill, 3);
});

test("creature combat skills and life are deterministic within configured ranges", () => {
    const first = createRoom(4, -2, "combat-world");
    const replay = createRoom(4, -2, "combat-world");
    assert.deepEqual(first.creature, replay.creature);
    if (first.creature) {
        assert.ok(first.creature.combatSkill >= 1 && first.creature.combatSkill <= 3);
        assert.ok(first.creature.maxHealth >= 3 && first.creature.maxHealth <= 6);
        assert.equal(first.creature.health, first.creature.maxHealth);
    }

    const legacy = createGame("old-world", 1);
    assert.equal(legacy.generatorVersion, 1);
    assert.equal(currentRoom(legacy).creature.combatSkill, 1);
    assert.equal(currentRoom(legacy).creature.maxHealth, 3);
});

test("every room has at least two doors and every shared edge has matching openings", () => {
    for (let y = -4; y <= 4; y += 1) {
        for (let x = -4; x <= 4; x += 1) {
            const room = createRoom(x, y);
            assert.ok(roomKeys(room.doors).length >= 2, `room ${room.id} needs two doors`);
            for (const direction of ["east", "south"]) {
                const nextX = direction === "east" ? x + 1 : x;
                const nextY = direction === "south" ? y + 1 : y;
                const opposite = direction === "east" ? "west" : "north";
                const neighbor = createRoom(nextX, nextY);
                assert.deepEqual(
                    room.doors[direction],
                    neighbor.doors[opposite],
                    `room ${room.id} and ${neighbor.id} must share matching ${direction} doors`,
                );
            }
        }
    }
});

test("a double doorway is mirrored by two openings in the adjacent room", () => {
    let found = false;
    for (let x = -10; x <= 10 && !found; x += 1) {
        const eastDoors = edgeDoors(x, 0, "east");
        if (eastDoors.length !== 2) continue;
        const room = createRoom(x, 0);
        const neighbor = createRoom(x + 1, 0);
        assert.deepEqual(room.doors.east, neighbor.doors.west);
        assert.equal(neighbor.doors.west.length, 2);
        found = true;
    }
    assert.equal(found, true);
});

test("door openings break the outer wall while the other wall cells stay closed", () => {
    const room = createRoom(3, -2);
    for (const { direction, position } of roomKeys(room.doors)) {
        const cell = doorCell(direction, position);
        assert.equal(room.terrain[cell.y][cell.x], ".");
        assert.equal(isDoor(room, cell.x, cell.y), true);
    }
    assert.equal(room.terrain[0][0], "#");
    assert.equal(isDoor(room, 0, 0), false);
});

test("the interior path connects the center to every door", () => {
    for (let y = -3; y <= 3; y += 1) {
        for (let x = -3; x <= 3; x += 1) {
            const room = createRoom(x, y);
            const reachable = new Set(["6,4"]);
            const pending = [{ x: 6, y: 4 }];
            for (let index = 0; index < pending.length; index += 1) {
                const cell = pending[index];
                for (const neighbor of [
                    { x: cell.x, y: cell.y - 1 },
                    { x: cell.x + 1, y: cell.y },
                    { x: cell.x, y: cell.y + 1 },
                    { x: cell.x - 1, y: cell.y },
                ]) {
                    if (
                        neighbor.x >= 0 &&
                        neighbor.x < WIDTH &&
                        neighbor.y >= 0 &&
                        neighbor.y < HEIGHT &&
                        room.terrain[neighbor.y][neighbor.x] !== "#" &&
                        !reachable.has(`${neighbor.x},${neighbor.y}`)
                    ) {
                        reachable.add(`${neighbor.x},${neighbor.y}`);
                        pending.push(neighbor);
                    }
                }
            }
            for (const { direction, position } of roomKeys(room.doors)) {
                const cell = doorCell(direction, position);
                assert.ok(reachable.has(`${cell.x},${cell.y}`), `door in room ${room.id} is unreachable`);
            }
        }
    }
});

test("trees and bushes obstruct sight while bushes remain traversable", () => {
    const game = createGame();
    const room = currentRoom(game);
    assert.equal(hasLineOfSight(game, { x: 1, y: 4 }, room.creature), false);
    assert.equal(hasLineOfSight(game, { x: 6, y: 3 }, room.creature), false);
    assert.notEqual(room.terrain[3][7], "#");
});

test("a creature hidden in a bush cannot be seen from a distance", () => {
    const game = createGame();
    const room = currentRoom(game);
    Object.assign(game.player, { x: 8, y: 1 });
    room.creature = { x: 9, y: 3, aware: false, lastKnownPlayer: null };
    assert.equal(hasLineOfSight(game, game.player, room.creature), true);
    assert.equal(perception(game).playerSeesCreature, false);
});

test("an observation from cover gives knowledge without revealing the player", () => {
    const game = createGame();
    const room = currentRoom(game);
    Object.assign(game.player, { x: 7, y: 3 });
    room.creature = { x: 10, y: 4, aware: false, lastKnownPlayer: null };
    assert.equal(perception(game).playerSeesCreature, true);
    step(game, "observe");
    assert.equal(room.playerStudiedCreature, true);
    assert.equal(room.creature.aware, false);
    assert.equal(room.playerHasSeenCreature, true);
});

test("a blocked move still advances the shared turn", () => {
    const game = createGame();
    Object.assign(game.player, { x: 3, y: 4 });
    step(game, "east");
    assert.equal(game.player.x, 3);
    assert.equal(game.player.y, 4);
    assert.equal(game.turn, 1);
    assert.match(game.message, /spærrer vejen/);
});

test("a combat turn displays seeded d6 rolls and the higher attack deals one damage", () => {
    const game = createGame("combat-resolution");
    const replay = createGame("combat-resolution");
    for (const candidate of [game, replay]) {
        const room = currentRoom(candidate);
        candidate.player = { ...candidate.player, x: 7, y: 4 };
        Object.assign(room.creature, {
            x: 8,
            y: 4,
            combatSkill: 2,
            health: 4,
            maxHealth: 4,
            aware: true,
            studiedPlayer: true,
        });
        room.playerStudiedCreature = true;
    }
    assert.equal(inCombatZone(game), true);

    const playerHealth = game.player.health;
    const creatureHealth = currentRoom(game).creature.health;
    step(game, "attack");
    step(replay, "attack");

    assert.deepEqual(game.lastCombat, replay.lastCombat);
    assert.equal(game.lastCombat.playerRolls.length, 2);
    assert.equal(game.lastCombat.creatureRolls.length, 2);
    assert.ok([...game.lastCombat.playerRolls, ...game.lastCombat.creatureRolls].every((roll) => roll >= 1 && roll <= 6));
    assert.equal(game.lastCombat.playerAttack, Math.max(...game.lastCombat.playerRolls));
    assert.equal(game.lastCombat.creatureAttack, Math.max(...game.lastCombat.creatureRolls));
    assert.equal(currentRoom(game).creature.health, creatureHealth - game.lastCombat.playerDamage);
    assert.equal(game.player.health, playerHealth - game.lastCombat.creatureDamage);
    assert.equal(game.turn, 1);
    assert.equal(game.experience, 1);
    assert.equal(currentRoom(game).combatExperience, 1);
    assert.match(game.message, /Kamp: Du slår/);
    assert.match(combatResult(game), /sammenstød|Uafgjort/);
});

test("the player regains one life per non-combat turn, but not during combat", () => {
    const game = createGame("combat-healing");
    game.player.health = 5;
    step(game, "wait");
    assert.equal(game.player.health, 6);

    game.player = { ...game.player, x: 7, y: 4 };
    Object.assign(currentRoom(game).creature, { x: 8, y: 4 });
    game.player.health = 5;
    step(game, "attack");
    assert.equal(game.player.health, 5 - game.lastCombat.creatureDamage);
});

test("the player can flee after matching combat rolls and loses this room's combat experience", () => {
    let game;
    for (let seedIndex = 0; seedIndex < 100 && !game; seedIndex += 1) {
        const seed = `player-flee-${seedIndex}`;
        for (let turn = 0; turn < 30; turn += 1) {
            const candidate = createGame(seed);
            const room = currentRoom(candidate);
            candidate.turn = turn;
            Object.assign(candidate.player, { x: 7, y: 4 });
            Object.assign(room.creature, {
                x: 8,
                y: 4,
                combatSkill: 1,
                health: 99,
                maxHealth: 99,
                aware: true,
                studiedPlayer: true,
            });
            room.playerStudiedCreature = true;
            const position = room.doors.west[0];
            candidate.rooms["-1,0"] = createRoom(-1, 0, seed);
            candidate.route.push({ roomId: "-1,0", direction: "east", position });
            step(candidate, "attack");
            if (canFlee(candidate)) {
                game = candidate;
                break;
            }
        }
    }

    assert.ok(game, "a deterministic matching-roll combat turn should be found");
    const combatRoom = currentRoom(game);
    const experienceBeforeFlight = game.experience;
    assert.equal(combatRoom.combatExperience, 1);
    step(game, "flee");
    assert.equal(game.roomId, "-1,0");
    assert.equal(game.experience, experienceBeforeFlight - 1);
    assert.equal(combatRoom.combatExperience, 0);
    assert.equal(game.route.length, 0);
    assert.match(game.message, /flygter gennem døren/);
});

test("creatures flee through their nearest door on matching rolls when eligible", () => {
    let fled;
    for (let seedIndex = 0; seedIndex < 100 && !fled; seedIndex += 1) {
        const seed = `creature-flee-${seedIndex}`;
        for (let turn = 0; turn < 30; turn += 1) {
            const game = createGame(seed);
            const room = currentRoom(game);
            game.turn = turn;
            Object.assign(game.player, { x: 7, y: 4 });
            Object.assign(room.creature, {
                x: 8,
                y: 4,
                combatSkill: 2,
                health: 99,
                maxHealth: 99,
                aware: true,
                studiedPlayer: true,
            });
            room.playerStudiedCreature = true;
            step(game, "attack");
            if (game.lastCombat.creatureFledTo) {
                fled = { game, room };
                break;
            }
        }
    }

    assert.ok(fled, "a deterministic matching-roll creature escape should be found");
    assert.equal(fled.room.creature, null);
    const destination = fled.game.rooms[fled.game.lastCombat.creatureFledTo];
    assert.ok(destination.creature || destination.visitors.length > 0);
    assert.match(fled.game.message, /flygter gennem døren mod/);
});

test("a stronger skill-four creature does not flee on matching rolls", () => {
    let matched;
    for (let seedIndex = 0; seedIndex < 100 && !matched; seedIndex += 1) {
        const seed = `strong-creature-${seedIndex}`;
        for (let turn = 0; turn < 30; turn += 1) {
            const game = createGame(seed);
            const room = currentRoom(game);
            game.turn = turn;
            Object.assign(game.player, { x: 7, y: 4, combatSkill: 2 });
            Object.assign(room.creature, {
                x: 8,
                y: 4,
                combatSkill: 4,
                health: 99,
                maxHealth: 99,
                aware: true,
                studiedPlayer: true,
            });
            room.playerStudiedCreature = true;
            step(game, "attack");
            if (game.lastCombat.creatureRolls.every((roll) => roll === game.lastCombat.creatureRolls[0])) {
                matched = { game, room };
                break;
            }
        }
    }

    assert.ok(matched, "a deterministic matching-roll strong creature turn should be found");
    assert.equal(matched.game.lastCombat.creatureCanFlee, false);
    assert.equal(matched.room.creature.health > 0, true);
    assert.equal(matched.game.lastCombat.creatureFledTo, undefined);
});

test("version-two journeys retain their original skill and combat progression", () => {
    const game = createGame("previous-combat", 2);
    const room = currentRoom(game);
    game.experience = 100;
    game.inventory.carried.push("fund", "fund", "fund", "fund");
    Object.assign(game.player, room.hokora);
    assert.equal(canTrain(game), false);
    game.experience = 0;
    Object.assign(game.player, { x: 7, y: 4 });
    Object.assign(room.creature, {
        x: 8,
        y: 4,
        combatSkill: 1,
        health: 99,
        maxHealth: 99,
        aware: true,
        studiedPlayer: true,
    });
    room.playerStudiedCreature = true;
    step(game, "attack");
    assert.equal(game.player.combatSkill, 3);
    assert.equal(game.experience, 0);
    assert.equal(room.combatExperience, 0);
    assert.equal(canFlee(game), false);
});

test("legacy journeys switch combat rules at a replayable upgrade action", () => {
    const seed = "legacy-rules-upgrade";
    const untouched = createGame(seed, 2);
    step(untouched, "upgrade_rules");
    assert.equal(untouched.generatorVersion, 3);
    assert.equal(untouched.player.combatSkill, 2);

    const existing = createGame(seed, 2);
    step(existing, "wait");
    step(existing, "upgrade_rules");
    assert.equal(existing.generatorVersion, 3);
    assert.equal(existing.player.combatSkill, 3);
    step(existing, "wait");

    const replay = createGame(seed, 2);
    step(replay, "wait");
    step(replay, "upgrade_rules");
    step(replay, "wait");
    assert.deepEqual(existing, replay);
});

test("hokora training spends all experience and treasures up to skill six", () => {
    const game = createGame("hokora-training");
    const room = currentRoom(game);
    Object.assign(game.player, room.hokora);
    room.playerStudiedCreature = true;
    room.creature.studiedPlayer = true;
    assert.deepEqual(trainingCost(2), { experience: 8, treasure: 1 });
    assert.deepEqual(trainingCost(3), { experience: 10, treasure: 2 });
    assert.deepEqual(trainingCost(4), { experience: 12, treasure: 3 });
    assert.deepEqual(trainingCost(5), { experience: 14, treasure: 4 });
    assert.equal(trainingCost(6), null);

    for (let skill = 2; skill < 6; skill += 1) {
        const cost = trainingCost(skill);
        game.experience = cost.experience;
        game.inventory.carried.push(...Array(cost.treasure).fill("fund"));
        assert.equal(canTrain(game), true);
        step(game, "train");
        assert.equal(game.player.combatSkill, skill + 1);
        assert.equal(game.experience, 0);
        assert.equal(room.combatExperience, 0);
        assert.equal(game.inventory.carried.filter((item) => item === "fund").length, 0);
        assert.equal(game.checkpoint.playerCombatSkill, skill + 1);
        assert.equal(
            game.message,
            "Kamien tager imod din skat og lytter til beretningen om din vandring. Din kampfærdighed vokser.",
        );
    }
    assert.equal(canTrain(game), false);
    assert.equal(game.player.combatSkill, 6);
});

test("hokora praises experience shared beyond the training requirement", () => {
    const game = createGame("hokora-surplus-training");
    const room = currentRoom(game);
    Object.assign(game.player, room.hokora);
    game.experience = 11;
    room.combatExperience = 9;
    game.inventory.carried.push("fund");

    step(game, "train");

    assert.equal(game.experience, 0);
    assert.equal(room.combatExperience, 0);
    assert.equal(game.player.combatSkill, 3);
    assert.equal(
        game.message,
        "Kamien tager imod din skat og lytter til beretningen om din vandring. Din kampfærdighed vokser. Den synes særligt tilfreds med alt det, du har delt ud over det nødvendige.",
    );
});

test("the first room has no escape route even after matching player rolls", () => {
    let game;
    for (let seedIndex = 0; seedIndex < 100 && !game; seedIndex += 1) {
        const seed = `no-first-room-flee-${seedIndex}`;
        for (let turn = 0; turn < 30; turn += 1) {
            const candidate = createGame(seed);
            const room = currentRoom(candidate);
            candidate.turn = turn;
            Object.assign(candidate.player, { x: 7, y: 4 });
            Object.assign(room.creature, {
                x: 8,
                y: 4,
                combatSkill: 1,
                health: 99,
                maxHealth: 99,
                aware: true,
                studiedPlayer: true,
            });
            room.playerStudiedCreature = true;
            step(candidate, "attack");
            if (candidate.lastCombat.playerCanFlee) {
                game = candidate;
                break;
            }
        }
    }

    assert.ok(game);
    assert.equal(canFlee(game), false);
    assert.match(game.message, /ingen dør tilbage/);
});

test("attacking outside the combat zone is rejected and does not roll dice", () => {
    const game = createGame();
    const health = game.player.health;
    assert.equal(inCombatZone(game), false);
    step(game, "attack");
    assert.equal(game.lastCombat, null);
    assert.equal(game.player.health, health);
    assert.match(game.message, /ikke i zone of combat/);
});

test("a defeated creature flees and player defeat returns to the checkpoint", () => {
    const seed = "combat-defeat";
    let victory;
    let defeat;
    for (let turn = 0; turn < 100 && (!victory || !defeat); turn += 1) {
        const game = createGame(seed);
        const room = currentRoom(game);
        game.turn = turn;
        game.player = { ...game.player, x: 7, y: 4 };
        Object.assign(room.creature, {
            x: 8,
            y: 4,
            combatSkill: 1,
            health: 1,
            maxHealth: 1,
        });
        step(game, "attack");
        if (game.lastCombat.playerDamage && !victory) victory = game;

        const defeated = createGame(seed);
        const defeatedRoom = currentRoom(defeated);
        defeated.turn = turn;
        defeated.player = { ...defeated.player, x: 7, y: 4, health: 1 };
        Object.assign(defeatedRoom.creature, {
            x: 8,
            y: 4,
            combatSkill: 1,
            health: 99,
            maxHealth: 99,
        });
        defeated.checkpoint = {
            roomId: "0,0",
            player: { x: 2, y: 4, health: 7 },
            playerCombatSkill: 3,
            playerMaxHealth: 10,
            experience: 2,
            route: [],
            inventory: { secured: ["fund"], carried: [] },
        };
        defeated.inventory.carried.push("bær");
        step(defeated, "attack");
        if (defeated.lastCombat.creatureDamage && !defeat) defeat = defeated;
    }

    assert.ok(victory);
    assert.equal(currentRoom(victory).creature, null);
    assert.match(victory.message, /flygter fra rummet/);
    assert.ok(defeat);
    assert.equal(defeat.player.health, 7);
    assert.equal(defeat.player.x, 2);
    assert.deepEqual(defeat.inventory, { secured: ["fund"], carried: [] });
    assert.match(defeat.message, /forvist til din hokora/);
});

test("a noticed creature remembers the player and does not share their tile", () => {
    const game = createGame();
    const room = currentRoom(game);
    Object.assign(game.player, { x: 7, y: 4 });
    step(game, "wait");
    assert.equal(room.creature.aware, true);
    assert.equal(room.creature.lastKnownPlayer.x, 7);
    assert.equal(room.creature.lastKnownPlayer.y, 4);
    step(game, "wait");
    assert.notDeepEqual(game.player, { x: room.creature.x, y: room.creature.y });
});

test("entering a doorway creates the adjacent room at its matching opening", () => {
    const game = createGame();
    const room = currentRoom(game);
    const direction = "east";
    const position = room.doors[direction][0];
    const outside = WIDTH - 1;
    const inside = WIDTH - 2;
    Object.assign(game.player, { x: inside, y: position });
    step(game, direction);
    assert.equal(game.player.x, outside);
    assert.equal(game.player.y, position);

    step(game, direction);
    const next = currentRoom(game);
    assert.equal(game.roomId, direction === "east" ? "1,0" : "-1,0");
    assert.equal(game.player.y, position);
    assert.equal(game.player.x, direction === "east" ? 0 : WIDTH - 1);
    assert.deepEqual(
        next.doors[direction === "east" ? "west" : "east"],
        room.doors[direction],
    );
    assert.equal(next.visits, 1);
});

test("a room's creature stays still while away, then wanders within the elapsed turns on return", () => {
    const game = createGame();
    const firstRoom = currentRoom(game);
    const position = firstRoom.doors.east[0];
    Object.assign(game.player, { x: WIDTH - 2, y: position });
    step(game, "east");
    step(game, "east");
    const creatureAtDeparture = { ...firstRoom.creature };
    const departureTurn = firstRoom.lastLeftTurn;
    assert.equal(game.roomId, "1,0");

    Object.assign(game.player, { x: 0, y: position });
    step(game, "wait");
    assert.equal(game.turn, departureTurn + 1);
    assert.deepEqual(firstRoom.creature, creatureAtDeparture);

    Object.assign(game.player, { x: 0, y: position });
    step(game, "west");
    assert.equal(game.roomId, "0,0");
    assert.equal("pendingCreaturePlacement" in game, false);
    assert.equal(firstRoom.lastLeftTurn, null);
    assert.ok(game.turn - departureTurn > 0);
    assert.match(game.message, /Væsenet har flyttet sig, mens du var væk/);

    const displacement =
        Math.abs(firstRoom.creature.x - creatureAtDeparture.x) +
        Math.abs(firstRoom.creature.y - creatureAtDeparture.y);
    assert.ok(displacement <= game.turn - departureTurn);
    assert.notDeepEqual(
        { x: firstRoom.creature.x, y: firstRoom.creature.y },
        { x: game.player.x, y: game.player.y },
    );
});

test("monthly seeds produce repeatable but distinct worlds and reject unknown versions", () => {
    const first = createGame("musha-shugyo:2026-10");
    const replay = createGame("musha-shugyo:2026-10");
    const nextMonth = createGame("musha-shugyo:2026-11");
    assert.deepEqual(first, replay);
    assert.notDeepEqual(currentRoom(first).terrain, currentRoom(nextMonth).terrain);
    assert.throws(() => createGame("future-world", 4), /Unsupported/);

    const history = ["wait", "observe", "north", "collect", "east", "wait"];
    for (const action of history) step(first, action);
    for (const action of history) step(replay, action);
    assert.deepEqual(first, replay);
});

test("resources are collected, carried, and regrow after game turns", () => {
    const game = createGame();
    const room = currentRoom(game);
    const resource = room.resources[0];
    Object.assign(game.player, { x: resource.x, y: resource.y });

    step(game, "collect");
    assert.equal(resource.available, false);
    assert.equal(game.inventory.carried.length, 1);
    assert.equal(game.inventory.carried[0], resource.type);

    for (let index = 0; index < 19; index += 1) step(game, "wait");
    assert.equal(resource.available, false);
    step(game, "wait");
    assert.equal(resource.available, true);
    assert.equal(resource.regrowAt, null);
});

test("seeded treasures persist after they are collected", () => {
    const seed = "musha-shugyo:treasure-test";
    let treasureRoom;
    for (let y = -20; y <= 20 && !treasureRoom; y += 1) {
        for (let x = -20; x <= 20; x += 1) {
            const room = createRoom(x, y, seed);
            if (room.treasure) {
                treasureRoom = room;
                break;
            }
        }
    }
    assert.ok(treasureRoom);
    assert.deepEqual(treasureRoom, createRoom(treasureRoom.x, treasureRoom.y, seed));

    const game = createGame(seed);
    game.rooms[treasureRoom.id] = treasureRoom;
    game.roomId = treasureRoom.id;
    Object.assign(game.player, { x: treasureRoom.treasure.x, y: treasureRoom.treasure.y });
    step(game, "collect");
    assert.equal(treasureRoom.treasure.available, false);
    assert.ok(game.inventory.carried.includes("fund"));
});

test("a hokora anchors inventory and experience without rolling back world changes", () => {
    const game = createGame();
    const room = currentRoom(game);
    Object.assign(game.player, room.hokora);
    game.inventory.carried.push("bær");
    game.experience = 4;

    step(game, "activate_checkpoint");
    assert.deepEqual(game.inventory, { secured: ["bær"], carried: [] });
    assert.equal(game.checkpoint.experience, 4);

    const resource = room.resources[0];
    Object.assign(game.player, { x: resource.x, y: resource.y });
    step(game, "collect");
    assert.equal(resource.available, false);
    game.experience = 9;
    game.inventory.carried.push("svamp");
    step(game, "return_checkpoint");

    assert.equal(game.player.x, room.hokora.x);
    assert.equal(game.player.y, room.hokora.y);
    assert.equal(game.experience, 4);
    assert.deepEqual(game.inventory, { secured: ["bær"], carried: [] });
    assert.equal(resource.available, false);
    assert.match(game.message, /Usikrede fund går tabt/);

    const doorPosition = room.doors.east[0];
    Object.assign(game.player, { x: WIDTH - 2, y: doorPosition });
    step(game, "east");
    step(game, "east");
    assert.equal(game.route.length, 1);
    step(game, "return_checkpoint");
    assert.equal(game.roomId, room.id);
    assert.equal(game.route.length, 0);
    assert.equal(resource.available, false);
});

test("first shared observations grant experience once to both actors", () => {
    const game = createGame();
    const room = currentRoom(game);
    Object.assign(game.player, { x: 7, y: 4 });
    Object.assign(room.creature, { x: 9, y: 4 });

    step(game, "wait");
    assert.equal(game.experience, 1);
    assert.equal(room.creature.experience, 1);
    step(game, "observe");
    assert.equal(game.experience, 1);
    assert.equal(room.creature.experience, 1);
});

test("yield returns through the route and keeps a chronological journal", () => {
    const game = createGame();
    const firstRoom = currentRoom(game);
    const position = firstRoom.doors.east[0];
    Object.assign(game.player, { x: WIDTH - 2, y: position });
    step(game, "east");
    step(game, "east");
    assert.equal(game.roomId, "1,0");

    step(game, "yield");
    assert.equal(game.roomId, "0,0");
    assert.match(game.message, /giver plads/);
    for (let index = 0; index < 7; index += 1) step(game, "wait");
    assert.ok(game.journal.length > 5);
});

test("creatures heal over turns when food remains in their room", () => {
    const game = createGame();
    const creature = currentRoom(game).creature;
    creature.health = 1;
    creature.lastHealingTurn = 0;
    game.turn = 4;

    step(game, "wait");
    assert.equal(creature.health, 2);
});

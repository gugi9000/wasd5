(function (root, factory) {
    const api = factory();
    if (typeof module === "object" && module.exports) module.exports = api;
    else root.MushaShugyoCore = api;
})(typeof globalThis === "object" ? globalThis : this, function () {
    "use strict";

    const WIDTH = 13;
    const HEIGHT = 9;
    const DEFAULT_SEED = "musha-shugyo-prototype-v1";
    const GENERATOR_VERSION = 3;
    const DIRECTIONS = {
        north: { x: 0, y: -1, opposite: "south", label: "nord" },
        east: { x: 1, y: 0, opposite: "west", label: "øst" },
        south: { x: 0, y: 1, opposite: "north", label: "syd" },
        west: { x: -1, y: 0, opposite: "east", label: "vest" },
    };
    const DIRECTION_ORDER = ["north", "east", "south", "west"];
    const BASE_ROWS = [
        "#############",
        "#...........#",
        "#...#.#...#.#",
        "#...#.....#.#",
        "#...#.......#",
        "#...#.....#.#",
        "#...#.#...#.#",
        "#...........#",
        "#############",
    ];
    const BASE_BUSHES = [
        [2, 3],
        [3, 3],
        [2, 5],
        [3, 5],
        [7, 3],
        [9, 3],
        [7, 5],
        [9, 5],
    ];

    function hash(value) {
        let result = 2166136261;
        for (let index = 0; index < value.length; index += 1) {
            result ^= value.charCodeAt(index);
            result = Math.imul(result, 16777619);
        }

        return result >>> 0;
    }

    function roomId(x, y) {
        return `${x},${y}`;
    }

    function roomHash(x, y, purpose, seed = DEFAULT_SEED) {
        return hash(`${seed}:${purpose}:${x},${y}`);
    }

    function edgeDoors(x, y, direction, seed = DEFAULT_SEED) {
        const horizontal = direction === "east" || direction === "west";
        const edgeX = direction === "east" ? x + 1 : x;
        const edgeY = direction === "south" ? y + 1 : y;
        const value = hash(`${seed}:edge:${horizontal ? "horizontal" : "vertical"}:${edgeX},${edgeY}`);

        if (horizontal) return value % 4 === 0 ? [2, 6] : [4];
        switch (value % 4) {
            case 0:
                return [];
            case 1:
                return [4];
            case 2:
                return [8];
            default:
                return [4, 8];
        }
    }

    function doorCell(direction, position) {
        if (direction === "north") return { x: position, y: 0 };
        if (direction === "east") return { x: WIDTH - 1, y: position };
        if (direction === "south") return { x: position, y: HEIGHT - 1 };
        return { x: 0, y: position };
    }

    function makeTerrain(x, y, doors, seed) {
        const variant = roomHash(x, y, "terrain", seed);
        const flipX = x === 0 && y === 0 ? false : (variant & 1) !== 0;
        const flipY = x === 0 && y === 0 ? false : (variant & 2) !== 0;
        const terrain = Array.from({ length: HEIGHT }, () => Array(WIDTH).fill("."));

        for (let sourceY = 0; sourceY < HEIGHT; sourceY += 1) {
            for (let sourceX = 0; sourceX < WIDTH; sourceX += 1) {
                const targetX = flipX ? WIDTH - 1 - sourceX : sourceX;
                const targetY = flipY ? HEIGHT - 1 - sourceY : sourceY;
                terrain[targetY][targetX] = BASE_ROWS[sourceY][sourceX];
            }
        }

        for (const [sourceX, sourceY] of BASE_BUSHES) {
            const targetX = flipX ? WIDTH - 1 - sourceX : sourceX;
            const targetY = flipY ? HEIGHT - 1 - sourceY : sourceY;
            terrain[targetY][targetX] = "b";
        }

        for (const direction of DIRECTION_ORDER) {
            for (const position of doors[direction]) {
                const cell = doorCell(direction, position);
                terrain[cell.y][cell.x] = ".";
            }
        }
        return terrain;
    }

    function createRoom(x, y, seed = DEFAULT_SEED, generatorVersion = GENERATOR_VERSION) {
        if (![1, 2, GENERATOR_VERSION].includes(generatorVersion)) {
            throw new Error(`Unsupported Musha Shugyo generator version: ${generatorVersion}`);
        }
        const doors = {};
        for (const direction of DIRECTION_ORDER) {
            doors[direction] = edgeDoors(x, y, direction, seed);
        }

        const room = {
            id: roomId(x, y),
            x,
            y,
            doors,
            terrain: makeTerrain(x, y, doors, seed),
            creature: null,
            visitors: [],
            combatExperience: 0,
            hokora: null,
            resources: [],
            treasure: null,
            playerHasSeenCreature: false,
            playerStudiedCreature: false,
            visits: 0,
            lastLeftTurn: null,
        };

        if (x === 0 && y === 0) {
            room.creature = {
                x: 8,
                y: 4,
                combatSkill: generatorVersion === 1 ? 1 : 2,
                aware: false,
                lastKnownPlayer: null,
                experience: 0,
                studiedPlayer: false,
                successfulEvasions: 0,
                memoryOfPlayer: "unfamiliar",
                temperament: "curious",
                health: generatorVersion === 1 ? 3 : 4,
                maxHealth: generatorVersion === 1 ? 3 : 4,
                lastHealingTurn: 0,
            };
        } else if (roomHash(x, y, "creature", seed) % 4 !== 0) {
            const candidates = [];
            for (let cellY = 2; cellY < HEIGHT - 2; cellY += 1) {
                for (let cellX = 2; cellX < WIDTH - 2; cellX += 1) {
                    if (room.terrain[cellY][cellX] !== "#") candidates.push({ x: cellX, y: cellY });
                }
            }
            const position = candidates[roomHash(x, y, "creature-position", seed) % candidates.length];
            const temperament = ["curious", "shy", "watchful"][roomHash(x, y, "temperament", seed) % 3];
            const combatSkill =
                generatorVersion === 1
                    ? 1
                    : 1 + (roomHash(x, y, "creature-combat-skill", seed) % 3);
            const maxHealth =
                generatorVersion === 1
                    ? 3
                    : 3 + (roomHash(x, y, "creature-max-health", seed) % 4);
            room.creature = {
                ...position,
                combatSkill,
                aware: false,
                lastKnownPlayer: null,
                experience: 0,
                studiedPlayer: false,
                successfulEvasions: 0,
                memoryOfPlayer: "unfamiliar",
                temperament,
                health: maxHealth,
                maxHealth,
                lastHealingTurn: 0,
            };
        }

        const openCells = [];
        for (let cellY = 1; cellY < HEIGHT - 1; cellY += 1) {
            for (let cellX = 1; cellX < WIDTH - 1; cellX += 1) {
                if (
                    room.terrain[cellY][cellX] !== "#" &&
                    (!room.creature || distance({ x: cellX, y: cellY }, room.creature) > 1)
                ) {
                    openCells.push({ x: cellX, y: cellY });
                }
            }
        }
        if (x === 0 && y === 0) {
            room.hokora = { x: 2, y: 4 };
        } else if (roomHash(x, y, "hokora", seed) % 4 === 0) {
            room.hokora = openCells[roomHash(x, y, "hokora-position", seed) % openCells.length];
        }

        const resourceCount = 1 + (roomHash(x, y, "resources", seed) % 2);
        const usedCells = new Set(room.hokora ? [roomKey(room.hokora)] : []);
        for (let index = 0; index < openCells.length && room.resources.length < resourceCount; index += 1) {
            const cell = openCells[(roomHash(x, y, `resource-position-${index}`, seed) + index) % openCells.length];
            if (usedCells.has(roomKey(cell))) continue;
            usedCells.add(roomKey(cell));
            room.resources.push({
                id: `${room.id}:${room.resources.length}`,
                x: cell.x,
                y: cell.y,
                type: roomHash(x, y, `resource-type-${index}`, seed) % 2 === 0 ? "bær" : "svamp",
                available: true,
                regrowAt: null,
            });
        }
        if (roomHash(x, y, "treasure", seed) % 12 === 0) {
            const treasureCells = openCells.filter((cell) => !usedCells.has(roomKey(cell)));
            if (treasureCells.length > 0) {
                const position = treasureCells[roomHash(x, y, "treasure-position", seed) % treasureCells.length];
                room.treasure = {
                    ...position,
                    type: "fund",
                    available: true,
                };
            }
        }

        return room;
    }

    function roomKey(position) {
        return `${position.x},${position.y}`;
    }

    function createGame(seed = DEFAULT_SEED, generatorVersion = GENERATOR_VERSION) {
        if (![1, 2, GENERATOR_VERSION].includes(generatorVersion)) {
            throw new Error(`Unsupported Musha Shugyo generator version: ${generatorVersion}`);
        }
        const room = createRoom(0, 0, seed, generatorVersion);
        room.visits = 1;
        return {
            rooms: { [room.id]: room },
            roomId: room.id,
            player: {
                x: 1,
                y: 4,
                combatSkill: generatorVersion >= 3 ? 2 : 3,
                health: 10,
                maxHealth: 10,
            },
            turn: 0,
            seed,
            generatorVersion,
            experience: 0,
            lastCombat: null,
            inventory: { secured: [], carried: [] },
            checkpoint: null,
            route: [],
            status: "playing",
            journal: ["Du begynder vandringen ved skovens indgang."],
            message: "Du står ved skovens indgang. Find en åbning i ydervæggen for at gå videre.",
        };
    }

    function currentRoom(game) {
        return game.rooms[game.roomId];
    }

    function inBounds(x, y) {
        return x >= 0 && x < WIDTH && y >= 0 && y < HEIGHT;
    }

    function tileAt(game, x, y) {
        const room = currentRoom(game);
        return inBounds(x, y) ? room.terrain[y][x] : "#";
    }

    function isWalkable(room, x, y) {
        return inBounds(x, y) && room.terrain[y][x] !== "#";
    }

    function distance(a, b) {
        return Math.abs(a.x - b.x) + Math.abs(a.y - b.y);
    }

    function hasLineOfSight(game, from, to) {
        const room = currentRoom(game);
        let x = from.x;
        let y = from.y;
        const dx = Math.abs(to.x - from.x);
        const dy = Math.abs(to.y - from.y);
        const sx = from.x < to.x ? 1 : -1;
        const sy = from.y < to.y ? 1 : -1;
        let error = dx - dy;

        while (x !== to.x || y !== to.y) {
            const doubledError = error * 2;
            if (doubledError > -dy) {
                error -= dy;
                x += sx;
            }
            if (doubledError < dx) {
                error += dx;
                y += sy;
            }
            if (
                (x !== to.x || y !== to.y) &&
                (!inBounds(x, y) || ["#", "b"].includes(room.terrain[y][x]))
            ) {
                return false;
            }
        }
        return true;
    }

    function perception(game) {
        const creature = currentRoom(game).creature;
        if (!creature) {
            return { distance: Infinity, playerSeesCreature: false, creatureSeesPlayer: false };
        }
        const range = distance(game.player, creature);
        const clearView = hasLineOfSight(game, game.player, creature);
        return {
            distance: range,
            playerSeesCreature:
                clearView &&
                range <= 8 &&
                (tileAt(game, creature.x, creature.y) !== "b" || range <= 2),
            creatureSeesPlayer:
                clearView &&
                range <= 6 &&
                (tileAt(game, game.player.x, game.player.y) !== "b" || range <= 2),
        };
    }

    function neighboringCells(room, position, occupied) {
        return DIRECTION_ORDER.map((direction) => {
            const delta = DIRECTIONS[direction];
            return { x: position.x + delta.x, y: position.y + delta.y };
        }).filter(
            (cell) =>
                isWalkable(room, cell.x, cell.y) &&
                (!occupied || cell.x !== occupied.x || cell.y !== occupied.y),
        );
    }

    function stepToward(room, from, target, occupied, turn) {
        return neighboringCells(room, from, occupied)
            .filter((cell) => distance(cell, target) < distance(from, target))
            .sort(
                (a, b) =>
                    distance(a, target) - distance(b, target) ||
                    ((a.x + a.y + turn) % 4) - ((b.x + b.y + turn) % 4),
            )[0] || from;
    }

    function creatureIntent(game, before) {
        const room = currentRoom(game);
        const creature = room.creature;
        if (!creature) return null;

        if (creature.aware && before.creatureSeesPlayer) {
            const range = before.distance;
            if (creature.temperament === "shy" && range <= 4) {
                const options = neighboringCells(room, creature, game.player);
                options.sort(
                    (a, b) =>
                        distance(b, game.player) - distance(a, game.player) ||
                        ((a.x + a.y + game.turn) % 4) - ((b.x + b.y + game.turn) % 4),
                );
                return options[0] || creature;
            }
            if (range > 3 && creature.temperament !== "watchful") {
                return stepToward(room, creature, game.player, game.player, game.turn);
            }
            if (range <= 2) {
                const options = neighboringCells(room, creature, game.player);
                options.sort(
                    (a, b) =>
                        distance(b, game.player) - distance(a, game.player) ||
                        ((a.x + a.y + game.turn) % 4) - ((b.x + b.y + game.turn) % 4),
                );
                return options[0] || creature;
            }
            return creature;
        }
        if (creature.aware && creature.lastKnownPlayer) {
            return stepToward(room, creature, creature.lastKnownPlayer, game.player, game.turn);
        }

        const options = neighboringCells(room, creature, game.player);
        if (options.length === 0) return creature;
        return options[game.turn % options.length];
    }

    function addJournal(game, message) {
        if (!message) return;
        game.journal.push(message);
    }

    function doorAt(room, position) {
        for (const direction of DIRECTION_ORDER) {
            for (const doorPosition of room.doors[direction]) {
                const cell = doorCell(direction, doorPosition);
                if (cell.x === position.x && cell.y === position.y) {
                    return { direction, position: doorPosition };
                }
            }
        }
        return null;
    }

    function adjacentRoomPosition(room, direction) {
        const delta = DIRECTIONS[direction];
        return { x: room.x + delta.x, y: room.y + delta.y };
    }

    function entryPosition(direction, position) {
        const opposite = DIRECTIONS[direction].opposite;
        return doorCell(opposite, position);
    }

    function simulateCreatureWandering(room, turns, leftAtTurn, occupied, seed) {
        if (!room.creature) return false;
        let moved = false;

        for (let elapsed = 0; elapsed < turns; elapsed += 1) {
            const options = neighboringCells(room, room.creature, occupied);
            if (options.length === 0) break;
            const choice = hash(`${seed}:wander:${room.id}:${leftAtTurn}:${elapsed}`) % options.length;
            room.creature.x = options[choice].x;
            room.creature.y = options[choice].y;
            moved = true;
        }

        return moved;
    }

    function collectibleAt(room, position) {
        const resource = room.resources.find(
            (entry) => entry.x === position.x && entry.y === position.y && entry.available,
        );
        if (resource) return { type: "resource", item: resource };
        if (
            room.treasure?.available &&
            room.treasure.x === position.x &&
            room.treasure.y === position.y
        ) {
            return { type: "treasure", item: room.treasure };
        }
        return null;
    }

    function advanceRoom(game, room, allowCreatureHealing = true) {
        const regrown = [];
        for (const resource of room.resources) {
            if (!resource.available && resource.regrowAt !== null && game.turn >= resource.regrowAt) {
                resource.available = true;
                resource.regrowAt = null;
                regrown.push(resource.type);
            }
        }
        if (allowCreatureHealing) {
            const hasFood = room.resources.some((resource) => resource.available);
            for (const creature of creaturesIn(room)) {
                if (creature.health >= creature.maxHealth) continue;
                if (!hasFood) {
                    creature.lastHealingTurn = game.turn;
                    continue;
                }
                const healingTurns = Math.floor((game.turn - creature.lastHealingTurn) / 5);
                if (healingTurns > 0) {
                    creature.health = Math.min(creature.maxHealth, creature.health + healingTurns);
                    creature.lastHealingTurn += healingTurns * 5;
                }
            }
        }
        return regrown;
    }

    function activateCheckpoint(game, room) {
        game.inventory.secured.push(...game.inventory.carried);
        game.inventory.carried = [];
        game.checkpoint = {
            roomId: room.id,
            player: { ...game.player },
            playerCombatSkill: game.player.combatSkill,
            playerMaxHealth: game.player.maxHealth,
            experience: game.experience,
            route: game.route.map((entry) => ({ ...entry })),
            inventory: {
                secured: [...game.inventory.secured],
                carried: [],
            },
        };
    }

    function returnToCheckpoint(game) {
        if (!game.checkpoint) return false;
        game.roomId = game.checkpoint.roomId;
        const room = game.rooms[game.roomId];
        game.player = safePosition(room, game.checkpoint.player, creaturesIn(room));
        game.player.combatSkill = game.checkpoint.playerCombatSkill;
        game.player.maxHealth = game.checkpoint.playerMaxHealth;
        game.player.health = game.checkpoint.player.health;
        game.experience = game.checkpoint.experience;
        game.route = game.checkpoint.route.map((entry) => ({ ...entry }));
        game.inventory = {
            secured: [...game.checkpoint.inventory.secured],
            carried: [],
        };
        return true;
    }

    function safePosition(room, preferred, occupied) {
        const queue = [preferred];
        const visited = new Set([roomKey(preferred)]);
        for (let index = 0; index < queue.length; index += 1) {
            const position = queue[index];
            if (
                isWalkable(room, position.x, position.y) &&
                (!occupied ||
                    (Array.isArray(occupied)
                        ? !occupied.some((actor) => roomKey(position) === roomKey(actor))
                        : roomKey(position) !== roomKey(occupied)))
            ) {
                return { ...position };
            }
            for (const neighbor of neighboringCells(room, position, null)) {
                const key = roomKey(neighbor);
                if (!visited.has(key)) {
                    visited.add(key);
                    queue.push(neighbor);
                }
            }
        }
        throw new Error(`Room ${room.id} has no safe position for the player.`);
    }

    function combatRolls(game, actor, skill, turn) {
        if (game.generatorVersion < 3) {
            return Array.from({ length: skill }, (_, index) =>
                (hash(`${game.seed}:combat:${turn}:${actor}:${index}`) % 6) + 1,
            );
        }
        const base = hash(`${game.seed}:combat:${turn}:${actor}`);
        return Array.from({ length: skill }, (_, index) => {
            let value = (base + Math.imul(index + 1, 0x9e3779b9)) >>> 0;
            value = Math.imul(value ^ (value >>> 16), 0x7feb352d);
            value = Math.imul(value ^ (value >>> 15), 0x846ca68b);
            value = (value ^ (value >>> 16)) >>> 0;
            return (value % 6) + 1;
        });
    }

    function inCombatZone(game) {
        const room = currentRoom(game);
        if (!room.creature || room.creature.health <= 0) return false;
        const current = perception(game);
        return current.playerSeesCreature && current.distance <= 2;
    }

    function creaturesIn(room) {
        return [room.creature, ...(room.visitors || [])].filter(Boolean);
    }

    function creatureAt(room, position, except = null) {
        return creaturesIn(room).some(
            (creature) =>
                creature !== except && creature.x === position.x && creature.y === position.y,
        );
    }

    function visibleCreatures(game) {
        const room = currentRoom(game);
        return creaturesIn(room).filter((creature) => {
            if (!creature) return false;
            const range = distance(game.player, creature);
            return (
                range <= 8 &&
                hasLineOfSight(game, game.player, creature) &&
                (tileAt(game, creature.x, creature.y) !== "b" || range <= 2)
            );
        });
    }

    function removeCreature(room, creature) {
        if (room.creature === creature) {
            room.creature = (room.visitors || []).shift() || null;
            return;
        }
        const index = (room.visitors || []).indexOf(creature);
        if (index >= 0) room.visitors.splice(index, 1);
    }

    function allDiceMatch(rolls) {
        return rolls.length > 1 && rolls.every((roll) => roll === rolls[0]);
    }

    function canFlee(game) {
        return (
            game.generatorVersion >= 3 &&
            game.route.length > 0 &&
            game.lastCombat?.turn === game.turn &&
            game.lastCombat.playerCanFlee &&
            game.player.health > 0 &&
            currentRoom(game).creature
        );
    }

    function trainingCost(combatSkill) {
        if (combatSkill < 2 || combatSkill >= 6) return null;
        return { experience: 2 * combatSkill + 4, treasure: combatSkill - 1 };
    }

    function canTrain(game) {
        const room = currentRoom(game);
        const cost = trainingCost(game.player.combatSkill);
        if (
            game.generatorVersion < 3 ||
            !cost ||
            !room.hokora ||
            roomKey(room.hokora) !== roomKey(game.player)
        ) {
            return false;
        }
        const treasures =
            game.inventory.carried.filter((item) => item === "fund").length +
            game.inventory.secured.filter((item) => item === "fund").length;
        return game.experience >= cost.experience && treasures >= cost.treasure;
    }

    function trainCombatSkill(game, room) {
        if (game.generatorVersion < 3) {
            return "Denne rejse bruger en ældre kampversion uden træning.";
        }
        const cost = trainingCost(game.player.combatSkill);
        if (!room.hokora || roomKey(room.hokora) !== roomKey(game.player)) {
            return "Du skal stå ved en hokora for at træne din kampfærdighed.";
        }
        if (!cost) return "Din kampfærdighed kan ikke øges mere.";
        if (game.experience < cost.experience) {
            return `Du mangler erfaring. Træningen kræver ${cost.experience} erfaring.`;
        }
        const carriedFunds = game.inventory.carried.filter((item) => item === "fund").length;
        const securedFunds = game.inventory.secured.filter((item) => item === "fund").length;
        if (carriedFunds + securedFunds < cost.treasure) {
            return `Du mangler skatte. Træningen kræver ${cost.treasure}.`;
        }

        let remaining = cost.treasure;
        for (const items of [game.inventory.carried, game.inventory.secured]) {
            for (let index = items.length - 1; index >= 0 && remaining > 0; index -= 1) {
                if (items[index] === "fund") {
                    items.splice(index, 1);
                    remaining -= 1;
                }
            }
        }
        const hadSurplusExperience = game.experience > cost.experience;
        game.experience = 0;
        room.combatExperience = 0;
        game.player.combatSkill += 1;
        activateCheckpoint(game, room);
        const message =
            "Kamien tager imod din skat og lytter til beretningen om din vandring. Din kampfærdighed vokser.";
        return hadSurplusExperience
            ? `${message} Den synes særligt tilfreds med alt det, du har delt ud over det nødvendige.`
            : message;
    }

    function sendCreatureThroughNearestDoor(game, room, creature, messages) {
        const doors = DIRECTION_ORDER.flatMap((direction) =>
            room.doors[direction].map((position) => ({
                direction,
                position,
                cell: doorCell(direction, position),
            })),
        ).sort(
            (a, b) =>
                distance(a.cell, creature) - distance(b.cell, creature) ||
                DIRECTION_ORDER.indexOf(a.direction) - DIRECTION_ORDER.indexOf(b.direction) ||
                a.position - b.position,
        );
        const door = doors[0];
        if (!door) throw new Error(`Room ${room.id} has no door for a fleeing creature.`);

        const adjacent = adjacentRoomPosition(room, door.direction);
        const nextId = roomId(adjacent.x, adjacent.y);
        const nextRoom =
            game.rooms[nextId] ||
            createRoom(adjacent.x, adjacent.y, game.seed, game.generatorVersion);
        const occupied = creaturesIn(nextRoom);
        const position = safePosition(
            nextRoom,
            entryPosition(door.direction, door.position),
            occupied,
        );
        creature.x = position.x;
        creature.y = position.y;
        creature.aware = false;
        creature.lastKnownPlayer = null;
        if (nextRoom.creature) {
            nextRoom.visitors ??= [];
            nextRoom.visitors.unshift(nextRoom.creature);
        }
        nextRoom.creature = creature;
        game.rooms[nextId] = nextRoom;
        if (!room.creature) {
            room.creature = (room.visitors || []).shift() || null;
        }
        messages.push(`Væsenet flygter gennem døren mod ${DIRECTIONS[door.direction].label} ind i rum ${nextId}.`);
        return nextId;
    }

    function resolveCombat(game, room) {
        const creature = room.creature;
        const turn = game.turn;
        const playerRolls = combatRolls(game, "player", game.player.combatSkill, turn);
        const creatureRolls = combatRolls(game, "creature", creature.combatSkill, turn);
        const playerAttack = Math.max(...playerRolls);
        const creatureAttack = Math.max(...creatureRolls);
        const playerDamage = playerAttack > creatureAttack ? 1 : 0;
        const creatureDamage = creatureAttack > playerAttack ? 1 : 0;
        const playerCanFlee = allDiceMatch(playerRolls);
        const creatureCanFlee =
            allDiceMatch(creatureRolls) &&
            (creature.combatSkill <= 3 || creature.combatSkill <= game.player.combatSkill);

        creature.health = Math.max(0, creature.health - playerDamage);
        game.player.health = Math.max(0, game.player.health - creatureDamage);
        if (game.generatorVersion >= 3) {
            game.experience += 1;
            room.combatExperience += 1;
        }
        game.lastCombat = {
            turn: game.generatorVersion >= 3 ? turn : turn + 1,
            playerRolls,
            creatureRolls,
            playerAttack,
            creatureAttack,
            playerDamage,
            creatureDamage,
            playerCanFlee,
            creatureCanFlee,
        };

        const messages = [
            `Kamp: Du slår [${playerRolls.join(", ")}] (angreb ${playerAttack}); væsenet slår [${creatureRolls.join(", ")}] (angreb ${creatureAttack}).`,
        ];
        if (game.generatorVersion >= 3) messages.push("Du får 1 kamperfaring for runden.");
        if (playerDamage) messages.push(`Dit angreb giver væsenet 1 skade (${creature.health}/${creature.maxHealth} liv).`);
        if (creatureDamage) messages.push(`Væsenets angreb giver dig 1 skade (${game.player.health}/${game.player.maxHealth} liv).`);
        if (!playerDamage && !creatureDamage) messages.push("Angrebene står lige; ingen tager skade.");

        if (creature.health === 0) {
            removeCreature(room, creature);
            messages.push("Væsenet er slået ud af kampen og flygter fra rummet.");
        } else if (game.generatorVersion >= 3 && creatureCanFlee) {
            removeCreature(room, creature);
            game.lastCombat.creatureFledTo = sendCreatureThroughNearestDoor(
                game,
                room,
                creature,
                messages,
            );
        }
        if (playerCanFlee && game.player.health > 0 && room.creature === creature) {
            messages.push(
                game.route.length > 0
                    ? "Dine terninger viser det samme. Du kan vælge at flygte tilbage gennem døren."
                    : "Dine terninger viser det samme, men herfra er der ingen dør tilbage at flygte igennem.",
            );
        }
        if (game.player.health === 0) {
            const returned = returnToCheckpoint(game);
            if (!returned) {
                const home = game.rooms["0,0"];
                game.roomId = home.id;
                game.player = safePosition(home, { x: 1, y: 4 }, creaturesIn(home));
                game.player.combatSkill = game.generatorVersion >= 3 ? 2 : 3;
                game.player.maxHealth = 10;
                game.player.health = 10;
                game.route = [];
                game.inventory.carried = [];
            }
            messages.push(
                returned
                    ? "Du mister kampen og bliver forvist til din hokora. Usikrede fund går tabt; verdenen fortsætter."
                    : "Du mister kampen og bliver forvist til skovens indgang. Usikrede fund går tabt; verdenen fortsætter.",
            );
        }
        return messages;
    }

    function step(game, action) {
        if (game.status !== "playing") return game;
        const room = currentRoom(game);
        const before = perception(game);
        const combat = action === "attack" && inCombatZone(game);
        const fleeing = action === "flee" && canFlee(game);
        const originalPlayer = { ...game.player };
        const collectible = action === "collect" ? collectibleAt(room, originalPlayer) : null;
        const creatureDestination = combat || fleeing ? null : creatureIntent(game, before);
        const move = DIRECTIONS[action];
        let playerDestination = game.player;
        let playerMoved = false;
        let crossing = null;

        if (move) {
            const destination = { x: game.player.x + move.x, y: game.player.y + move.y };
            if (inBounds(destination.x, destination.y)) {
                if (
                    isWalkable(room, destination.x, destination.y) &&
                    !creatureAt(room, destination)
                ) {
                    playerDestination = destination;
                    playerMoved = true;
                }
            } else {
                const door = doorAt(room, game.player);
                if (door && door.direction === action) crossing = door;
            }
        }

        let creatureMoved = false;
        if (room.creature && creatureDestination) {
            const canCreatureMove =
                isWalkable(room, creatureDestination.x, creatureDestination.y) &&
                (creatureDestination.x !== originalPlayer.x || creatureDestination.y !== originalPlayer.y) &&
                (creatureDestination.x !== playerDestination.x || creatureDestination.y !== playerDestination.y) &&
                !creatureAt(room, creatureDestination, room.creature);
            if (canCreatureMove) {
                creatureMoved =
                    creatureDestination.x !== room.creature.x ||
                    creatureDestination.y !== room.creature.y;
                room.creature.x = creatureDestination.x;
                room.creature.y = creatureDestination.y;
            }
        }
        if (!crossing) game.player = { ...game.player, ...playerDestination };
        game.turn += 1;
        if (!combat && !fleeing) game.player.health = Math.min(game.player.maxHealth, game.player.health + 1);
        const regrown = advanceRoom(game, room, !combat && !fleeing);

        const discoveries = [];
        if (room.creature && before.playerSeesCreature) {
            room.playerHasSeenCreature = true;
            if (action === "observe" || before.creatureSeesPlayer) {
                if (!room.playerStudiedCreature) {
                    room.playerStudiedCreature = true;
                    game.experience += 1;
                    discoveries.push(
                        action === "observe" && !before.creatureSeesPlayer
                            ? "Du aflæser væsenet, før det opdager dig."
                            : "Du lærer væsenets bevægelser at kende.",
                    );
                }
            }
        }
        if (room.creature && before.creatureSeesPlayer) {
            if (!room.creature.aware) {
                discoveries.push("Væsenet får øje på dig og husker dit nærvær.");
            }
            room.creature.aware = true;
            room.creature.lastKnownPlayer = originalPlayer;
            if (!room.creature.studiedPlayer) {
                room.creature.studiedPlayer = true;
                room.creature.experience += 1;
                room.creature.memoryOfPlayer = "interesting";
                discoveries.push("Væsenet lærer også noget om din måde at færdes på.");
            }
        }

        const messages = [...discoveries];
        if (action === "attack") {
            if (combat) {
                messages.push(...resolveCombat(game, room));
            } else {
                messages.push("Du er ikke i zone of combat; angrebet kan ikke udføres.");
            }
        }
        if (crossing) {
            const target = adjacentRoomPosition(room, crossing.direction);
            const targetId = roomId(target.x, target.y);
            const nextRoom =
                game.rooms[targetId] ||
                createRoom(target.x, target.y, game.seed, game.generatorVersion);
            const reverseDoors = nextRoom.doors[DIRECTIONS[crossing.direction].opposite];
            if (!reverseDoors.includes(crossing.position)) {
                throw new Error(`Room ${targetId} is missing the matching doorway.`);
            }
            if (room.creature?.aware && before.distance > 2) {
                room.creature.successfulEvasions += 1;
                if (room.creature.successfulEvasions >= 3) {
                    room.creature.successfulEvasions = 0;
                    room.creature.aware = false;
                    room.creature.lastKnownPlayer = null;
                    room.creature.memoryOfPlayer = "harmless";
                    messages.push("Efter flere rolige møder virker væsenet mindre opmærksomt på dig.");
                }
            }
            room.lastLeftTurn = game.turn;
            game.route.push({
                roomId: room.id,
                direction: crossing.direction,
                position: crossing.position,
            });
            game.rooms[targetId] = nextRoom;
            nextRoom.visits += 1;
            game.roomId = targetId;
            game.player = {
                ...game.player,
                ...entryPosition(crossing.direction, crossing.position),
            };

            if (nextRoom.creature && nextRoom.lastLeftTurn !== null) {
                const leftAtTurn = nextRoom.lastLeftTurn;
                const elapsedTurns = Math.max(0, game.turn - leftAtTurn);
                const moved = simulateCreatureWandering(
                    nextRoom,
                    elapsedTurns,
                    leftAtTurn,
                    game.player,
                    game.seed,
                );
                messages.push(
                    moved
                        ? "Du vender tilbage. Væsenet har flyttet sig, mens du var væk."
                        : "Du vender tilbage. Væsenet er stadig her.",
                );
            } else {
                messages.push(`Du går gennem døren mod ${DIRECTIONS[crossing.direction].label} ind i rum ${targetId}.`);
            }
            regrown.push(...advanceRoom(game, nextRoom));
            nextRoom.lastLeftTurn = null;
        } else if (action === "flee") {
            if (fleeing) {
                const lostExperience = Math.min(game.experience, room.combatExperience);
                game.experience -= lostExperience;
                room.combatExperience = 0;
                const entry = game.route.pop();
                const previous = game.rooms[entry.roomId];
                room.lastLeftTurn = game.turn;
                game.roomId = previous.id;
                game.player = {
                    ...game.player,
                    ...entryPosition(entry.direction, entry.position),
                };
                previous.visits += 1;
                if (previous.creature && previous.lastLeftTurn !== null) {
                    const leftAtTurn = previous.lastLeftTurn;
                    simulateCreatureWandering(
                        previous,
                        Math.max(0, game.turn - leftAtTurn),
                        leftAtTurn,
                        game.player,
                        game.seed,
                    );
                    previous.lastLeftTurn = null;
                }
                regrown.push(...advanceRoom(game, previous));
                messages.push(
                    `Du flygter gennem døren tilbage til rum ${previous.id} og mister ${lostExperience} kamperfaring fra rummet.`,
                );
            } else {
                messages.push("Du kan ikke flygte nu. Flygt kun efter ens terningeslag og gennem en tilbagevej.");
            }
        } else if (action === "collect") {
            if (collectible?.type === "resource") {
                collectible.item.available = false;
                collectible.item.regrowAt = game.turn + 20;
                game.inventory.carried.push(collectible.item.type);
                messages.push(`Du samler ${collectible.item.type}. Det vokser frem igen efter en tid.`);
            } else if (collectible?.type === "treasure") {
                collectible.item.available = false;
                game.inventory.carried.push(collectible.item.type);
                messages.push("Du finder et lille fund, som du kan sikre ved en hokora.");
            } else {
                messages.push("Der er ingen ressource ved dine fødder at samle op.");
            }
        } else if (action === "upgrade_rules") {
            if (game.generatorVersion < 3) {
                if (game.turn === 1 && game.experience === 0) game.player.combatSkill = 2;
                game.generatorVersion = 3;
                messages.push("Din rejse er opdateret med det nye kamp- og træningssystem.");
            } else {
                messages.push("Din rejse bruger allerede det nyeste kampsystem.");
            }
        } else if (action === "activate_checkpoint") {
            if (room.hokora && roomKey(room.hokora) === roomKey(originalPlayer)) {
                activateCheckpoint(game, room);
                messages.push("Du forankrer din vandring ved hokoraen. Dine fund er nu sikret.");
            } else {
                messages.push("Du må stå ved en hokora for at forankre din vandring.");
            }
        } else if (action === "train") {
            messages.push(trainCombatSkill(game, room));
        } else if (action === "return_checkpoint") {
            messages.push(
                returnToCheckpoint(game)
                    ? "Du vender tilbage til din senest aktiverede hokora. Usikrede fund går tabt."
                    : "Du har endnu ikke aktiveret en hokora.",
            );
        } else if (action === "yield") {
            const entry = game.route.pop();
            if (entry) {
                const previous = game.rooms[entry.roomId];
                const previousPosition = entryPosition(entry.direction, entry.position);
                room.lastLeftTurn = game.turn;
                game.roomId = previous.id;
                game.player = { ...game.player, ...previousPosition };
                previous.visits += 1;
                if (previous.creature && previous.lastLeftTurn !== null) {
                    const leftAtTurn = previous.lastLeftTurn;
                    const moved = simulateCreatureWandering(
                        previous,
                        Math.max(0, game.turn - leftAtTurn),
                        leftAtTurn,
                        game.player,
                        game.seed,
                    );
                    messages.push(
                        moved
                            ? "Du giver plads og vender tilbage. Væsenet har flyttet sig imens."
                            : "Du giver plads og vender roligt tilbage til det forrige rum.",
                    );
                    previous.lastLeftTurn = null;
                } else {
                    messages.push("Du giver plads og trækker dig roligt tilbage til det forrige rum.");
                }
                regrown.push(...advanceRoom(game, previous));
            } else {
                messages.push("Der er ikke et tidligere rum at trække sig tilbage til.");
            }
        } else {
            if (action === "observe" && !before.playerSeesCreature) {
                messages.push("Træerne og buskadset skjuler væsenet for dit blik.");
            } else if (
                action === "observe" &&
                room.creature &&
                before.playerSeesCreature &&
                room.playerStudiedCreature &&
                discoveries.length === 0
            ) {
                messages.push("Du iagttager væsenet uden at lære noget nyt denne gang.");
            } else if (action === "wait") {
                messages.push("Du venter. Skoven svarer med stilhed.");
            } else if (move && !playerMoved) {
                messages.push("Et træ, en lukket ydervæg eller væsenet spærrer vejen. Turen går alligevel.");
            } else if (playerMoved && tileAt(game, game.player.x, game.player.y) === "b") {
                messages.push("Du bevæger dig lydløst ind i buskadset.");
            } else if (playerMoved) {
                messages.push("Du fortsætter ad skovstien.");
            }

            if (creatureMoved && room.creature?.aware) {
                messages.push("Væsenet flytter sig forsigtigt gennem skoven.");
            } else if (creatureMoved && before.distance <= 5) {
                messages.push("Væsenet pusler mellem træerne.");
            }
        }
        if (regrown.length > 0) {
            messages.push(`Nye ${regrown.join(" og ")} er vokset frem i området.`);
        }

        game.message = messages.join(" ");
        addJournal(game, game.message);
        return game;
    }

    function zone(game) {
        if (!currentRoom(game).creature) return "Ingen væsener her";
        const current = perception(game);
        if (!current.playerSeesCreature) return "Ingen i syne";
        if (current.distance <= 2) return "Zone of combat";
        if (current.distance <= 5) return "Væsenet er nysgerrigt";
        return "På afstand";
    }

    function mood(game) {
        const room = currentRoom(game);
        const creature = room.creature;
        if (!creature) return "Ingen væsener har slået sig ned her.";
        const current = perception(game);
        if (current.creatureSeesPlayer && current.distance <= 2) {
            return "I er tæt på hinanden. Væsenet giver plads i stedet for at angribe.";
        }
        if (!room.playerStudiedCreature) {
            if (current.creatureSeesPlayer) return "Det har bemærket dig og følger dig med nysgerrighed.";
            if (creature.aware) return "Det husker dig, men har mistet dig af syne.";
            if (current.playerSeesCreature) return "Du kan se det, men har endnu ikke lært det at kende.";
            return "Skoven er stille.";
        }
        if (creature.temperament === "shy" && current.creatureSeesPlayer) {
            return "Det virker sky og søger afstand, når det mærker dit nærvær.";
        }
        if (creature.temperament === "watchful" && current.creatureSeesPlayer) {
            return "Det iagttager dig på afstand uden at nærme sig.";
        }
        if (creature.memoryOfPlayer === "harmless") {
            return "Det genkender dig som ufarlig og følger roligt med blikket.";
        }
        if (creature.memoryOfPlayer === "interesting" && current.creatureSeesPlayer) {
            return "Det husker jeres tidligere møde og er nysgerrigt på dig.";
        }
        if (current.creatureSeesPlayer) return "Det har bemærket dig og følger dig med nysgerrighed.";
        if (creature.aware) return "Det husker dig, men har mistet dig af syne.";
        if (current.playerSeesCreature) return "Det pusler roligt rundt og har endnu ikke set dig.";
        return "Skoven er stille.";
    }

    function playerKnowledge(game) {
        const room = currentRoom(game);
        if (!room.creature) return "Du har ikke mødt et væsen i dette rum.";
        if (room.playerStudiedCreature && room.creature.studiedPlayer) {
            return "I har begge lært af mødet; væsenet kender nu også din færden.";
        }
        if (room.playerStudiedCreature) {
            return "Du har lært væsenets bevægelser at kende uden selv at blive opdaget.";
        }
        if (room.playerHasSeenCreature) return "Du har set væsenet, men endnu ikke studeret det.";
        return "Du har endnu ikke studeret væsenet.";
    }

    function inventorySummary(game) {
        const carried = game.inventory.carried.length;
        const secured = game.inventory.secured.length;
        return `Sikret: ${secured} · Usikret: ${carried}`;
    }

    function combatSummary(game) {
        const player = `Dig: kamp ${game.player.combatSkill}, liv ${game.player.health}/${game.player.maxHealth}`;
        const creature = currentRoom(game).creature;
        const cost = trainingCost(game.player.combatSkill);
        const progression = cost
            ? `Erfaring: ${game.experience} · næste niveau koster ${cost.experience} erfaring og ${cost.treasure} ${cost.treasure === 1 ? "skat" : "skatte"}`
            : `Erfaring: ${game.experience} · kampfærdighed er maksimal`;
        if (!creature) return `${player} · ${progression}`;
        return `${player} · Væsen: kamp ${creature.combatSkill}, liv ${creature.health}/${creature.maxHealth} · ${progression}`;
    }

    function combatResult(game) {
        if (!game.lastCombat) return "Ingen kamp endnu.";
        const combat = game.lastCombat;
        if (combat.playerDamage && combat.creatureDamage) return "Begge ramte hinanden.";
        if (combat.playerDamage) return "Du vandt dette sammenstød.";
        if (combat.creatureDamage) return "Væsenet vandt dette sammenstød.";
        return "Uafgjort — ingen skade.";
    }

    function isDoor(room, x, y) {
        return doorAt(room, { x, y }) !== null;
    }

    return {
        WIDTH,
        HEIGHT,
        GENERATOR_VERSION,
        DIRECTIONS,
        createGame,
        createRoom,
        currentRoom,
        combatResult,
        combatSummary,
        canFlee,
        canTrain,
        edgeDoors,
        hasLineOfSight,
        inventorySummary,
        inCombatZone,
        isDoor,
        mood,
        perception,
        playerKnowledge,
        step,
        tileAt,
        trainingCost,
        visibleCreatures,
        zone,
    };
});

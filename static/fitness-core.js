(function (root) {
    "use strict";

    function expand(workout) {
        const validRepeat = n => Number.isInteger(n) && n >= 1 && n <= 100;
        if (!workout || !validRepeat(workout.repeats) || !Array.isArray(workout.blocks)
            || workout.blocks.length < 1 || workout.blocks.length > 50) {
            throw new Error("Use 1-50 groups and repeats from 1-100.");
        }
        let count = 0;
        for (const block of workout.blocks) {
            if (!validRepeat(block.repeats) || !Array.isArray(block.steps)
                || block.steps.length < 1 || block.steps.length > 50) {
                throw new Error("Each group needs 1-50 steps and repeats from 1-100.");
            }
            for (const step of block.steps) {
                if (typeof step.label !== "string" || !step.label.trim()
                    || Array.from(step.label).length > 100 || !validRepeat(step.repeats)
                    || (step.seconds !== null && (!Number.isInteger(step.seconds)
                        || step.seconds < 1 || step.seconds > 86400))) {
                    throw new Error("Name every step, use repeats from 1-100, and durations from 1-86,400 seconds or manual rest.");
                }
                count += workout.repeats * block.repeats * step.repeats;
            }
        }
        if (count > 10000) throw new Error("Limit the workout to 10,000 expanded steps.");
        const steps = [];
        for (let round = 1; round <= workout.repeats; round++) {
            workout.blocks.forEach((block, group) => {
                for (let repeat = 1; repeat <= block.repeats; repeat++) {
                    for (const step of block.steps) {
                        for (let repetition = 1; repetition <= step.repeats; repetition++) {
                            steps.push({
                                label: step.label, seconds: step.seconds,
                                context: `Workout ${round}/${workout.repeats} - Group ${group + 1}, round ${repeat}/${block.repeats} - Step repeat ${repetition}/${step.repeats}`,
                            });
                        }
                    }
                }
            });
        }
        return steps;
    }

    function preset(kind, rounds) {
        const step = (label, seconds) => ({ label, seconds, repeats: 1 });
        return {
            repeats: 1,
            blocks: [{
                repeats: kind === "custom" ? 1 : rounds,
                steps: kind === "tabata" ? [step("Work", 20), step("Rest", 10)]
                    : kind === "emom" ? [step("EMOM", 60)] : [step("Work", 30), step("Rest - continue when ready", null)],
            }],
        };
    }

    class Runner {
        constructor(steps) {
            if (!steps.length) throw new Error("The workout has no steps.");
            this.steps = steps;
            this.reset();
        }
        reset() {
            this.index = 0;
            this.state = "ready";
            this.remaining = (this.steps[0].seconds || 0) * 1000;
            this.deadline = 0;
        }
        enter(now) {
            if (this.index >= this.steps.length) {
                this.state = "done";
                this.remaining = 0;
            } else if (this.steps[this.index].seconds === null) {
                this.state = "waiting";
                this.remaining = 0;
            } else {
                this.state = "running";
                this.remaining = this.steps[this.index].seconds * 1000;
                this.deadline = now + this.remaining;
            }
        }
        tick(now) {
            while (this.state === "running" && now >= this.deadline) {
                const boundary = this.deadline;
                this.index++;
                this.enter(boundary);
            }
            if (this.state === "running") this.remaining = this.deadline - now;
        }
        toggle(now) {
            if (this.state === "running") {
                this.tick(now);
                if (this.state === "running") this.state = "paused";
            } else if (this.state === "ready") this.enter(now);
            else if (this.state === "paused") {
                this.deadline = now + this.remaining;
                this.state = "running";
            } else if (this.state === "waiting") {
                this.index++;
                this.enter(now);
            }
        }
    }

    const api = { expand, preset, Runner };
    if (typeof module !== "undefined" && module.exports) module.exports = api;
    else root.Fitness = api;
})(typeof globalThis !== "undefined" ? globalThis : this);

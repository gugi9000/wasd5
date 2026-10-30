(async function () {
    "use strict";
    const root = document.getElementById("timer-player");
    const error = document.getElementById("timer-error");
    const toggle = document.getElementById("timer-toggle");
    const next = document.getElementById("timer-continue");
    const restart = document.getElementById("timer-restart");
    const fail = message => { error.textContent = message; error.hidden = false; };
    const remove = document.getElementById("delete-timer");
    if (remove) remove.addEventListener("click", async () => {
        if (!window.confirm("Permanently delete this public timer?")) return;
        remove.disabled = true;
        try {
            const response = await fetch(`/health/timer/${root.dataset.id}/delete`, {
                method: "POST", headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ csrf: remove.dataset.csrf }),
            });
            if (response.status !== 204) throw new Error("Delete failed. Check your sign-in and reload to try again.");
            window.location.assign("/health/timer");
        } catch (e) { fail(e.message); remove.disabled = false; }
    });

    try {
        const response = await fetch(`/health/timer/${root.dataset.id}/data`);
        if (!response.ok) throw new Error("Could not load the workout. Reload to try again.");
        const timer = await response.json();
        const steps = Fitness.expand(timer.workout);
        const runner = new Fitness.Runner(steps);
        const outline = document.getElementById("timer-outline");
        for (const block of timer.workout.blocks) {
            const item = document.createElement("li");
            item.textContent = `${block.repeats} group round(s): ` + block.steps.map(step =>
                `${step.label} (${step.seconds === null ? "wait for key / button" : step.seconds + "s"}, ${step.repeats} repeat(s))`).join("; ");
            outline.append(item);
        }
        outline.prepend(document.createTextNode(`Whole workout: ${timer.workout.repeats} repeat(s).`));
        const clock = document.getElementById("timer-clock");
        const label = document.getElementById("timer-label");
        const progress = document.getElementById("timer-progress");
        const status = document.getElementById("timer-status");
        const upcoming = document.getElementById("timer-next");
        function render() {
            const done = runner.state === "done";
            const waiting = runner.state === "waiting";
            const step = steps[runner.index];
            const seconds = Math.ceil(runner.remaining / 1000);
            clock.textContent = done ? "Done!" : (waiting || step.seconds === null) ? "--:--"
                : `${Math.floor(seconds / 60).toString().padStart(2, "0")}:${(seconds % 60).toString().padStart(2, "0")}`;
            const title = done ? "Workout complete" : step.label;
            if (label.textContent !== title) label.textContent = title;
            progress.textContent = done ? `${steps.length} / ${steps.length} steps` : `${runner.index + 1} / ${steps.length} - ${step.context}`;
            const message = { ready: "Ready to start", running: "Running", paused: "Paused",
                waiting: "Rest as long as you need. Press Continue, Space, or Enter.", done: "Workout complete" }[runner.state];
            if (status.textContent !== message) status.textContent = message;
            upcoming.textContent = steps[runner.index + 1] ? `Next: ${steps[runner.index + 1].label}` : "";
            toggle.hidden = waiting;
            toggle.disabled = done;
            toggle.textContent = runner.state === "running" ? "Pause" : runner.state === "paused" ? "Resume" : "Start";
            next.hidden = !waiting;
            restart.disabled = false;
        }
        const act = () => { runner.toggle(performance.now()); render(); };
        toggle.addEventListener("click", act);
        next.addEventListener("click", act);
        restart.addEventListener("click", () => { runner.reset(); render(); });
        document.addEventListener("keydown", event => {
            if (event.repeat || event.altKey || event.ctrlKey || event.metaKey
                || !["Space", "Enter"].includes(event.code)
                || event.target.closest("button, a, input, textarea, select, summary, [contenteditable]")) return;
            event.preventDefault();
            act();
        });
        render();
        window.setInterval(() => { runner.tick(performance.now()); render(); }, 100);
    } catch (e) { fail(e.message); }
})();

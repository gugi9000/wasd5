(async function () {
    "use strict";
    const root = document.getElementById("timer-editor");
    const id = root.dataset.id;
    const blocks = document.getElementById("timer-blocks");
    const form = document.getElementById("timer-form");
    const error = document.getElementById("timer-error");
    const fields = document.getElementById("editor-fields");
    let uid = 0;
    const fail = message => { error.textContent = message; error.hidden = false; };

    function input(parent, text, value, type, min, max) {
        const wrapper = document.createElement("div");
        wrapper.className = "col-sm";
        const label = document.createElement("label");
        label.className = "form-label";
        label.textContent = text;
        const field = document.createElement("input");
        field.id = `timer-field-${++uid}`;
        label.htmlFor = field.id;
        field.className = "form-control";
        field.type = type;
        field.value = value;
        field.required = true;
        if (type === "number") { field.min = min; field.max = max; }
        else field.maxLength = 100;
        wrapper.append(label, field);
        parent.append(wrapper);
        return field;
    }

    function button(parent, text, action) {
        const element = document.createElement("button");
        element.type = "button";
        element.className = "btn btn-sm btn-outline-secondary";
        element.textContent = text;
        element.addEventListener("click", action);
        parent.append(element);
    }

    function controls(parent, element, container, name) {
        const row = document.createElement("div");
        row.className = "d-flex flex-wrap gap-2 mt-2";
        button(row, `Move ${name} up`, () => {
            if (element.previousElementSibling) container.insertBefore(element, element.previousElementSibling);
        });
        button(row, `Move ${name} down`, () => {
            if (element.nextElementSibling) container.insertBefore(element.nextElementSibling, element);
        });
        button(row, `Remove ${name}`, () => element.remove());
        parent.append(row);
    }

    function addStep(container, step) {
        const row = document.createElement("div");
        row.className = "timer-step";
        const inputs = document.createElement("div");
        inputs.className = "row g-2";
        const label = input(inputs, "Step name", step.label, "text");
        label.dataset.field = "label";
        const seconds = input(inputs, "Seconds", step.seconds === null ? 30 : step.seconds, "number", 1, 86400);
        seconds.dataset.field = "seconds";
        const repeats = input(inputs, "Step repeats", step.repeats, "number", 1, 100);
        repeats.dataset.field = "repeats";
        const manualLabel = document.createElement("label");
        manualLabel.className = "form-check-label mt-2";
        const manual = document.createElement("input");
        manual.type = "checkbox";
        manual.className = "form-check-input me-2";
        manual.checked = step.seconds === null;
        manual.dataset.field = "manual";
        seconds.disabled = manual.checked;
        manual.addEventListener("change", () => { seconds.disabled = manual.checked; });
        manualLabel.append(manual, "Wait for button / key press (no countdown)");
        row.append(inputs, manualLabel);
        controls(row, row, container, "step");
        container.append(row);
    }

    function addBlock(block) {
        const card = document.createElement("section");
        card.className = "card mb-3";
        const body = document.createElement("div");
        body.className = "card-body";
        const heading = document.createElement("h2");
        heading.className = "h5";
        heading.textContent = "Repeatable group";
        body.append(heading);
        const repeats = input(body, "Group repeats", block.repeats, "number", 1, 100);
        repeats.dataset.field = "group-repeats";
        repeats.classList.add("repeat-input", "mb-3");
        const steps = document.createElement("div");
        steps.dataset.steps = "";
        block.steps.forEach(step => addStep(steps, step));
        body.append(steps);
        button(body, "Add step", () => addStep(steps, { label: "Work", seconds: 30, repeats: 1 }));
        controls(body, card, blocks, "group");
        card.append(body);
        blocks.append(card);
    }

    function render(workout) {
        blocks.replaceChildren();
        document.getElementById("workout-repeats").value = workout.repeats;
        workout.blocks.forEach(addBlock);
    }

    function workout() {
        return {
            repeats: Number(document.getElementById("workout-repeats").value),
            blocks: Array.from(blocks.children, block => ({
                repeats: Number(block.querySelector('[data-field="group-repeats"]').value),
                steps: Array.from(block.querySelector("[data-steps]").children, step => {
                    const value = name => step.querySelector(`[data-field="${name}"]`);
                    return { label: value("label").value.trim(), repeats: Number(value("repeats").value),
                        seconds: value("manual").checked ? null : Number(value("seconds").value) };
                }),
            })),
        };
    }

    document.getElementById("add-block").addEventListener("click", () => addBlock({ repeats: 1, steps: [{ label: "Work", seconds: 30, repeats: 1 }] }));
    document.getElementById("preset").addEventListener("change", event => {
        document.getElementById("preset-rounds").value = event.target.value === "tabata" ? 8 : 10;
    });
    document.getElementById("apply-preset").addEventListener("click", () => {
        const kind = document.getElementById("preset").value;
        const rounds = document.getElementById("preset-rounds");
        if (!rounds.reportValidity()) return;
        if (!window.confirm("Replace the current steps with this preset?")) return;
        render(Fitness.preset(kind, Number(rounds.value)));
        if (!document.getElementById("timer-title").value) {
            document.getElementById("timer-title").value = kind === "emom" ? `EMOM ${rounds.value}` : kind === "tabata" ? "Tabata" : "Custom workout";
        }
    });

    form.addEventListener("submit", async event => {
        event.preventDefault();
        error.hidden = true;
        const save = document.getElementById("save-timer");
        try {
            const definition = workout();
            Fitness.expand(definition);
            save.disabled = true;
            const response = await fetch(id ? `/health/timer/${id}` : "/health/timer", {
                method: "POST", headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ title: document.getElementById("timer-title").value,
                    description: document.getElementById("timer-description").value,
                    csrf: form.elements.csrf.value, workout: definition }),
            });
            if (!response.ok || response.redirected) throw new Error("Save failed. Check your sign-in, reload if your session expired, and try again. " + (response.redirected ? "" : await response.text()));
            window.location.assign(`/health/timer/${await response.json()}`);
        } catch (e) { fail(e.message); }
        finally { save.disabled = false; }
    });

    try {
        if (id) {
            const response = await fetch(`/health/timer/${id}/data`);
            if (!response.ok) throw new Error("Could not load this timer. Reload to try again.");
            const timer = await response.json();
            Fitness.expand(timer.workout);
            document.getElementById("timer-title").value = timer.title;
            document.getElementById("timer-description").value = timer.description;
            render(timer.workout);
        } else render(Fitness.preset("custom", 1));
        fields.disabled = false;
    } catch (e) { fail(e.message); }
})();

(() => {
    const guide = document.querySelector(".breathing-guide");
    const toggle = document.getElementById("breathing-toggle");
    const patterns = {
        box: document.getElementById("breathing-box"),
        triangle: document.getElementById("breathing-triangle"),
    };

    function setRunning(running) {
        guide.classList.toggle("is-running", running);
        toggle.textContent = running ? "Pause animation" : "Start animation";
        toggle.setAttribute("aria-pressed", String(running));
    }

    guide.querySelectorAll('input[name="breathing-pattern"]').forEach((input) => {
        input.addEventListener("change", () => {
            for (const [name, pattern] of Object.entries(patterns)) {
                pattern.hidden = name !== input.value;
            }
        });
    });

    toggle.disabled = false;
    toggle.addEventListener("click", () => {
        setRunning(!guide.classList.contains("is-running"));
    });
    setRunning(!matchMedia("(prefers-reduced-motion: reduce)").matches);
})();

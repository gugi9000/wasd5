const status = document.getElementById("makibishi-status");
const canvas = document.getElementById("makibishi-canvas");

async function loadMakibishi() {
    let init;
    try {
        ({ default: init } = await import("/static/makibishi/makibishi.js"));
    } catch (error) {
        throw new Error(
            "Makibishi's browser build is missing or could not be downloaded. " +
            "Deploy the wasm-bindgen web build to /static/makibishi/ and reload.",
            { cause: error },
        );
    }

    try {
        await init({ module_or_path: "/static/makibishi/makibishi_bg.wasm" });
    } catch (error) {
        throw new Error(
            "Makibishi could not start. Check that makibishi_bg.wasm and its assets " +
            "are deployed under /static/makibishi/ and that your browser supports WebGL2 and WebAssembly.",
            { cause: error },
        );
    }
    status.hidden = true;
    canvas.addEventListener("pointerdown", () => canvas.focus({ preventScroll: true }));
    canvas.addEventListener("contextmenu", (event) => event.preventDefault());
}

loadMakibishi().catch((error) => {
    status.hidden = false;
    status.dataset.error = "true";
    status.textContent = error.message;
    console.error("Makibishi startup failed:", error);
});

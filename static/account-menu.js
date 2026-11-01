(async function () {
    "use strict";

    const login = document.getElementById("account-login");
    const admin = document.getElementById("account-admin");
    const username = document.getElementById("account-username");
    const error = document.getElementById("account-menu-error");
    if (!login || !admin || !username || !error) return;

    try {
        const response = await fetch("/api/navigation/account", {
            headers: { Accept: "application/json" },
            cache: "no-store",
        });
        if (!response.ok) throw new Error("Could not load the account menu.");
        const account = await response.json();
        if (account.username !== null && typeof account.username !== "string") {
            throw new Error("Invalid account menu response.");
        }
        const signedIn = account.username !== null;
        username.textContent = signedIn ? `Signed in as ${account.username}` : "";
        login.hidden = signedIn;
        admin.hidden = !signedIn;
    } catch (reason) {
        error.textContent = reason.message;
        error.hidden = false;
    }
})();

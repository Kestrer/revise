import "normalize.css";

const params = new URLSearchParams(location.search);
if (params.has("loginError")) {
	document.getElementById("loginError")!.append("Username or password was incorrect.");
}
if (params.has("createAccountError")) {
	document.getElementById("createAccountError")!.append("An account with that email already exists.");
}
history.pushState(null, "", "/");

(() => {
	const form = document.getElementById("createAccountForm") as HTMLFormElement;
	const password = form.elements.namedItem("password") as HTMLInputElement;
	const password2 = form.elements.namedItem("password2") as HTMLInputElement;

	const updateValidity = (): void => {
		if (password.value !== password2.value) {
			password2.setCustomValidity("Passwords are not equal");
		} else {
			password2.setCustomValidity("");
		}
	};

	password.addEventListener("input", updateValidity);
	password2.addEventListener("input", updateValidity);

	form.addEventListener("submit", () => password2.disabled = true);
})();

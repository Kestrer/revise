import "normalize.css";

import { JSX } from "solid-js";
import { render } from "solid-js/web";

import SetPassword from "./setPassword";

const params = new URLSearchParams(location.search);

const loginError = params.has("loginError")
	&& <p style="color:red">Username or password was incorrect.</p>;

const createAccountError = params.has("createAccountError")
	&& <p style="color:red">An account with that email already exists.</p>;

history.pushState(null, "", "/");

function App(): JSX.Element {
	return <>
		<form action="/accounts/login" method="post">
			<h2>Log In</h2>
			{loginError}
			<p><label>Email: <input name="email" type="email" required /></label></p>
			<p><label>Password: <input name="password" type="password" required /></label></p>
			<button>Log in</button>
		</form>

		<form id="createAccountForm" action="/accounts/create" method="post">
			<h2>Create Account</h2>
			{createAccountError}
			<p><label>Email: <input name="email" type="email" required /></label></p>
			<SetPassword passwordName="password" />
			<button>Create account</button>
		</form>
	</>;
}

render(App, document.getElementById("app")!);

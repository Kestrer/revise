import { JSX } from "solid-js";

export default function(props: {
	passwordName?: string,
	passwordValue?: (value: string) => void,
}): JSX.Element {
	const password1 = (<input name={props.passwordName} type="password" minlength="8" required />) as HTMLInputElement;
	const password2 = (<input type="password" minlength="8" required />) as HTMLInputElement;

	const updateValidity = (): void => {
		if (password1.value !== password2.value) {
			password2.setCustomValidity("Passwords are not equal");
		} else {
			password2.setCustomValidity("");
		}
	};

	password1.addEventListener("input", () => {
		updateValidity();
		props.passwordValue?.(password1.value);
	});
	password2.addEventListener("input", updateValidity);

	return <>
		<p><label>Password: {password1}</label></p>
		<p><label>Confirm password: {password2}</label></p>
	</>;
}

import "normalize.css";

import { createMemo, createResource, createSignal } from "solid-js";
import { For } from "solid-js";
import { JSX } from "solid-js";
import { render } from "solid-js/web";

import "./dashboard.scss";

interface Card {
    id: number,
    terms: string,
    definitions: string,
    case_sensitive: boolean,
    knowledge: number,
    safety_net: boolean,
}

function App(): JSX.Element {
	const [cards] = createResource(async () => {
		const response = await fetch("/cards");
		return await response.json() as Card[];
	});

	const [addingCard, setAddingCard] = createSignal(false);

	return <>
		1
		{() => {
			if (addingCard()) {
				return <CardEditor
					onSave={(terms, definitions) => {
						void (async () => {
							try {
								const res = await fetch("/cards", {
									method: "POST",
									body: JSON.stringify({
										terms,
										definitions,
										case_sensitive: false,
									}),
									headers: {
										"content-type": "application/json",
									},
								});
								if (!res.ok) {
									throw new Error();
								}
								setAddingCard(false);
							} catch (e) {
								alert("something went wrong :(");
							}
						})();
					}}
					onCancel={() => setAddingCard(false)}
				/>;
			} else {
				return <button type="button" onClick={() => setAddingCard(true)}>Create card</button>;
			}
		}}
		{() => {
			const cards_ = cards();
			if (cards_ === undefined) {
				return <p>Loading...</p>;
			}
			return <For each={cards_}>{card => {
				return <div class="card">
					<p>{card.terms}</p>
					<p>{card.definitions}</p>
				</div>;
			}}</For>;
		}}
	</>;
}

function CardEditor(props: {
	onSave: (terms: string, definitions: string) => void,
	onCancel: () => void,
}): JSX.Element {
	const terms = <textarea required onInput={() => {
		terms.setCustomValidity(
			/^\n+$/.test(terms.value)
			? "at least one term must be provided"
			: ""
		);
	}}></textarea> as HTMLTextAreaElement;
	const definitions = <textarea required onInput={() => {
		definitions.setCustomValidity(
			/^\n+$/.test(definitions.value)
			? "at least one definition must be provided"
			: ""
		);
	}}></textarea> as HTMLTextAreaElement;

	const [disabled, setDisabled] = createSignal(false);

	return <div class="card">
		<form action="javascript:void(0)" onSubmit={() => {
			if (!disabled()) {
				props.onSave(terms.value, definitions.value);
				setDisabled(true);
			}
		}}>
			{terms}
			{definitions}
			<button>Save</button>
			<button type="button" disabled={disabled()} onClick={() => props.onCancel()}>Cancel</button>
		</form>
	</div>;
}

render(App, document.getElementById("app")!);

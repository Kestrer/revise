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
		try {
			const response = await fetch("/cards");
			if (response.status !== 200) {
				throw new Error(`Status of ${response.status}: ${await response.text()}`);
			}
			return await response.json() as Card[];
		} catch (e) {
			console.error(e);
			return null;
		}
	});

	const [addingCard, setAddingCard] = createSignal(false);

	return <>
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
									throw new Error(`${res.status}`);
								}
								setAddingCard(false);
							} catch (e) {
								console.error(e);
								alert(`could not create card: ${(e as Error).toString()}`);
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
			} else if (cards_ === null) {
				return <p>Failed to retrieve cards. Try reloading the page.</p>;
			} else {
				return <For each={cards_}>{card => <Card card={card} />}</For>;
			}
		}}
	</>;
}

function Card(props: { card: Card }): JSX.Element {
	const [editing, setEditing] = createSignal(false);

	return createMemo(() => {
		if (editing()) {
			return <CardEditor
				terms={props.card.terms}
				definitions={props.card.definitions}
				onSave={(terms, definitions) => {
					void (async () => {
						try {
							const res = await fetch(`/cards/${props.card.id}`, {
								method: "PUT",
								body: JSON.stringify({ terms, definitions }),
								headers: {
									"content-type": "application/json",
								},
							});
							if (!res.ok) {
								throw new Error(`${res.status}`);
							}
							setEditing(false);
						} catch (e) {
							console.error(e);
							alert(`could not save card: ${(e as Error).toString()}`);
						}
					})();
				}}
				onCancel={() => setEditing(false)}
			/>
		} else {
			return <div class="card">
				<p>{props.card.terms}</p>
				<p>{props.card.definitions}</p>
				<button type="button" onClick={() => setEditing(true)}>Edit</button>
			</div>;
		}
	});
}

function CardEditor(props: {
	terms?: string,
	definitions?: string,
	onSave: (terms: string, definitions: string) => void,
	onCancel: () => void,
}): JSX.Element {
	const initTerms = props.terms ?? "";
	const initDefinitions = props.definitions ?? "";

	const terms = <textarea required onInput={() => {
		terms.setCustomValidity(
			/\S/.test(terms.value) ? "" : "at least one term must be provided"
		);
	}}>{initTerms}</textarea> as HTMLTextAreaElement;
	const definitions = <textarea required onInput={() => {
		definitions.setCustomValidity(
			/\S/.test(terms.value) ? "" : "at least one definition must be provided"
		);
	}}>{initDefinitions}</textarea> as HTMLTextAreaElement;

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

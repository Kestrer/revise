import "normalize.css";

import { createEffect, createMemo, createResource, createSignal } from "solid-js";
import { For } from "solid-js";
import { JSX } from "solid-js";
import { render } from "solid-js/web";

import "./dashboard.scss";

interface Card {
	id: number,
	createdAt: number,
	terms: string,
	definitions: string,
	caseSensitive: boolean,
	knowledge: number,
	safetyNet: boolean,
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
		<h1>dashboard</h1>
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
										caseSensitive: false,
										createdAt: Date.now(),
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
		<UserAccount />
	</>;
}

function Card(props: { card: Card }): JSX.Element {
	const [editing, setEditing] = createSignal(false);
	const [deleting, setDeleting] = createSignal(false);

	createEffect(() => {
		if (!deleting()) {
			return;
		}

		void (async () => {
			try {
				const res = await fetch(`/cards/${props.card.id}`, { method: "DELETE" });
				if (!res.ok) {
					throw new Error(`${res.status}`);
				}
			} catch (e) {
				console.error(e);
				alert(`could not delete card: ${(e as Error).toString()}`);
			} finally {
				setDeleting(false);
			}
		})();
	});

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
				<button type="button" disabled={deleting()} onClick={() => setDeleting(true)}>Delete</button>
				<p>Created at {new Date(props.card.createdAt).toLocaleString()}</p>
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

interface Me {
	email: string,
}

function UserAccount(): JSX.Element {
	const [me] = createResource(async () => {
		try {
			const response = await fetch("/accounts/me");
			if (response.status !== 200) {
				throw new Error(`Status of ${response.status}: ${await response.text()}`);
			}
			return await response.json() as Me;
		} catch (e) {
			console.error(e);
			return null;
		}
	});

	return <>
		<h2>User Account</h2>
		<form action="/accounts/logout" method="post"><button>Log Out</button></form>
		{() => {
			const me_ = me();
			if (me_ === undefined) {
				return;
			} else if (me_ === null) {
				return <p>Failed to retrieve user data. Try reloading the page.</p>;
			} else {
				const [newEmail, setNewEmail] = createSignal(me_.email);
				const [saving, setSaving] = createSignal(false);

				return <form action="javascript:void(0)" onSubmit={() => {
					setSaving(true);
					void (async () => {
						try {
							const res = await fetch("/accounts/me", {
								method: "PUT",
								body: JSON.stringify({ email: newEmail() }),
								headers: {
									"content-type": "application/json",
								},
							});
							if (!res.ok) {
								throw new Error(`${res.status}`);
							}
						} catch (e) {
							console.log(e);
							alert(`could not change email: ${(e as Error).toString()}`)
						} finally {
							setSaving(false);
						}
					})();
				}}>
					<label>Email: <input
						type="email"
						required
						value={newEmail()}
						onInput={e => setNewEmail((e.target as HTMLInputElement).value)}
					/></label>
					<button disabled={saving() || newEmail() === me_.email}>Save</button>
				</form>;
			}
		}}
		<form action="/accounts/delete" method="post"><button>Delete Account</button></form>
	</>;
}

render(App, document.getElementById("app")!);

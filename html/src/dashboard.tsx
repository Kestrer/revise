import "normalize.css";

import { SetStoreFunction, createStore } from "solid-js/store";
import { batch, createEffect, createMemo, createSignal } from "solid-js";
import { For } from "solid-js";
import { JSX } from "solid-js";
import { render } from "solid-js/web";

import "./dashboard.scss";

interface UserData {
	email: string,
	cards: Card[],
}

interface Card {
	id: number,
	created_at: number,
	terms: string,
	definitions: string,
	case_sensitive: boolean,
	knowledge: number,
	safety_net: boolean,
}

type UserDataState = "loading"
	| "error"
	| { data: UserData, setData: SetStoreFunction<UserData> };

function App(): JSX.Element {
	const [userData, setUserData] = createSignal<UserDataState>("loading");

	// eslint-disable-next-line prefer-const
	let events: UserEvents;

	const onConnect = (): void => {
		events.send({
			type: "SetQueryOpts",
			limit: 10,
			offset: 0,
		});
	};

	const onError = (e: Event): void => {
		console.error("ws error: ", e);
		setUserData("error");
	};

	const onMessage = (message: WsResponse): void => {
		if (message.type === "Update") {
			batch(() => {
				let data: UserData, setData: SetStoreFunction<UserData>;

				const userData_ = userData();
				if (typeof(userData_) === "object") {
					data = userData_.data;
					setData = userData_.setData;
				} else {
					[data, setData] = createStore<UserData>({
						email: "",
						cards: [],
					});
					setUserData({ data, setData });
				}

				if (message.email !== undefined) {
					setData("email", message.email);
				}
				if (message.cards !== undefined) {
					setData("cards", message.cards);
				}
			});
		} else if (message.type === "LogOut") {
			location.href = "/accounts/clear-session-cookie";
		} else if (message.type === "Error") {
			console.error("error from ws:", message.message);
			setUserData("error");
		}
	};

	events = new UserEvents(onConnect, onError, onMessage);

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
			const userData_ = userData();
			if (userData_ === "loading") {
				return <p>Loading...</p>;
			} else if (userData_ === "error") {
				return <p>Failed to retrieve cards. Try reloading the page.</p>;
			} else {
				return <For each={userData_.data.cards}>{card => <Card card={card} />}</For>;
			}
		}}
		<UserAccount userData={userData()} />
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
				<p>Created at {new Date(props.card.created_at).toLocaleString()}</p>
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

function UserAccount(props: { userData: UserDataState }): JSX.Element {
	return <>
		<h2>User Account</h2>
		<form action="/accounts/logout" method="post"><button>Log Out</button></form>
		{() => {
			const userData = props.userData;
			if (userData === "loading") {
				return;
			} else if (userData === "error") {
				return <p>Failed to retrieve user data. Try reloading the page.</p>;
			}
			const data = userData.data;
			return createMemo(() => {
				const [newEmail, setNewEmail] = createSignal(data.email);
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
					<button disabled={saving() || newEmail() === data.email}>Save</button>
				</form>;
			});
		}}
		<form action="/accounts/delete" method="post"><button>Delete Account</button></form>
	</>;
}

type WsRequest = { type: "SetQueryOpts", limit: number, offset: number };

type WsResponse = never
	| { type: "Update", email?: string, cards?: Card[] }
	| { type: "LogOut" }
	| { type: "Error", message: string };

class UserEvents {
	private ws: WebSocket;

	constructor(
		private readonly onConnect: () => void,
		private readonly onError: (e: Event) => void,
		private readonly onMessage: (m: WsResponse) => void,
	) {
		this.reconnect();
	}

	private reconnect(): void {
		this.ws = new WebSocket(`wss://${window.location.host}/user-events`);
		this.ws.addEventListener("open", () => this.onConnect());
		this.ws.addEventListener("error", e => this.onError(e));
		this.ws.addEventListener("message", e => {
			this.onMessage(JSON.parse(e.data as string) as WsResponse);
		});
		this.ws.addEventListener("close", () => {
			console.error("Websocket closed. Attempting reconnect.");
			setTimeout(() => this.reconnect(), 2000);
		});
	}

	send(request: WsRequest): void {
		this.ws.send(JSON.stringify(request));
	}
}

render(App, document.getElementById("app")!);

CREATE EXTENSION pgcrypto;

CREATE TABLE IF NOT EXISTS users (
	id INT8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	email TEXT UNIQUE CHECK(octet_length(email) != 0) NOT NULL,
	password TEXT CHECK(octet_length(password) != 0) NOT NULL
);

CREATE TABLE IF NOT EXISTS cards (
	id INT8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	owner INT8 REFERENCES users ON DELETE CASCADE NOT NULL,
	created_at INT8 NOT NULL, -- Javascript timestamp
	terms TEXT CHECK(terms ~ '\S') NOT NULL,
	definitions TEXT CHECK(definitions ~ '\S') NOT NULL,
	case_sensitive BOOLEAN NOT NULL,
	knowledge INT2 CHECK(knowledge >= 0 AND knowledge <= 3) NOT NULL DEFAULT 0,
	safety_net BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS tags (
	id INT8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	owner INT8 REFERENCES users ON DELETE CASCADE NOT NULL,
	text TEXT CHECK(text ~ '^[^\n]*$') NOT NULL,
	description TEXT NOT NULL,
	color BYTEA CHECK(octet_length(color) = 3) NOT NULL -- color of the tag in RGB
);

CREATE TABLE IF NOT EXISTS tagged_cards (
	card INT8 REFERENCES cards ON DELETE CASCADE,
	tag INT8 REFERENCES tags ON DELETE CASCADE,
	PRIMARY KEY (card, tag)
);

CREATE TABLE IF NOT EXISTS tagged_tags (
	tag INT8 REFERENCES tags ON DELETE CASCADE,
	supertag INT8 REFERENCES tags ON DELETE CASCADE CHECK(tag != supertag),
	PRIMARY KEY (tag, supertag)
);

CREATE TABLE IF NOT EXISTS session_cookies (
	cookie_value TEXT PRIMARY KEY,
	for_user INT8 REFERENCES users ON DELETE CASCADE NOT NULL
);

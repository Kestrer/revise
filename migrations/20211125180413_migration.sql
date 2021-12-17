CREATE EXTENSION pgcrypto;

CREATE TABLE users (
	id Int8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	email Text UNIQUE CHECK(octet_length(email) != 0) NOT NULL,
	password Text CHECK(octet_length(password) != 0) NOT NULL
);

CREATE TABLE cards (
	id Int8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	owner Int8 REFERENCES users ON DELETE CASCADE NOT NULL,
	created_at Int8 NOT NULL, -- Javascript timestamp
	terms Text CHECK(terms ~ '\S') NOT NULL,
	definitions Text CHECK(definitions ~ '\S') NOT NULL,
	case_sensitive Boolean NOT NULL,
	knowledge Int2 CHECK(knowledge >= 0 AND knowledge <= 3) NOT NULL DEFAULT 0,
	safety_net Boolean NOT NULL DEFAULT FALSE
);

CREATE TABLE tags (
	id Int8 PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
	owner Int8 REFERENCES users ON DELETE CASCADE NOT NULL,
	text Text CHECK(text ~ '^[^\n]*$') NOT NULL,
	description Text NOT NULL,
	color ByteA CHECK(octet_length(color) = 3) NOT NULL -- color of the tag in RGB
);

CREATE TABLE tagged_cards (
	card Int8 REFERENCES cards ON DELETE CASCADE,
	tag Int8 REFERENCES tags ON DELETE CASCADE,
	PRIMARY KEY (card, tag)
);

CREATE TABLE tagged_tags (
	tag Int8 REFERENCES tags ON DELETE CASCADE,
	supertag Int8 REFERENCES tags ON DELETE CASCADE CHECK(tag != supertag),
	PRIMARY KEY (tag, supertag)
);

CREATE TABLE session_cookies (
	cookie_value Text PRIMARY KEY,
	for_user Int8 REFERENCES users ON DELETE CASCADE NOT NULL
);

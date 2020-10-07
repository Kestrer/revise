# revise

Revise is a command line tool to help students revise - like Quizlet, but on the command line.

## Installation

You first need cargo installed. Then type:
```sh
cargo install --git https://github.com/Koxiaet/revise
```

## Usage

Each set is a [RON](https://github.com/ron-rs/ron) file, listing the name and the array of terms.
For example:
```ron
(
	name: "Example Set",
	terms: [
		("First term", "First definition"),
		("Second term", "Second definition"),
	],
)
```

Each term an definition is also a regex, although anchors are not supported. When testing the user
on the terms a random string which matches the regex is used, and any answer which matches the regex
is accepted.

`revise` stores a database of how well you know all the terms you have revised in
`~/.local/share/revise/data.ron` on Linux, `~/Library/Application Support/revise/data.ron` on macOS
and `~\AppData\Roaming\revise\data\data.ron` on Windows. You can edit this to manually tell `revise`
your knowledge of a term, although it isn't formatted.

When revising, `revise` chooses a random term with a knowledge level under 3 from the set you are
revising from, and it tells you to write out your answer.

If you get it right the term will move up a category and if you get it wrong for the second time in
a row it will move down. Once all terms are in the third category the revision session ends.

When a set is opened and all terms are in category 3 they are moved to category 2 to prevent
revision sessions that instantly end.

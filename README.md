# revise

Revise is a command line tool to help students revise - like Quizlet, but on the command line.

## Installation

You first need cargo installed. Then type:
```
cargo install --git https://github.com/Koxiaet/revise
```

## Usage

Each set is a JSON file with the following format:
```
{
	"name": "The set name",
	"terms": {
		"First term": "First definition",
		"Second term": "Second definition"
	}
}
```
Each term and definition is also a regex, so for example: `"water": "[Hh]2[Oo]"`.

Type `revise -h` to get a list of options. Here is a more detailed description of each of the modes:

- Test mode shuffles all the terms and tests them all on you. At the end, you get a list of all your
incorrect terms.
- Rounds mode is like test mode, but at the end of each test you are tested on all the terms you got
wrong in the previous test - like Quizlet's Write mode. It finishes once you get all the terms right.
- In learn mode each term is placed into four different categories, and they all start off in the
first. You are tested on a random term, and depending on which category that term is in the test is
different. If you succeed the test the term moves up a category, if you fail it moves down a
category. It finishes once all the terms are in the top category.

Here is a more detailed description of each of the test types:

- Write mode prompts you with the term regex, and you have to enter either the definition regex or
something that matches the definition regex. The terms and definitions are swapped if you enable the
`--inverted` flag.
- Choose mode prompts you with the term regex, and gives you four (or less if the set is too small)
possible options to choose from. You press 0, 1, 2 or 3 to choose. The terms and definitions are
swapped if you enable the `--inverted` flag.

## License

This is dual-licensed under MIT OR Apache-2.0.

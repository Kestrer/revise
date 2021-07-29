# revise

Revise is a command line tool to help students revise - like Quizlet, but on the command line.

## Installation

You first need cargo installed. Then type:
```sh
cargo install --git https://github.com/Kestrer/revise
```

## Usage

Sets are stored in `.set` files, which look like this:

```
Set name

first term - first definition
second term, alternative term - second definition, alternative definition
# Comments start with a hash
```

When revising a set, you will be prompted with a randomly chosen term and will have to write down
every single definition, in no particular order. Each card (corresponding to one line in a set)
is ranked under 4 levels of knowledge, and starts on the first. Getting it correct moves it up a
level, and getting it wrong for the second time in a row moves it down a level. Once all cards are
in the 4th level, the session ends.

When a set is opened and all terms are in category 4 they are moved to category 3 to prevent
revision sessions that instantly end.

`revise` stores a database of how well you know all the terms you have revised in
`~/.local/share/revise/data.sqlite3` on Linux, `~/Library/Application Support/revise/data.sqlite3`
on macOS and `~\AppData\Roaming\revise\data\data.sqlite3` on Windows.

## Demo

Here is an example revision session using the following set:

```
Example Set

mi - me, my, myself
moku - food, to eat
ale, ali - all
```

![revise demo](./demo.gif)

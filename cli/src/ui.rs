use std::convert::TryFrom;
use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::queue;
use crossterm::{
    cursor,
    terminal::{self, ClearType},
};
use unicode_width::UnicodeWidthStr as _;

use Direction::{Left, Right};

pub(crate) fn read_line(mut out: impl io::Write) -> io::Result<Option<String>> {
    let (start_x, start_y) = cursor::position()?;
    let mut line = String::new();
    let mut position = 0;

    loop {
        let key_event = match read_key()? {
            Some(key) => key,
            None => return Ok(None),
        };
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                line.insert(position, c);
                position += c.len_utf8();
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL) | (KeyCode::Backspace, _) => {
                if next_boundary(Left, &mut position, &line) {
                    line.remove(position);
                }
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                let remove_from = last_word_start(&line[..position]);
                line.replace_range(remove_from..position, "");
                position = remove_from;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                line.clear();
                position = 0;
            }
            (KeyCode::Delete, _) => {
                if position < line.len() {
                    line.remove(position);
                }
            }
            (KeyCode::Left, _) => {
                next_boundary(Left, &mut position, &line);
            }
            (KeyCode::Right, _) => {
                next_boundary(Right, &mut position, &line);
            }
            (KeyCode::Up | KeyCode::Home, _) => position = 0,
            (KeyCode::Down | KeyCode::End, _) => position = line.len(),
            (KeyCode::Enter, _) => break,
            _ => (),
        }

        let (cols, _) = terminal::size()?;

        queue!(
            out,
            cursor::MoveTo(start_x, start_y),
            terminal::Clear(ClearType::FromCursorDown)
        )?;
        out.write_all(line.as_bytes())?;
        let x = start_x + u16::try_from(line[..position].width()).unwrap();
        queue!(out, cursor::MoveTo(x % cols, start_y + x / cols))?;
        out.flush()?;
    }

    queue!(
        out,
        cursor::MoveTo(start_x + u16::try_from(line.width()).unwrap(), start_y)
    )?;
    out.flush()?;

    Ok(Some(line))
}

pub(crate) fn read_key() -> io::Result<Option<KeyEvent>> {
    let key = loop {
        if let Event::Key(key) = event::read()? {
            break key;
        }
    };
    Ok(
        if key.modifiers == KeyModifiers::CONTROL
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q'))
        {
            None
        } else {
            Some(key)
        },
    )
}

fn next_boundary(direction: Direction, position: &mut usize, on: &str) -> bool {
    if *position
        == match direction {
            Direction::Left => 0,
            Direction::Right => on.len(),
        }
    {
        return false;
    }

    loop {
        match direction {
            Direction::Left => *position -= 1,
            Direction::Right => *position += 1,
        }
        if on.is_char_boundary(*position) {
            break true;
        }
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Left,
    Right,
}

fn last_word_start(s: &str) -> usize {
    let end_of_whitespace = s
        .char_indices()
        .rfind(|(_, c)| !c.is_whitespace())
        .map_or(0, |(i, _)| i);

    s[..end_of_whitespace]
        .char_indices()
        .rev()
        .take_while(|(_, c)| !c.is_whitespace())
        .last()
        .map_or(end_of_whitespace, |(i, _)| i)
}

#[test]
fn test_last_word_start() {
    assert_eq!(last_word_start(""), 0);
    assert_eq!(last_word_start("ab"), 0);
    assert_eq!(last_word_start("  "), 0);
    assert_eq!(last_word_start("a "), 0);
    assert_eq!(last_word_start(" a"), 1);
    assert_eq!(last_word_start("hello  world"), 7);
    assert_eq!(last_word_start("hello  world "), 7);
    assert_eq!(last_word_start("hello  world\t \n\t  \t"), 7);
}

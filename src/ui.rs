use std::convert::TryFrom;
use std::fmt::{self, Display, Formatter};
use std::io;

use anyhow::bail;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::queue;
use crossterm::{
    cursor,
    terminal::{self, ClearType},
};
use unicode_width::UnicodeWidthStr as _;

use Direction::{Left, Right};

pub(crate) fn read_line(mut out: impl io::Write) -> anyhow::Result<String> {
    let (start_x, start_y) = cursor::position()?;
    let mut line = String::new();
    let mut position = 0;

    loop {
        let key_event = read_key()?;
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                line.insert(position, c);
                position += c.len_utf8();
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL) | (KeyCode::Backspace, _) => {
                if next_boundary(Left, &mut position, &line) {
                    line.remove(position);
                }
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                let remove_from = line[..position]
                    .char_indices()
                    .rev()
                    .find(|(_, c)| c.is_whitespace())
                    .map_or(0, |(i, _)| i);
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
            (KeyCode::Up, _) | (KeyCode::Home, _) => position = 0,
            (KeyCode::Down, _) | (KeyCode::End, _) => position = line.len(),
            (KeyCode::Enter, _) => break,
            _ => (),
        }

        queue!(
            out,
            cursor::MoveTo(start_x, start_y),
            terminal::Clear(ClearType::UntilNewLine)
        )?;
        out.write_all(line.as_bytes())?;
        queue!(
            out,
            cursor::MoveTo(
                start_x + u16::try_from(line[..position].width()).unwrap(),
                start_y
            )
        )?;
        out.flush()?;
    }

    queue!(
        out,
        cursor::MoveTo(start_x + u16::try_from(line.width()).unwrap(), start_y)
    )?;
    out.flush()?;

    Ok(line)
}

pub(crate) fn read_key() -> anyhow::Result<KeyEvent> {
    Ok(loop {
        if let Event::Key(key) = event::read()? {
            if key.modifiers == KeyModifiers::CONTROL
                && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q'))
            {
                bail!(QuitEarly);
            }
            break key;
        }
    })
}

#[derive(Debug)]
pub(crate) struct QuitEarly;

impl Display for QuitEarly {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("Quit early")
    }
}

impl std::error::Error for QuitEarly {}

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

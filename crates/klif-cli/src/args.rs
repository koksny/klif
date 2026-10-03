//! A small command-line parser: commands take their `--flags` / `--opt value` first, then positionals in order;
//! whatever is left is a usage error.

use crate::out::{CliError, CliResult};
use std::str::FromStr;

pub struct Args {
    items: Vec<String>,
}

impl Args {
    pub fn new(items: Vec<String>) -> Args {
        Args { items }
    }

    /// Remove every `name` (a boolean flag); true when it was there.
    pub fn flag(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|a| a != name);
        self.items.len() != before
    }

    /// Remove `name value` or `name=value`; the last one wins.
    pub fn opt(&mut self, name: &str) -> CliResult<Option<String>> {
        let mut found = None;
        let mut i = 0;
        while i < self.items.len() {
            let a = &self.items[i];
            if a == name {
                if i + 1 >= self.items.len() {
                    return Err(CliError::usage(format!("{name} needs a value.")));
                }
                found = Some(self.items[i + 1].clone());
                self.items.drain(i..=i + 1);
                continue;
            }
            if let Some(v) = a.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
                found = Some(v.to_string());
                self.items.remove(i);
                continue;
            }
            i += 1;
        }
        Ok(found)
    }

    /// `opt` parsed as a number.
    pub fn opt_num<T: FromStr>(&mut self, name: &str) -> CliResult<Option<T>> {
        match self.opt(name)? {
            None => Ok(None),
            Some(v) => v.trim().parse::<T>().map(Some).map_err(|_| CliError::usage(format!("{name} needs a number, not \"{v}\"."))),
        }
    }

    /// The next positional (required).
    pub fn pos(&mut self, what: &str) -> CliResult<String> {
        self.next_pos().ok_or_else(|| CliError::usage(format!("Missing {what}.")))
    }

    /// The next positional, if any.
    pub fn next_pos(&mut self) -> Option<String> {
        let i = self.items.iter().position(|a| !is_option(a))?;
        Some(self.items.remove(i))
    }

    /// Every remaining positional.
    pub fn rest(&mut self) -> Vec<String> {
        let (pos, opts): (Vec<String>, Vec<String>) = std::mem::take(&mut self.items).into_iter().partition(|a| !is_option(a));
        self.items = opts;
        pos
    }

    /// Nothing may be left.
    pub fn done(self) -> CliResult<()> {
        match self.items.first() {
            None => Ok(()),
            Some(a) if is_option(a) => Err(CliError::usage(format!("Unknown option {a}."))),
            Some(a) => Err(CliError::usage(format!("Unexpected argument \"{a}\"."))),
        }
    }
}

/// `--name` (a lone `-` or a negative number is a value).
fn is_option(a: &str) -> bool {
    a.starts_with("--") && a.len() > 2
}

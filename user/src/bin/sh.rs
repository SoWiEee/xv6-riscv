// user/src/bin/sh.rs
//
// A port of the xv6 shell (user/sh.c) to Rust. Supports the full xv6 command
// grammar: exec, I/O redirection (`<`, `>`, `>>`), pipes (`|`), command lists
// (`;`), background jobs (`&`), and parenthesised subshells (`(...)`). The
// parser is a direct recursive-descent port; the C version threads raw `char*`
// cursors, whereas here a `Parser` owns the input slice and a position, and the
// command tree is an owned `Cmd` enum instead of tagged `malloc`'d structs.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use xv6_user_lib::fs::{O_CREATE, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use xv6_user_lib::{print, println, syscall};

/// Parsed command tree. Mirrors the five `*cmd` node types in xv6 sh.c.
enum Cmd {
    /// A program to run: `argv[0]` plus arguments.
    Exec { argv: Vec<String> },
    /// Redirect `fd` to `file` opened with `mode`, then run the inner command.
    Redir {
        cmd: Box<Cmd>,
        file: String,
        mode: i32,
        fd: i32,
    },
    /// `left | right` — left's stdout piped to right's stdin.
    Pipe { left: Box<Cmd>, right: Box<Cmd> },
    /// `left ; right` — run left to completion, then right.
    List { left: Box<Cmd>, right: Box<Cmd> },
    /// `cmd &` — run cmd in the background (parent does not wait).
    Back { cmd: Box<Cmd> },
}

const WHITESPACE: &[u8] = b" \t\r\n\x0b";
const SYMBOLS: &[u8] = b"<|>&;()";

/// Recursive-descent parser over the raw command bytes.
struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { s: s.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && WHITESPACE.contains(&self.s[self.pos]) {
            self.pos += 1;
        }
    }

    /// Return true if the next non-whitespace byte is one of `toks`. Advances
    /// past leading whitespace (matching xv6 `peek`).
    fn peek(&mut self, toks: &[u8]) -> bool {
        self.skip_ws();
        self.pos < self.s.len() && toks.contains(&self.s[self.pos])
    }

    /// Consume one token. Returns its kind and, for word tokens, the word text:
    ///   0    end of input
    ///   b'a' a word (returned in the `String`)
    ///   b'+' the `>>` append operator
    ///   else the literal symbol byte (`<`, `>`, `|`, `&`, `;`, `(`, `)`)
    fn gettoken(&mut self) -> (u8, Option<String>) {
        self.skip_ws();
        if self.pos >= self.s.len() {
            return (0, None);
        }
        let c = self.s[self.pos];
        let ret;
        let mut word = None;
        match c {
            b'|' | b'(' | b')' | b';' | b'&' | b'<' => {
                ret = c;
                self.pos += 1;
            }
            b'>' => {
                self.pos += 1;
                if self.pos < self.s.len() && self.s[self.pos] == b'>' {
                    ret = b'+'; // ">>"
                    self.pos += 1;
                } else {
                    ret = b'>';
                }
            }
            _ => {
                let start = self.pos;
                while self.pos < self.s.len()
                    && !WHITESPACE.contains(&self.s[self.pos])
                    && !SYMBOLS.contains(&self.s[self.pos])
                {
                    self.pos += 1;
                }
                word = Some(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned());
                ret = b'a';
            }
        }
        self.skip_ws();
        (ret, word)
    }

    fn parseline(&mut self) -> Option<Box<Cmd>> {
        let mut cmd = self.parsepipe()?;
        while self.peek(b"&") {
            self.gettoken();
            cmd = Box::new(Cmd::Back { cmd });
        }
        if self.peek(b";") {
            self.gettoken();
            let right = self.parseline()?;
            cmd = Box::new(Cmd::List { left: cmd, right });
        }
        Some(cmd)
    }

    fn parsepipe(&mut self) -> Option<Box<Cmd>> {
        let cmd = self.parseexec()?;
        if self.peek(b"|") {
            self.gettoken();
            let right = self.parsepipe()?;
            return Some(Box::new(Cmd::Pipe { left: cmd, right }));
        }
        Some(cmd)
    }

    /// Collect any leading/trailing redirections into `(op, file)` pairs, where
    /// `op` is `<`, `>` or `+` (`>>`). Returns false on a missing filename.
    fn collect_redirs(&mut self, redirs: &mut Vec<(u8, String)>) -> bool {
        while self.peek(b"<>") {
            let (op, _) = self.gettoken();
            let (tok, file) = self.gettoken();
            if tok != b'a' {
                println!("missing file for redirection");
                return false;
            }
            redirs.push((op, file.unwrap()));
        }
        true
    }

    /// Wrap `cmd` in `Redir` nodes for each collected redirection.
    fn wrap_redirs(cmd: Box<Cmd>, redirs: Vec<(u8, String)>) -> Box<Cmd> {
        let mut c = cmd;
        for (op, file) in redirs {
            let (mode, fd) = match op {
                b'<' => (O_RDONLY, 0),
                b'>' => (O_WRONLY | O_CREATE | O_TRUNC, 1),
                b'+' => (O_WRONLY | O_CREATE, 1), // ">>"
                _ => continue,
            };
            c = Box::new(Cmd::Redir { cmd: c, file, mode, fd });
        }
        c
    }

    fn parseblock(&mut self) -> Option<Box<Cmd>> {
        self.gettoken(); // consume '('
        let cmd = self.parseline()?;
        if !self.peek(b")") {
            println!("syntax - missing )");
            return None;
        }
        self.gettoken(); // consume ')'
        let mut redirs = Vec::new();
        if !self.collect_redirs(&mut redirs) {
            return None;
        }
        Some(Self::wrap_redirs(cmd, redirs))
    }

    fn parseexec(&mut self) -> Option<Box<Cmd>> {
        if self.peek(b"(") {
            return self.parseblock();
        }

        let mut argv: Vec<String> = Vec::new();
        let mut redirs: Vec<(u8, String)> = Vec::new();
        if !self.collect_redirs(&mut redirs) {
            return None;
        }
        while !self.peek(b"|)&;") {
            let (tok, word) = self.gettoken();
            if tok == 0 {
                break;
            }
            if tok != b'a' {
                println!("syntax error");
                return None;
            }
            argv.push(word.unwrap());
            if !self.collect_redirs(&mut redirs) {
                return None;
            }
        }
        Some(Self::wrap_redirs(Box::new(Cmd::Exec { argv }), redirs))
    }
}

fn parsecmd(s: &str) -> Option<Box<Cmd>> {
    let mut p = Parser::new(s);
    let cmd = p.parseline()?;
    p.skip_ws();
    if p.pos != p.s.len() {
        let leftover = String::from_utf8_lossy(&p.s[p.pos..]);
        println!("leftovers: {}", leftover);
        return None;
    }
    Some(cmd)
}

/// Fork, exiting the shell on failure (xv6 `fork1`).
fn fork1() -> isize {
    let pid = syscall::fork();
    if pid == -1 {
        println!("fork failed");
        syscall::exit(1);
    }
    pid
}

/// Build a NUL-terminated `argv` array and `exec` it. Returns only on failure.
fn exec_argv(argv: &[String]) {
    // The kernel reads each argv entry as a C string, so every argument needs a
    // trailing NUL; `owned` keeps those buffers alive until after exec.
    let owned: Vec<Vec<u8>> = argv
        .iter()
        .map(|a| {
            let mut v = Vec::with_capacity(a.len() + 1);
            v.extend_from_slice(a.as_bytes());
            v.push(0);
            v
        })
        .collect();
    let mut c_args: Vec<*const u8> = owned.iter().map(|v| v.as_ptr()).collect();
    c_args.push(core::ptr::null());
    syscall::exec(&argv[0], &c_args);
}

/// Execute a command tree. Never returns — always exits the process.
fn runcmd(cmd: &Cmd) -> ! {
    match cmd {
        Cmd::Exec { argv } => {
            if argv.is_empty() {
                syscall::exit(1);
            }
            exec_argv(argv);
            println!("exec {} failed", argv[0]);
        }
        Cmd::Redir { cmd, file, mode, fd } => {
            syscall::close(*fd);
            if syscall::open(file, *mode) < 0 {
                println!("open {} failed", file);
                syscall::exit(1);
            }
            runcmd(cmd);
        }
        Cmd::List { left, right } => {
            if fork1() == 0 {
                runcmd(left);
            }
            syscall::wait(core::ptr::null_mut());
            runcmd(right);
        }
        Cmd::Pipe { left, right } => {
            let mut p = [0i32; 2];
            if syscall::pipe(&mut p) < 0 {
                println!("pipe failed");
                syscall::exit(1);
            }
            // Left child: stdout -> pipe write end.
            if fork1() == 0 {
                syscall::close(1);
                syscall::dup(p[1]);
                syscall::close(p[0]);
                syscall::close(p[1]);
                runcmd(left);
            }
            // Right child: stdin -> pipe read end.
            if fork1() == 0 {
                syscall::close(0);
                syscall::dup(p[0]);
                syscall::close(p[0]);
                syscall::close(p[1]);
                runcmd(right);
            }
            syscall::close(p[0]);
            syscall::close(p[1]);
            syscall::wait(core::ptr::null_mut());
            syscall::wait(core::ptr::null_mut());
        }
        Cmd::Back { cmd } => {
            if fork1() == 0 {
                runcmd(cmd);
            }
        }
    }
    syscall::exit(0);
}

/// Read one line from stdin into `buf`. Returns false on EOF (empty read),
/// mirroring xv6 `getcmd` returning -1. The kernel console driver echoes typed
/// characters (including the newline), so the shell does not echo.
fn getcmd(buf: &mut String) -> bool {
    print!("$ ");
    buf.clear();
    loop {
        match syscall::getc() {
            Some(b'\n') | Some(b'\r') => break,
            Some(c) => buf.push(c as char),
            None => return false, // EOF
        }
    }
    true
}

#[unsafe(no_mangle)]
fn main() -> ! {
    xv6_user_lib::init_heap();

    // Ensure fds 0, 1, 2 are open on the console. init normally sets these up
    // before exec'ing the shell, but re-opening is cheap and matches xv6.
    loop {
        let fd = syscall::open("console", O_RDWR);
        if fd < 0 {
            break;
        }
        if fd >= 3 {
            syscall::close(fd as i32);
            break;
        }
    }

    let mut line = String::new();
    while getcmd(&mut line) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // `cd` must run in the shell itself, not a child, so the change sticks.
        if let Some(dir) = trimmed.strip_prefix("cd ") {
            let dir = dir.trim();
            if syscall::chdir(dir) < 0 {
                println!("cannot cd {}", dir);
            }
            continue;
        }

        // Everything else runs in a forked child, parsed there as in xv6.
        if fork1() == 0 {
            match parsecmd(trimmed) {
                Some(cmd) => runcmd(&cmd),
                None => syscall::exit(1),
            }
        }
        syscall::wait(core::ptr::null_mut());
    }
    syscall::exit(0);
}

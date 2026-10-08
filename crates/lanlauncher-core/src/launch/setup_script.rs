//! macOS/Linux: a game's ETI scripts, run inside the game's prefix —
//! `game_setup.cmd` once, `game_start.cmd` at each start ([`Script`]).
//!
//! The setup script is what makes an ETI package complete: it writes the CD
//! keys, the install paths and the player's profile into the registry, and
//! starts the package's own helpers (`wc3_keys.exe`, `keygen.exe`, a language
//! selector). Without it a game asks for a key it should have had, or does not
//! find itself. The script comes with the game from the sync server, so the
//! values it writes — keys among them — never have to be copied anywhere else.
//!
//! Wine's own `cmd.exe` runs it, in the prefix and with the runner the game
//! starts with: `set`, `if`, `for`, `%~dp0` and `reg add` behave there as
//! they do on Windows. What a Windows machine needs and a prefix does not, or
//! what would wait for a console nobody sees, is left out line by line; see
//! [`filter`]. The filtered copy sits next to the original
//! ([`Script::filtered_name`]), so `%~dp0` still is the game's folder.

use std::path::Path;

/// Which of a game's ETI scripts runs in the prefix, and the files it gets
/// in the game's folder: the filtered copy, and the one-line batch that
/// runs it with ETI's four arguments ([`wrapper`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// `game_setup.cmd`, once per prefix.
    Setup,
    /// `game_start.cmd` as a whole: the start itself, as on Windows —
    /// language branches, `fnr.exe` writing the player's name, menus.
    Start,
    /// What `game_start.cmd` does before it starts the game
    /// ([`preparation`]), for a game whose profile starts it itself.
    Preparation,
    /// The registry values a profile sets for the player
    /// (`player_settings::write_registry_script`); written by the launcher,
    /// not taken from the package.
    Settings,
}

impl Script {
    pub fn filtered_name(self) -> &'static str {
        match self {
            Script::Setup => ".nll-setup.cmd",
            Script::Start => ".nll-start.cmd",
            Script::Preparation => ".nll-prep.cmd",
            Script::Settings => ".nll-settings.cmd",
        }
    }

    pub fn wrapper_name(self) -> &'static str {
        match self {
            Script::Setup => ".nll-setup-run.cmd",
            Script::Start => ".nll-start-run.cmd",
            Script::Preparation => ".nll-prep-run.cmd",
            Script::Settings => ".nll-settings-run.cmd",
        }
    }

    /// The package's script this one is made from; none for the settings,
    /// which the launcher writes itself.
    fn source(self, paths: &crate::paths::GamePaths) -> Option<&Path> {
        match self {
            Script::Setup => Some(&paths.setup_script),
            Script::Start | Script::Preparation => Some(&paths.start_script),
            Script::Settings => None,
        }
    }
}

/// Why a line of a setup script does not run in a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Windows' firewall (`netsh`): a prefix has none, and the launcher does
    /// not touch the host's.
    Firewall,
    /// `dism`: Windows features. DirectPlay and the like come from the
    /// game's components (winetricks) in a prefix.
    WindowsFeature,
    /// `taskkill`: would end Wine's or Proton's own processes as well.
    EndsProcesses,
    /// `pause`, `timeout`, `choice`: wait for a key in a console nobody sees.
    WaitsForKey,
    /// A Windows tool a prefix does not have (`powershell`, `bcdedit`, …).
    NotInWine(String),
    /// A program outside the game's folder — mostly the old ETI launcher's
    /// helpers (`%programfiles%\eti\lan launcher\unrar.exe`), which a prefix
    /// never has.
    OutsideGame(String),
    /// Not left out: `fnr.exe` runs as the launcher's own find and replace
    /// (`launch::fnr`), the line stays as it was otherwise.
    FindAndReplace,
    /// Not left out: the profile adds arguments to a program the line
    /// starts (`[[script_args]]`).
    Arguments,
    /// A `[[script_args]]` entry whose program the script never starts as a
    /// command; its line is 0.
    ArgumentsUnused,
}

/// Written into a `rem` line that may stand inside an `if` block, so it holds
/// no parenthesis of its own: a program's path is quoted
/// (`%programfiles(x86)%`).
impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::Firewall => f.write_str("Windows firewall"),
            Reason::WindowsFeature => {
                f.write_str("Windows feature, the game's components provide it")
            }
            Reason::EndsProcesses => f.write_str("would end Wine's own processes"),
            Reason::WaitsForKey => f.write_str("waits for a key nobody can press"),
            Reason::NotInWine(tool) => write!(f, "{tool} does not exist in Wine"),
            Reason::OutsideGame(program) => write!(f, "\"{program}\" is outside the game folder"),
            Reason::FindAndReplace => f.write_str("fnr.exe runs as the launcher's own"),
            Reason::Arguments => f.write_str("arguments from the game's profile"),
            Reason::ArgumentsUnused => {
                f.write_str("the profile's arguments found no such program in the script")
            }
        }
    }
}

/// One line of the script that was left out, or that lost a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 1-based, as an editor counts.
    pub line: usize,
    pub text: String,
    pub reason: Reason,
    /// The line has parentheses (an `if`/`for` block, an `else`), so it
    /// stays and only the refused command in it became `ver>nul`, which
    /// keeps the block intact; otherwise the line became a remark.
    pub inline: bool,
}

/// The script as it runs in a prefix, and what was left out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filtered {
    /// Original bytes for every line that stays: a script written in a
    /// Windows code page keeps its umlauts.
    pub bytes: Vec<u8>,
    pub skipped: Vec<Skipped>,
}

/// Tools of a Windows installation that Wine does not have, or must not run.
fn denied(base: &str) -> Option<Reason> {
    Some(match base {
        "netsh" => Reason::Firewall,
        "dism" => Reason::WindowsFeature,
        "taskkill" => Reason::EndsProcesses,
        "pause" | "timeout" | "choice" => Reason::WaitsForKey,
        "powershell" | "pwsh" | "bcdedit" | "shutdown" | "runas" => {
            Reason::NotInWine(base.to_string())
        }
        _ => return None,
    })
}

/// Wine's own tools: wherever a script spells their path, they are there.
const WINE_TOOLS: [&str; 10] = [
    "reg", "regedit", "cmd", "xcopy", "wmic", "find", "findstr", "attrib", "notepad", "start",
];

/// Roots that are not the game's folder. `%~dp0`, `%1` and a variable the
/// script set from them are; a drive letter or a program folder is not.
/// Windows' own folder is no outside either: Wine brings `regsvr32`,
/// `msiexec`, `rundll32` and the rest of `system32` itself, and what it
/// lacks fails harmlessly ([`denied`] takes what must not run at all).
fn outside_root(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if ["%systemroot%", "%windir%"]
        .iter()
        .any(|root| lower.starts_with(root))
    {
        return false;
    }
    let bytes = lower.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return !lower[3..].starts_with("windows\\");
    }
    [
        "%programfiles%",
        "%programfiles(x86)%",
        "%programw6432%",
        "%commonprogramfiles%",
        "%programdata%",
        "%allusersprofile%",
    ]
    .iter()
    .any(|root| lower.starts_with(root))
}

/// The name a program goes by: `C:\Windows\System32\DISM.EXE` is `dism`.
fn base_name(word: &str) -> String {
    let last = word.rsplit(['\\', '/']).next().unwrap_or(word);
    let lower = last.to_ascii_lowercase();
    for ext in [".exe", ".com", ".cmd", ".bat"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    lower
}

/// The program a statement runs, if it runs one: the first word, or after
/// `start` (its switches and a quoted window title passed over) and `call`
/// the word that follows.
fn program_of(statement: &str) -> Option<String> {
    let words = super::windows::tokens(statement);
    let first = words.first()?;
    match first.to_ascii_lowercase().as_str() {
        "start" => {
            // Switches may stand before and after the title; `/d` takes
            // the folder that follows it.
            let mut rest = words[1..].iter();
            let mut title = super::windows::after_start_is_quoted(statement);
            while let Some(word) = rest.next() {
                if word.eq_ignore_ascii_case("/d") {
                    rest.next();
                } else if word.starts_with('/') {
                } else if title {
                    title = false;
                } else {
                    return Some(word.clone());
                }
            }
            None
        }
        "call" => words.get(1).cloned(),
        _ => Some(first.clone()),
    }
}

/// Why `statement` must not run in a prefix, if it must not.
fn verdict(statement: &str) -> Option<Reason> {
    let mut command = super::windows::until_the_next_command(statement).trim();
    // `) else (`, `(netsh …`: the block's own punctuation is not a command.
    loop {
        let rest = command.trim_start_matches(['(', ')', '@']).trim_start();
        let rest = match rest.get(..5) {
            Some(word) if word.eq_ignore_ascii_case("else ") => rest[5..].trim_start(),
            _ => rest,
        };
        if rest.len() == command.len() {
            break;
        }
        command = rest;
    }
    let lower = command.to_ascii_lowercase();
    // `for … do <command>`: the command is what runs.
    if lower.starts_with("for ") {
        return lower
            .find(" do ")
            .and_then(|at| verdict(&command[at + 4..]));
    }
    // `if exist "x.exe" netsh …`: the condition is not a command, what
    // follows it is.
    if lower.starts_with("if ") {
        let then = after_condition(&command[3..])?;
        // `if x (echo ok) else taskkill …`: both branches are commands.
        return match unquoted_else(then) {
            Some((branch, other)) => verdict(branch).or_else(|| verdict(other)),
            None => verdict(then),
        };
    }
    judge(&program_of(command)?)
}

/// `text` split around an ` else ` outside quotes.
fn unquoted_else(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut quoted = false;
    for i in 0..bytes.len() {
        if bytes[i] == b'"' {
            quoted = !quoted;
        } else if !quoted && bytes.len() - i >= 6 && bytes[i..i + 6].eq_ignore_ascii_case(b" else ")
        {
            return Some((&text[..i], &text[i + 6..]));
        }
    }
    None
}

/// What an `if` runs once its condition holds: `[/i] [not]` and then
/// `exist <path>`, `defined <name>`, `errorlevel <n>`, `cmdextversion <n>`
/// or a comparison (`a==b`, `a equ b`). `None` where the condition does not
/// read like one of those; the line then runs as written.
fn after_condition(rest: &str) -> Option<&str> {
    let mut rest = rest.trim_start();
    for flag in ["/i ", "not "] {
        if rest
            .get(..flag.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(flag))
        {
            rest = rest[flag.len()..].trim_start();
        }
    }
    let keyword = rest
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if matches!(
        keyword.as_str(),
        "exist" | "defined" | "errorlevel" | "cmdextversion"
    ) {
        let (_, after) = argument(rest[keyword.len()..].trim_start())?;
        return Some(after);
    }
    // A comparison: one side, the operator, the other side.
    let (_, after) = argument(rest)?;
    let after = after.trim_start();
    let after = if let Some(other) = after.strip_prefix("==") {
        other
    } else {
        let op = after.get(..4)?.to_ascii_lowercase();
        if !["equ ", "neq ", "lss ", "leq ", "gtr ", "geq "].contains(&op.as_str()) {
            return None;
        }
        &after[4..]
    };
    let (_, after) = argument(after.trim_start())?;
    Some(after)
}

/// One argument of a condition and what follows it: a quoted string as a
/// whole, otherwise up to a space or an `=` (`%x%==1` has no spaces).
fn argument(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    let end = if let Some(quoted) = text.strip_prefix('"') {
        quoted.find('"').map(|at| at + 2)?
    } else {
        text.find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(text.len())
    };
    Some((&text[..end], &text[end..]))
}

/// The verdict on one program.
fn judge(program: &str) -> Option<Reason> {
    let base = base_name(program);
    if let Some(reason) = denied(&base) {
        return Some(reason);
    }
    // `start` and `call` on their own say nothing yet; `start notepad` is
    // judged by `notepad`.
    if WINE_TOOLS.contains(&base.as_str()) {
        return None;
    }
    outside_root(program).then(|| Reason::OutsideGame(program.to_string()))
}

/// What cmd shows nowhere but the filtered copy does: the reason and the
/// command, for whoever opens the file.
fn remark_line(out: &mut Vec<u8>, reason: &Reason, command: &[u8], eol: &[u8]) {
    out.extend_from_slice(b"rem [NextGen LAN Launcher] skipped, ");
    out.extend_from_slice(reason.to_string().as_bytes());
    out.extend_from_slice(b": ");
    out.extend_from_slice(command);
    out.extend_from_slice(eol);
}

/// Whether the parenthesis at `i` belongs to a variable's name —
/// `%ProgramFiles(x86)%`, `%CommonProgramFiles(x86)%` — not to a block.
fn in_variable(content: &[u8], i: usize) -> bool {
    let x86 = |from: usize| {
        content
            .get(from..from + 6)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"(x86)%"))
    };
    match content[i] {
        b'(' => x86(i),
        b')' => i >= 4 && x86(i - 4),
        _ => false,
    }
}

/// Where the parentheses of `if`/`for` blocks stand in `content`: outside
/// quotes and not in a variable's name. Positions are byte offsets: quotes
/// and parentheses are ASCII in every code page.
fn block_parentheses(content: &[u8]) -> Vec<usize> {
    let mut quoted = false;
    let mut out = Vec::new();
    for (i, b) in content.iter().enumerate() {
        match b {
            b'"' => quoted = !quoted,
            b'(' | b')' if !quoted && !in_variable(content, i) => out.push(i),
            _ => {}
        }
    }
    out
}

/// The commands of a piece of a line and the operators between them: cmd
/// chains commands with `&`, `&&`, `|` and `||` outside quotes, unless a
/// `^` escapes the character (`'tasklist ^| findstr …'`) or the `&` is part
/// of a redirection (`>nul 2>&1`).
fn commands_and_operators(piece: &[u8]) -> Vec<(&[u8], bool)> {
    let mut out = Vec::new();
    let (mut quoted, mut escaped, mut start) = (false, false, 0);
    for (i, b) in piece.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'^' if !quoted => escaped = true,
            b'"' => quoted = !quoted,
            b'&' | b'|' if !quoted && !(*b == b'&' && i > 0 && b"<>".contains(&piece[i - 1])) => {
                out.push((&piece[start..i], true));
                out.push((&piece[i..=i], false));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((&piece[start..], true));
    out
}

/// The words of one command with where each starts and whether a command
/// may begin there (the first word, or the one after `do` or `else`): a
/// quoted string is one word, anything else ends at a space or at what cmd
/// reads as a separator, a redirection or an escape.
fn command_words(text: &[u8]) -> Vec<(usize, String, bool)> {
    let mut out: Vec<(usize, String, bool)> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if b.is_ascii_whitespace() || b"<>^=,;@".contains(&b) {
            i += 1;
            continue;
        }
        let end = if b == b'"' {
            text[i + 1..]
                .iter()
                .position(|c| *c == b'"')
                .map_or(text.len(), |at| i + 2 + at)
        } else {
            text[i..]
                .iter()
                .position(|c| c.is_ascii_whitespace() || b"\"<>^=,;@".contains(c))
                .map_or(text.len(), |at| i + at)
        };
        let word = String::from_utf8_lossy(&text[i..end]).into_owned();
        let command_position = out.last().is_none_or(|(_, before, _)| {
            before.eq_ignore_ascii_case("do") || before.eq_ignore_ascii_case("else")
        });
        out.push((i, word, command_position));
        i = end;
    }
    out
}

/// Where in one command (no `&`/`|`, no block parenthesis) a refused
/// program begins, and why. A refused tool counts where a command begins —
/// first, or after `do`/`else`: `for … do taskkill`, `)else(pause` — not
/// as an argument (`set choice=1`, `reg add … /v Timeout`, `goto pause`).
/// What `if`, `start` and `call` run is read the way a line is
/// ([`verdict`]); it then goes from the command's first word on, after an
/// `else` or `do` that leads it.
fn refused_in(command: &[u8]) -> Option<(usize, Reason)> {
    let words = command_words(command);
    for (at, word, command_position) in &words {
        if !command_position {
            continue;
        }
        let program = word.trim_matches('"');
        let base = base_name(program);
        if let Some(reason) = denied(&base) {
            return Some((*at, reason));
        }
        if !WINE_TOOLS.contains(&base.as_str()) && outside_root(program) {
            return Some((*at, Reason::OutsideGame(program.to_string())));
        }
    }
    let lead = words
        .iter()
        .find(|(_, word, _)| !word.eq_ignore_ascii_case("do") && !word.eq_ignore_ascii_case("else"))
        .map(|(at, ..)| *at)?;
    let reason = verdict(&String::from_utf8_lossy(&command[lead..]))?;
    Some((lead, reason))
}

/// The first refused command of `piece`, written into `out` as `ver>nul`
/// — a command that does nothing — up to the next operator; the rest of the
/// piece as it was.
fn neutralize_piece(piece: &[u8], out: &mut Vec<u8>, first: &mut Option<Reason>) {
    for (part, is_command) in commands_and_operators(piece) {
        match is_command.then(|| refused_in(part)).flatten() {
            Some((at, reason)) => {
                out.extend_from_slice(&part[..at]);
                out.extend_from_slice(b"ver>nul ");
                first.get_or_insert(reason);
            }
            None => out.extend_from_slice(part),
        }
    }
}

/// Where in `line` a program `wanted` (by its [`base_name`]) begins a
/// command, as byte ranges of the word that names it: in every block and
/// after every operator, never as an argument.
fn command_spans(line: &[u8], wanted: impl Fn(&str) -> bool) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    let parentheses = block_parentheses(line);
    for end in parentheses.iter().copied().chain([line.len()]) {
        let piece = &line[start..end];
        let mut offset = 0;
        for (part, is_command) in commands_and_operators(piece) {
            if is_command {
                for (at, word, command_position) in command_words(part) {
                    // Positions are bytes, the word's length is its text's:
                    // the two agree for ASCII, which every name matched
                    // here is.
                    if command_position
                        && word.is_ascii()
                        && wanted(&base_name(word.trim_matches('"')))
                    {
                        let from = start + offset + at;
                        spans.push((from, from + word.len()));
                    }
                }
            }
            offset += part.len();
        }
        start = end + 1;
    }
    spans
}

/// `line` with every `fnr.exe` where a command begins — the old ETI
/// launcher's copy or one in the game's folder — replaced by
/// `"%NLL_FNR%"` (`launch::fnr`), its arguments as they were. `None` when
/// the line calls none.
fn swap_fnr(line: &[u8]) -> Option<Vec<u8>> {
    let spans = command_spans(line, |name| name == "fnr");
    if spans.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(line.len() + 16);
    let mut at = 0;
    for (from, to) in spans {
        out.extend_from_slice(&line[at..from]);
        out.extend_from_slice(format!("\"%{}%\"", super::fnr::VAR).as_bytes());
        at = to;
    }
    out.extend_from_slice(&line[at..]);
    Some(out)
}

/// `line` with a profile's arguments right after each program they belong
/// to (`wanted`: base name and the arguments as one string), where it
/// begins a command; the line's own arguments follow them. `None` when the
/// line calls none of them; `found` notes which entries matched.
fn add_arguments(line: &[u8], wanted: &[(String, String)], found: &mut [bool]) -> Option<Vec<u8>> {
    let mut inserts = Vec::new();
    for ((name, args), seen) in wanted.iter().zip(found.iter_mut()) {
        for (_, to) in command_spans(line, |base| base == name) {
            inserts.push((to, args.as_str()));
            *seen = true;
        }
    }
    if inserts.is_empty() {
        return None;
    }
    inserts.sort_by_key(|(to, _)| *to);
    let mut out = Vec::with_capacity(line.len() + 32);
    let mut at = 0;
    for (to, args) in inserts {
        out.extend_from_slice(&line[at..to]);
        out.push(b' ');
        out.extend_from_slice(args.as_bytes());
        at = to;
    }
    out.extend_from_slice(&line[at..]);
    Some(out)
}

/// `filtered` with a profile's arguments added ([`add_arguments`]). Each
/// line that got some is noted with [`Reason::Arguments`] — the list is of
/// lines that did not run as written, not only of lines left out — and
/// each entry that matched no line with [`Reason::ArgumentsUnused`] (line
/// 0), so a profile written for another form of the script says so.
fn with_arguments(filtered: Filtered, extra: &[crate::manifest::ScriptArgs]) -> Filtered {
    if extra.is_empty() {
        return filtered;
    }
    let wanted: Vec<(String, String)> = extra
        .iter()
        .map(|e| (base_name(&e.exe), e.args.join(" ")))
        .collect();
    let mut found = vec![false; wanted.len()];
    let Filtered { bytes, mut skipped } = filtered;
    let mut out = Vec::with_capacity(bytes.len());
    for (index, raw) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        match add_arguments(raw, &wanted, &mut found) {
            Some(line) => {
                out.extend_from_slice(&line);
                let text = String::from_utf8_lossy(&line);
                skipped.push(Skipped {
                    line: index + 1,
                    text: text.trim_end_matches(['\r', '\n']).to_string(),
                    reason: Reason::Arguments,
                    inline: true,
                });
            }
            None => out.extend_from_slice(raw),
        }
    }
    for (entry, seen) in extra.iter().zip(&found) {
        if !seen {
            skipped.push(Skipped {
                line: 0,
                text: entry.exe.clone(),
                reason: Reason::ArgumentsUnused,
                inline: true,
            });
        }
    }
    skipped.sort_by_key(|s| s.line);
    Filtered {
        bytes: out,
        skipped,
    }
}

/// `content` with every refused command replaced by `ver>nul`, every block
/// parenthesis where it was: `) else ( taskkill /f /im x & reg add …)`
/// becomes `) else ( ver>nul & reg add …)`. `None` when nothing in it is
/// refused.
fn neutralize(content: &[u8], parentheses: &[usize]) -> Option<(Vec<u8>, Reason)> {
    let mut out = Vec::with_capacity(content.len());
    let mut first = None;
    let mut start = 0;
    for &at in parentheses {
        neutralize_piece(&content[start..at], &mut out, &mut first);
        out.push(content[at]);
        start = at + 1;
    }
    neutralize_piece(&content[start..], &mut out, &mut first);
    first.map(|reason| (out, reason))
}

/// `script` with every refused command taken out: a line that is nothing
/// but that command becomes a remark that says why; in any other line the
/// command becomes `ver>nul` ([`neutralize`]), so no `if` or `for` block
/// loses its shape and no command chained to it (`cd local && …`) is lost.
pub fn filter(script: &[u8]) -> Filtered {
    let script = script.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(script);
    let mut bytes = Vec::with_capacity(script.len());
    let mut skipped = Vec::new();
    for (index, raw) in script.split_inclusive(|b| *b == b'\n').enumerate() {
        let text = String::from_utf8_lossy(raw);
        let line = text.trim_end_matches(['\r', '\n']);
        let command = line.trim_start().trim_start_matches('@').trim_start();
        let lower = command.to_ascii_lowercase();
        if lower.starts_with("rem ") || lower == "rem" || lower.starts_with("::") {
            bytes.extend_from_slice(raw);
            continue;
        }
        let swapped = swap_fnr(raw);
        let raw = swapped.as_deref().unwrap_or(raw);
        if swapped.is_some() {
            skipped.push(Skipped {
                line: index + 1,
                text: line.to_string(),
                reason: Reason::FindAndReplace,
                inline: true,
            });
        }
        let eol: &[u8] = if raw.ends_with(b"\r\n") {
            b"\r\n"
        } else {
            b"\n"
        };
        let content = raw.strip_suffix(b"\n").unwrap_or(raw);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let parentheses = block_parentheses(content);
        let (reason, inline) = if !parentheses.is_empty() {
            match neutralize(content, &parentheses) {
                Some((neutral, reason)) => {
                    bytes.extend_from_slice(&neutral);
                    bytes.extend_from_slice(eol);
                    (reason, true)
                }
                None => {
                    bytes.extend_from_slice(raw);
                    continue;
                }
            }
        } else {
            let commands: Vec<&[u8]> = commands_and_operators(content)
                .into_iter()
                .filter(|(part, is_command)| *is_command && !part.trim_ascii().is_empty())
                .map(|(part, _)| part)
                .collect();
            let reason = commands
                .iter()
                .find_map(|part| refused_in(part).map(|(_, reason)| reason));
            match reason {
                // `cd local && C:\Tools\patch.exe`: the `cd` stays, the lines
                // after it depend on it.
                Some(reason) if commands.len() > 1 => {
                    let mut first = None;
                    neutralize_piece(content, &mut bytes, &mut first);
                    bytes.extend_from_slice(eol);
                    (reason, true)
                }
                Some(reason) => {
                    remark_line(&mut bytes, &reason, content, eol);
                    (reason, false)
                }
                None => {
                    bytes.extend_from_slice(raw);
                    continue;
                }
            }
        };
        skipped.push(Skipped {
            line: index + 1,
            text: line.to_string(),
            reason,
            inline,
        });
    }
    Filtered { bytes, skipped }
}

/// A host path as Wine sees it: drive `Z:` is the root of the file system,
/// in every Wine prefix, Proton's and CrossOver's bottles included.
pub fn wine_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

/// The batch that runs the filtered script with ETI's contract
/// (`"<game_path>" <id> <lang> "<player>"`). Not through `call`: `call`
/// expands `%` a second time and doubles `^`, and a library under
/// `/home/u/LAN 100%` would come out without its `%` (tried with Proton 11).
/// The script's exit code is the wrapper's all the same.
///
/// The paths and the player's name reach it as variables ([`env`]): cmd reads
/// a batch file in the console's code page, and a library under
/// `/home/jürgen` written into the file would come out as `jÃ¼rgen`. Wine
/// hands the environment over in Unicode. The id and the language are ASCII.
pub fn wrapper(game_id: &str, lang: &str) -> String {
    format!(
        "@echo off\r\n\"%{SCRIPT_VAR}%\" \"%{GAME_PATH_VAR}%\" {game_id} {lang} \"%{PLAYER_VAR}%\"\r\n"
    )
}

const SCRIPT_VAR: &str = "NLL_SCRIPT";
const GAME_PATH_VAR: &str = "NLL_GAME_PATH";
const PLAYER_VAR: &str = "NLL_PLAYER";

/// What [`wrapper`] reads, for the runner's environment. The player name is
/// already free of quotes ([`crate::settings::Settings::safe_player_name`]).
pub fn env(script: Script, share_dir: &Path, player: &str) -> [(String, String); 3] {
    [
        (
            SCRIPT_VAR.into(),
            wine_path(&share_dir.join(script.filtered_name())),
        ),
        (GAME_PATH_VAR.into(), wine_path(share_dir)),
        (PLAYER_VAR.into(), player.replace('"', "")),
    ]
}

/// Write the filtered copy of `script` and its wrapper into the game's
/// folder, and say which lines were left out; the runner needs [`env`]
/// besides. `None` when there is nothing to run: no such script, or — for
/// [`Script::Preparation`] — no part before the game's start that can run on
/// its own. `game_exes` names what starts the game ([`game_exes`]); `extra`
/// is what the profile adds to the start script's programs.
pub fn prepare(
    paths: &crate::paths::GamePaths,
    script: Script,
    game_id: &str,
    lang: &str,
    game_exes: &[String],
    extra: &[crate::manifest::ScriptArgs],
) -> std::io::Result<Option<Vec<Skipped>>> {
    let Some(source) = script.source(paths) else {
        return Ok(None);
    };
    let text = match std::fs::read(source) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let text = match script {
        Script::Preparation => match preparation(&text, game_exes) {
            Some(part) => part,
            None => return Ok(None),
        },
        Script::Setup | Script::Start | Script::Settings => text,
    };
    let filtered = match script {
        Script::Start => with_arguments(filter(&text), extra),
        Script::Setup | Script::Preparation | Script::Settings => filter(&text),
    };
    write_if_changed(
        &paths.share_dir.join(script.filtered_name()),
        &filtered.bytes,
    )?;
    write_if_changed(
        &paths.share_dir.join(script.wrapper_name()),
        wrapper(game_id, lang).as_bytes(),
    )?;
    Ok(Some(filtered.skipped))
}

/// cmd reads a batch file from disk as it goes, by offset: a start that is
/// still running the same file (the game started twice) must not find it
/// rewritten under it. The same text is not written again.
pub(crate) fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes)
}

/// What starts the game, by name: the programs the start script lets
/// through the firewall — ETI's scripts name the game's executables there,
/// launchers included — and the profile's own executable. Helpers the
/// script runs before (`fnr.exe`, a language selector) are not among them.
pub fn game_exes(start_script: &str, profile_exe: Option<&str>) -> Vec<String> {
    use crate::script_probe::{Confidence, ScriptProbe};
    let mut names: Vec<String> = ScriptProbe::analyse(start_script)
        .candidates
        .into_iter()
        .filter(|c| c.confidence >= Confidence::Medium)
        .map(|c| base_name(&c.exe))
        .collect();
    if let Some(exe) = profile_exe.filter(|e| !e.is_empty()) {
        names.push(base_name(exe));
    }
    names.sort();
    names.dedup();
    names
}

/// The part of a start script before the line that starts the game: what
/// the script prepares — the player's name in a config file, the language
/// in the registry — for a profile that starts the game itself. `None` when
/// no line starts one of `game_exes`, or when the part before it cannot run
/// on its own: a question (`set /p`), a jump (`goto`, a `:label`), or a
/// block the game's line stands in.
pub fn preparation(script: &[u8], game_exes: &[String]) -> Option<Vec<u8>> {
    let script = script.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(script);
    let mut depth = 0i64;
    let mut offset = 0;
    for raw in script.split_inclusive(|b| *b == b'\n') {
        let content = raw.strip_suffix(b"\n").unwrap_or(raw);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let text = String::from_utf8_lossy(content);
        let command = text.trim_start().trim_start_matches('@').trim_start();
        let lower = command.to_ascii_lowercase();
        let remark = lower.starts_with("rem ") || lower == "rem" || lower.starts_with("::");
        if !remark {
            if lower.starts_with(':') || lower.contains("set /p") || jumps(content) {
                return None;
            }
            if starts_one_of(content, game_exes) {
                return (depth == 0).then(|| script[..offset].to_vec());
            }
            for at in block_parentheses(content) {
                depth += if content[at] == b'(' { 1 } else { -1 };
            }
        }
        offset += raw.len();
    }
    None
}

/// A `goto` anywhere a word stands — also behind an `if` condition
/// (`if exist x goto skip`) and as `goto:label`.
fn jumps(content: &[u8]) -> bool {
    command_words(content).iter().any(|(_, word, _)| {
        let lower = word.to_ascii_lowercase();
        lower == "goto" || lower.starts_with("goto:")
    })
}

/// Whether a command of `content` runs one of `names`: where a command
/// begins, or as what `start`/`call` run.
fn starts_one_of(content: &[u8], names: &[String]) -> bool {
    let is_game = |program: &str| names.contains(&base_name(program.trim_matches('"')));
    commands_and_operators(content)
        .into_iter()
        .filter(|(_, is_command)| *is_command)
        .any(|(part, _)| {
            command_words(part)
                .iter()
                .any(|(_, word, command_position)| *command_position && is_game(word))
                || program_of(&String::from_utf8_lossy(part)).is_some_and(|p| is_game(&p))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(exe: &str, words: &[&str]) -> crate::manifest::ScriptArgs {
        crate::manifest::ScriptArgs {
            exe: exe.into(),
            args: words.iter().map(|w| w.to_string()).collect(),
        }
    }

    #[test]
    fn a_profile_adds_arguments_where_the_script_starts_the_program() {
        // AoE II's menu: each choice starts one program from its folder.
        let script = b"set /P wahl=Auswahl:  \r\n\
            if /i \"%wahl%\"==\"2\" goto:Classic\r\n\
            :Classic\r\n\
            cd local\r\n\
            \"empires2.exe\"\r\n\
            AGE2_X1.EXE -multiple & echo done\r\n\
            if exist empires2.exe (start \"\" /wait empires2.exe)\r\n\
            echo empires2.exe\r\n\
            \"AoK HD.exe\"\r\n";
        let extra = [
            args("empires2.exe", &["NOSTARTUP"]),
            args("age2_x1", &["NOSTARTUP"]),
        ];
        let out = with_arguments(filter(script), &extra);
        let text = String::from_utf8(out.bytes).unwrap();
        assert!(text.contains("\"empires2.exe\" NOSTARTUP\r\n"), "{text}");
        assert!(
            text.contains("AGE2_X1.EXE NOSTARTUP -multiple & echo done"),
            "{text}"
        );
        // An argument (`if exist`, `echo`) is not a start — and neither, for
        // now, is what `start` runs: documented on `ScriptArgs::exe`.
        assert!(text.contains("echo empires2.exe\r\n"), "{text}");
        assert!(
            text.contains("(start \"\" /wait empires2.exe)\r\n"),
            "{text}"
        );
        assert!(text.contains("\"AoK HD.exe\"\r\n"), "{text}");
        let noted: Vec<usize> = out
            .skipped
            .iter()
            .filter(|s| s.reason == Reason::Arguments)
            .map(|s| s.line)
            .collect();
        assert_eq!(noted, [5, 6]);
        assert!(!out
            .skipped
            .iter()
            .any(|s| s.reason == Reason::ArgumentsUnused));

        // An entry the script never starts says so.
        let out = with_arguments(filter(script), &[args("age2_x2.exe", &["NOSTARTUP"])]);
        assert!(out
            .skipped
            .iter()
            .any(|s| s.reason == Reason::ArgumentsUnused && s.text == "age2_x2.exe"));
    }

    #[test]
    fn nothing_changes_without_arguments_in_the_profile() {
        let script = b"\"empires2.exe\"\r\n";
        assert_eq!(with_arguments(filter(script), &[]).bytes, script.to_vec());
    }

    #[test]
    fn profile_arguments_are_plain_words_only() {
        assert!(args("empires2.exe", &["NOSTARTUP"]).check().is_ok());
        assert!(args("AoK HD.exe", &["-w", "x=1"]).check().is_ok());
        for bad in ["a&b", "%x%", "!x!", "(x)", "a|b", "\"x\"", ""] {
            assert!(args("empires2.exe", &[bad]).check().is_err(), "{bad}");
        }
        assert!(args("..\\x.exe", &["A"]).check().is_err());
        assert!(args("empires2.exe", &[]).check().is_err());
    }

    fn text(filtered: &Filtered) -> String {
        String::from_utf8(filtered.bytes.clone()).unwrap()
    }

    #[test]
    fn registry_keys_paths_and_the_packages_own_helpers_run() {
        // The shapes of ETI's setup scripts, with made-up values.
        let script = "set game_path=%1\r\n\
            echo off\r\n\
            cls\r\n\
            setlocal EnableDelayedExpansion\r\n\
            cd /d \"%~dp0\"\r\n\
            \"demo_profile.exe\"\r\n\
            \"demo_keys.exe\"\r\n\
            cd local\r\n\
            \"Language Selector.exe\"\r\n\
            start \"\" notepad \"key.txt\"\r\n\
            reg.exe add \"HKLM\\SOFTWARE\\WOW6432Node\\Demo\" /v \"CDKEY\" /t REG_SZ /d \"AAAA-BBBB-CCCC\" /f\r\n\
            reg add %regpath% /v \"Path\" /t REG_EXPAND_SZ /d dpwsockx.dll /f /reg:32\r\n\
            regedit /s \"directplay-win64.reg\"\r\n\
            %windir%\\system32\\reg.exe add \"HKCU\\Software\\Demo\" /v x /d 1 /f\r\n\
            mkdir \"%userprofile%\\documents\\My Games\\Demo\"\r\n\
            echo width = %h% > \"%userprofile%\\documents\\My Games\\Demo\\demo.ini\"\r\n\
            for /f %%I in ('wmic.exe path Win32_VideoController get CurrentHorizontalResolution') do set \"h=%%I\"\r\n\
            %windir%\\SysWOW64\\regsvr32.exe /s \"%~dp0local\\dpwsockx.dll\"\r\n\
            C:\\Windows\\System32\\msiexec.exe /i \"%~dp0redist.msi\" /qn\r\n\
            if exist \"%ProgramFiles(x86)%\\Steam\\steam.exe\" set steam=1\r\n\
            if not exist \"C:\\Windows\\SysWOW64\\explorer.exe\" set reg=Wow6432Node\r\n\
            if errorlevel 1 goto pause\r\n\
            if \"%x%\"==\"1\" echo ok\r\n\
            if /i %game_lang% equ de set lang=german\r\n\
            set choice=1\r\n\
            reg add HKCU\\Software\\Game /v \"Timeout\" /t REG_DWORD /d 30 /f\r\n\
            reg add HKCU\\Software\\Game /v Shutdown /d 0 /f\r\n\
            echo Press a key to pause\r\n\
            if exist x (goto pause)\r\n\
            exit /b 0\r\n";
        let filtered = filter(script.as_bytes());
        assert_eq!(filtered.skipped, vec![]);
        assert_eq!(text(&filtered), script);
    }

    #[test]
    fn what_a_prefix_has_no_use_for_becomes_a_remark() {
        let script = "cd /d \"%~dp0\"\r\n\
            netsh advfirewall firewall add rule name=\"demo\" dir=in action=allow program=\"%~dp0local\\demo.exe\"\r\n\
            taskkill /f /im \"Demo.exe\"\r\n\
            timeout 5\r\n\
            @pause\r\n\
            \"%programfiles%\\eti\\lan launcher\\unrar.exe\" x -o -y \"demo_profile.eti\" %demopath%\r\n\
            cd local && C:\\Tools\\patch.exe\r\n\
            reg add \"HKCU\\Software\\Demo\" /v Done /d 1 /f\r\n\
            if exist \"%~dp0x\" \"%programfiles%\\eti\\lan launcher\\fnr.exe\" --x\r\n\
            if \"%a%\"==\"b\" taskkill /im x.exe\r\n\
            if exist x (echo ok) else pause\r\n";
        let filtered = filter(script.as_bytes());
        let reasons: Vec<(usize, &Reason)> = filtered
            .skipped
            .iter()
            .map(|s| (s.line, &s.reason))
            .collect();
        assert_eq!(
            reasons,
            vec![
                (2, &Reason::Firewall),
                (3, &Reason::EndsProcesses),
                (4, &Reason::WaitsForKey),
                (5, &Reason::WaitsForKey),
                (
                    6,
                    &Reason::OutsideGame("%programfiles%\\eti\\lan launcher\\unrar.exe".into())
                ),
                (7, &Reason::OutsideGame("C:\\Tools\\patch.exe".into())),
                (
                    9,
                    &Reason::OutsideGame("%programfiles%\\eti\\lan launcher\\fnr.exe".into())
                ),
                (10, &Reason::EndsProcesses),
                (11, &Reason::WaitsForKey),
            ]
        );
        let out = text(&filtered);
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lines[0], "cd /d \"%~dp0\"");
        assert!(lines[1].starts_with("rem [NextGen LAN Launcher] skipped, Windows firewall: netsh"));
        assert!(lines[4].ends_with(": @pause"));
        assert_eq!(lines[7], "reg add \"HKCU\\Software\\Demo\" /v Done /d 1 /f");
        let inline: Vec<usize> = filtered
            .skipped
            .iter()
            .filter(|s| s.inline)
            .map(|s| s.line)
            .collect();
        assert_eq!(inline, vec![7, 11]);
        // The `cd` the next lines depend on stays.
        assert_eq!(lines[6], "cd local && ver>nul ");
        assert_eq!(lines[10], "if exist x (echo ok) else ver>nul ");
        // A remark may stand inside a block: no parenthesis of its own.
        for s in &filtered.skipped {
            let reason = s.reason.to_string();
            let unquoted: String = reason.split('"').step_by(2).collect();
            assert!(!unquoted.contains(['(', ')']), "{reason}");
        }
    }

    #[test]
    fn in_a_line_with_parentheses_only_the_command_goes() {
        // Star Wars Racer: DISM in both branches of an IF block, each on a
        // line of its own, becomes a remark — fine inside a block. Where the
        // refused command shares its line with the block's parentheses, it
        // becomes `ver>nul` and every parenthesis stays where it was, in
        // whatever shape cmd allows.
        let script = "IF NOT DEFINED PROCESSOR_ARCHITEW6432 (\r\n\
            \t%SYSTEMROOT%\\system32\\DISM.EXE /Online /enable-feature /FeatureName:\"DirectPlay\" /all /NoRestart\r\n\
            ) ELSE (\r\n\
            \t%SYSTEMROOT%\\sysNative\\DISM.EXE /Online /enable-feature /FeatureName:\"DirectPlay\" /all /NoRestart\r\n\
            )\r\n\
            if exist x ( netsh firewall set opmode disable )\r\n\
            for %%p in (game.exe launcher.exe) do taskkill /f /im %%p\r\n\
            if exist y ( netsh advfirewall set allprofiles state off\r\n\
            ) else ( pause\r\n\
            timeout 5 )\r\n\
            if exist z (for %%a in (1) do pause\r\n\
            )else(taskkill /f /im game.exe\r\n\
            )\r\n\
            for /f \"tokens=2\" %%a in ('tasklist ^| findstr /i game.exe') do taskkill /f /pid %%a\r\n\
            if exist x (echo ok>log.txt) else pause\r\n\
            if exist y (start \"Setup\" /wait \"%programfiles%\\eti\\lan launcher\\unrar.exe\" x a.eti)\r\n\
            if exist x (taskkill /im a.exe & reg add HKCU\\K /v k /d 1 /f)\r\n\
            if exist y (cd local & start \"\" \"%programfiles%\\eti\\x.exe\")\r\n\
            if exist x (\r\n\
            echo x\r\n\
            ) else if exist y taskkill /im a.exe\r\n\
            %ProgramFiles(x86)%\\eti\\lan launcher\\unrar.exe x demo.eti %p%\r\n\
            if exist x (taskkill /f /im game.exe >nul 2>&1)\r\n";
        let filtered = filter(script.as_bytes());
        let summary: Vec<(usize, bool)> = filtered
            .skipped
            .iter()
            .map(|s| (s.line, s.inline))
            .collect();
        assert_eq!(
            summary,
            vec![
                (2, false),
                (4, false),
                (6, true),
                (7, true),
                (8, true),
                (9, true),
                (10, true),
                (11, true),
                (12, true),
                (14, true),
                (15, true),
                (16, true),
                (17, true),
                (18, true),
                (21, true),
                (22, false),
                (23, true),
            ]
        );
        let out = text(&filtered);
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert!(lines[1].starts_with("rem [NextGen LAN Launcher] skipped, Windows feature"));
        assert_eq!(lines[2], ") ELSE (");
        assert_eq!(
            &lines[5..],
            [
                "if exist x ( ver>nul )",
                "for %%p in (game.exe launcher.exe) do ver>nul ",
                "if exist y ( ver>nul ",
                ") else ( ver>nul ",
                "ver>nul )",
                "if exist z (for %%a in (1) do ver>nul ",
                ")else(ver>nul ",
                ")",
                "for /f \"tokens=2\" %%a in ('tasklist ^| findstr /i game.exe') do ver>nul ",
                "if exist x (echo ok>log.txt) else ver>nul ",
                "if exist y (ver>nul )",
                // The command after it stays, and so does the one before.
                "if exist x (ver>nul & reg add HKCU\\K /v k /d 1 /f)",
                "if exist y (cd local & ver>nul )",
                "if exist x (",
                "echo x",
                ") else ver>nul ",
                // Not a block: the parentheses belong to the variable.
                "rem [NextGen LAN Launcher] skipped, \"%ProgramFiles(x86)%\\eti\\lan\" is outside the game folder: %ProgramFiles(x86)%\\eti\\lan launcher\\unrar.exe x demo.eti %p%",
                // `2>&1` is a redirection, not a second command.
                "if exist x (ver>nul )",
                "",
            ]
        );
    }

    #[test]
    fn fnr_runs_as_the_launchers_own_wherever_it_was_called_from() {
        let script = "if %game_lang% == de (\r\n\
            \t\"%programfiles%\\eti\\lan launcher\\fnr.exe\" --cl --silent --dir \"%cd%\" --fileMask \"Nadeo.ini\" --useRegEx --find \"Language=.*\" --replace \"Language=de\"\r\n\
            )\r\n\
            \"fnr.exe\" --cl --silent --dir \"%cd%\\System\" --fileMask \"Demo.ini\" --find \"Name=.*\" --replace \"Name=%player%\"\r\n\
            cd local && fnr --cl --find a --replace b\r\n\
            echo fnr.exe is a tool\r\n";
        let filtered = filter(script.as_bytes());
        let out = text(&filtered);
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(
            lines[1],
            "\t\"%NLL_FNR%\" --cl --silent --dir \"%cd%\" --fileMask \"Nadeo.ini\" --useRegEx --find \"Language=.*\" --replace \"Language=de\""
        );
        assert!(lines[3].starts_with("\"%NLL_FNR%\" --cl --silent --dir \"%cd%\\System\""));
        assert_eq!(
            lines[4],
            "cd local && \"%NLL_FNR%\" --cl --find a --replace b"
        );
        assert_eq!(lines[5], "echo fnr.exe is a tool");
        assert!(filtered
            .skipped
            .iter()
            .all(|s| s.reason == Reason::FindAndReplace));
        assert_eq!(
            filtered.skipped.iter().map(|s| s.line).collect::<Vec<_>>(),
            [2, 4, 5]
        );
    }

    #[test]
    fn a_byte_order_mark_goes_and_a_code_page_stays() {
        // cmd reads the BOM as part of the first command ("\u{feff}echo" is
        // not a command); Windows-1252 bytes are not UTF-8 and must survive.
        let mut script = b"\xEF\xBB\xBFecho off\r\n".to_vec();
        script.extend_from_slice(b"set dir=\"%userprofile%\\Dokumente\\Sp\xFCle\"\r\nnetsh x\r\n");
        let filtered = filter(&script);
        assert!(filtered.bytes.starts_with(b"echo off\r\n"));
        assert!(filtered.bytes.windows(5).any(|w| w == b"Sp\xFCle"));
        assert_eq!(filtered.skipped.len(), 1);
    }

    #[test]
    fn the_wrapper_is_ascii_and_the_paths_come_as_wine_paths() {
        assert_eq!(
            wrapper("wc3", "de"),
            "@echo off\r\n\"%NLL_SCRIPT%\" \"%NLL_GAME_PATH%\" wc3 de \"%NLL_PLAYER%\"\r\n"
        );
        let env = env(
            Script::Start,
            Path::new("/home/jürgen/LAN/wc3"),
            "Jürgen \"K\"",
        );
        assert_eq!(
            env,
            [
                (
                    "NLL_SCRIPT".to_string(),
                    "Z:\\home\\jürgen\\LAN\\wc3\\.nll-start.cmd".to_string()
                ),
                (
                    "NLL_GAME_PATH".to_string(),
                    "Z:\\home\\jürgen\\LAN\\wc3".to_string()
                ),
                ("NLL_PLAYER".to_string(), "Jürgen K".to_string()),
            ]
        );
    }

    #[test]
    fn prepare_writes_both_files_next_to_the_script() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::GamePaths::new(dir.path(), "demo");
        assert_eq!(
            prepare(&paths, Script::Setup, "demo", "de", &[], &[]).unwrap(),
            None
        );
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        std::fs::write(&paths.setup_script, "cd /d \"%~dp0\"\r\ntimeout 5\r\n").unwrap();
        let skipped = prepare(&paths, Script::Setup, "demo", "de", &[], &[])
            .unwrap()
            .unwrap();
        assert_eq!(skipped.len(), 1);
        let copy =
            std::fs::read_to_string(paths.share_dir.join(Script::Setup.filtered_name())).unwrap();
        assert!(copy.starts_with("cd /d \"%~dp0\"\r\nrem "));
        assert!(
            std::fs::read_to_string(paths.share_dir.join(Script::Setup.wrapper_name()))
                .unwrap()
                .contains("%NLL_SCRIPT%")
        );
    }

    /// ETI's start template: header, language branches, `fnr.exe` writing
    /// the player's name, the firewall rule naming the game, the game, the
    /// rule removed again (Unreal Tournament 2004's shape, made-up values).
    const UT_SHAPE: &str = "set game_path=%1\r\n\
        set game_id=%2\r\n\
        set game_lang=%3\r\n\
        set player=%4\r\n\
        echo off\r\n\
        cd /d \"%~dp0\"\r\n\
        cd local\r\n\
        if %game_lang% == de (\r\n\
        \t\"fnr.exe\" --cl --silent --dir \"%cd%\\System\" --fileMask \"Demo.ini\" --find \"Language=.*\" --replace \"Language=det\"\r\n\
        )\r\n\
        \"fnr.exe\" --cl --silent --dir \"%cd%\\System\" --fileMask \"Demo.ini\" --find \"Name=.*\" --replace \"Name=%player%\"\r\n\
        netsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\system\\Demo.exe\" profile=any enable=yes >nul\r\n\
        cd system\r\n\
        \"Demo.exe\"\r\n\
        :end\r\n\
        netsh advfirewall firewall delete rule name=\"%game_id%\" >nul\r\n\
        exit\r\n";

    #[test]
    fn the_preparation_is_what_comes_before_the_game() {
        let names = game_exes(UT_SHAPE, None);
        assert_eq!(names, vec!["demo".to_string()]);
        let part = String::from_utf8(preparation(UT_SHAPE.as_bytes(), &names).unwrap()).unwrap();
        assert!(part.contains("--replace \"Name=%player%\""));
        assert!(part.ends_with("cd system\r\n"));
        assert!(!part.contains("\"Demo.exe\""));
        // The profile's own executable counts as the game, too.
        assert_eq!(
            game_exes(UT_SHAPE, Some("system/Other.exe")),
            vec!["demo".to_string(), "other".to_string()]
        );
    }

    #[test]
    fn no_preparation_where_the_part_before_the_game_cannot_run_alone() {
        let names = vec!["game".to_string()];
        let menu = "echo 1. HD\r\nset /P wahl=Auswahl: \r\nif /i \"%wahl%\"==\"1\" goto:HD\r\n:HD\r\n\"game.exe\"\r\n";
        assert_eq!(preparation(menu.as_bytes(), &names), None);
        let jump = "if exist x goto skip\r\nreg add HKCU\\X /v a /d 1 /f\r\n\"game.exe\"\r\n";
        assert_eq!(preparation(jump.as_bytes(), &names), None);
        let in_block = "if %game_lang% == de (\r\n\"game.exe\" -de\r\n)\r\n";
        assert_eq!(preparation(in_block.as_bytes(), &names), None);
        let elsewhere = "\"other.exe\"\r\n";
        assert_eq!(preparation(elsewhere.as_bytes(), &names), None);
        // Through `start` it is the game as well.
        let started = "reg add HKCU\\X /v a /d 1 /f\r\nstart \"\" /wait \"game.exe\"\r\n";
        assert_eq!(
            preparation(started.as_bytes(), &names).as_deref(),
            Some(&b"reg add HKCU\\X /v a /d 1 /f\r\n"[..])
        );
    }

    fn copy_has_fnr(paths: &crate::paths::GamePaths) -> bool {
        std::fs::read_to_string(paths.share_dir.join(".nll-prep.cmd"))
            .unwrap()
            .contains("\"%NLL_FNR%\" --cl")
    }

    #[test]
    fn prepare_writes_the_preparation_only_where_there_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::GamePaths::new(dir.path(), "demo");
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        std::fs::write(&paths.start_script, UT_SHAPE).unwrap();
        let names = game_exes(UT_SHAPE, None);
        let skipped = prepare(&paths, Script::Preparation, "demo", "de", &names, &[])
            .unwrap()
            .unwrap();
        // fnr for language and name, and the firewall rule before the game.
        assert_eq!(
            skipped.iter().map(|s| &s.reason).collect::<Vec<_>>(),
            [
                &Reason::FindAndReplace,
                &Reason::FindAndReplace,
                &Reason::Firewall
            ]
        );
        assert!(copy_has_fnr(&paths));
        let copy = std::fs::read_to_string(paths.share_dir.join(".nll-prep.cmd")).unwrap();
        assert!(copy.ends_with("cd system\r\n"));
        assert_eq!(
            prepare(
                &paths,
                Script::Preparation,
                "demo",
                "de",
                &["x".into()],
                &[]
            )
            .unwrap(),
            None
        );
    }
}

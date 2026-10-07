//! macOS/Linux: the part of `fnr.exe` ("Find and Replace") that ETI's
//! scripts use, as a host program.
//!
//! The start scripts write the language and the player's name into a game's
//! config files with it (`--find "Name=.*" --replace "Name=%player%"`).
//! Most call the copy that came with the original ETI launcher,
//! `%programfiles%\eti\lan launcher\fnr.exe`, which no prefix has — and a
//! .NET program besides. `setup_script::filter` turns those calls into
//! `"%NLL_FNR%" …`; Wine starts a host program named by its `Z:` path and
//! waits for it (tried with Proton 11), so this script does the replacing.
//!
//! What it understands is what the 23 calls in ETI's scripts use: `--dir`,
//! `--fileMask` (comma-separated, `*`/`?`), `--includeSubDirectories`,
//! `--find`, `--replace`, `--useRegEx`, `--useEscapeChars`,
//! `--caseSensitive`; `--cl` and `--silent` change nothing here. As in fnr,
//! matching ignores case unless asked not to, `^`/`$` match at each line,
//! and `.` does not cross a line break (it does take a `\r`, as .NET's does).

use std::path::{Path, PathBuf};

/// The variable the rewritten calls name (`"%NLL_FNR%" --cl …`): this
/// script's `Z:` path, set by the launcher on every script it runs.
pub const VAR: &str = "NLL_FNR";

/// Started by Wine with the host's environment as Proton changed it
/// (`LD_LIBRARY_PATH` names Proton's libraries): no business of Perl's. `$0`
/// is the path through the prefix's `dosdevices/z:`, a valid one.
const SHELL: &str = "#!/bin/sh\n\
# Written by NextGen LAN Launcher: fnr.exe's find and replace for Wine.\n\
exec env -u LD_LIBRARY_PATH -u LD_PRELOAD perl \"${0%.sh}.pl\" \"$@\"\n";

const PERL: &str = r#"# Written by NextGen LAN Launcher: fnr.exe's find and replace for Wine.
use strict;
use warnings;
use File::Find;
use Encode qw(decode encode);

my %o;
while (@ARGV) {
    my $arg = shift @ARGV;
    if ($arg =~ /^--(dir|fileMask|find|replace)$/i) {
        $o{lc $1} = shift @ARGV // "";
    } elsif ($arg =~ /^--(\w+)$/) {
        $o{lc $1} = 1;
    }
}
defined $o{find} or die "fnr: --find is missing\n";

# Z:\home\x is /home/x; another drive is the prefix's.
sub host_path {
    my $p = shift;
    if ($p =~ /^[zZ]:(.*)$/) {
        $p = $1;
    } elsif ($p =~ /^([a-yA-Y]):(.*)$/) {
        my $prefix = $ENV{WINEPREFIX}
            // (defined $ENV{STEAM_COMPAT_DATA_PATH} ? "$ENV{STEAM_COMPAT_DATA_PATH}/pfx" : "$ENV{HOME}/.wine");
        $p = "$prefix/dosdevices/" . lc($1) . ":$2";
    }
    $p =~ s{\\}{/}g;
    return $p;
}

sub unescape {
    my $s = shift;
    $s =~ s/\\(.)/$1 eq "n" ? "\n" : $1 eq "t" ? "\t" : $1 eq "r" ? "\r" : $1/ge;
    return $s;
}

# Wine hands the arguments over as UTF-8. The folder stays bytes, as the
# names `readdir` returns are: decoded, a name with an umlaut would no
# longer be found.
my $dir = host_path($o{dir} // ".");
my $find = decode("UTF-8", $o{find});
my $replace = decode("UTF-8", $o{replace} // "");
$replace = unescape($replace) if $o{useescapechars};
$find = unescape($find) if $o{useescapechars} && !$o{useregex};
$find = quotemeta($find) unless $o{useregex};
my $re = $o{casesensitive} ? qr/$find/m : qr/$find/mi;

# `*.*` is everything in Windows, a name without a dot included.
my @masks = map {
    $_ eq "*.*" ? qr/^/ :
    do {
        my $m = quotemeta($_);
        $m =~ s/\\\*/.*/g;
        $m =~ s/\\\?/./g;
        qr/^$m$/i;
    }
} grep { length } split /\s*[,;]\s*/, ($o{filemask} // "*.*");

# A file is written back in the encoding it came in, as fnr.exe does: a
# byte order mark says it, valid UTF-8 beyond ASCII says UTF-8, and
# everything else is what these games were written for, Windows' ANSI.
sub encoding_of {
    my $bytes = shift;
    return ("UTF-16LE", "\xFF\xFE") if $bytes =~ /^\xFF\xFE/;
    return ("UTF-16BE", "\xFE\xFF") if $bytes =~ /^\xFE\xFF/;
    return ("UTF-8", "\xEF\xBB\xBF") if $bytes =~ /^\xEF\xBB\xBF/;
    my $copy = $bytes;
    return ("UTF-8", "") if $bytes =~ /[\x80-\xFF]/ && utf8::decode($copy);
    return ("cp1252", "");
}

# .NET's replacement syntax: $0 the match, $1.. a group, ${name} a named
# one, $$ a dollar.
sub expand {
    my ($with, $whole, $groups, $named) = @_;
    $with =~ s/\$(?:(\d+)|\{(\w+)\}|(\$))/
        defined $3 ? "\$"
        : defined $1 ? ($1 == 0 ? $whole : ($groups->[$1 - 1] \/\/ ""))
        : ($2 =~ \/^\d+$\/ ? ($2 == 0 ? $whole : ($groups->[$2 - 1] \/\/ "")) : ($named->{$2} \/\/ ""))
    /ge;
    return $with;
}

sub replace_in {
    my $file = shift;
    my $name = $file;
    $name =~ s{.*/}{};
    return unless grep { $name =~ $_ } @masks;
    open(my $in, "<:raw", $file) or return;
    local $/;
    my $bytes = <$in>;
    close $in;
    my ($encoding, $bom) = encoding_of($bytes);
    my $text = decode($encoding, substr($bytes, length $bom));
    my $count = 0;
    $text =~ s{$re}{
        $count++;
        expand($replace, $&, [@{^CAPTURE}], {%+})
    }ge;
    return unless $count;
    open(my $out, ">:raw", $file) or die "fnr: cannot write $file: $!\n";
    print $out $bom . encode($encoding, $text);
    close $out;
    print "$file: $count\n" unless $o{silent};
}

if ($o{includesubdirectories}) {
    find({ wanted => sub { replace_in($File::Find::name) if -f }, no_chdir => 1 }, $dir);
} else {
    opendir(my $dh, $dir) or die "fnr: no folder $dir\n";
    for my $entry (readdir $dh) {
        replace_in("$dir/$entry") if -f "$dir/$entry";
    }
    closedir $dh;
}
exit 0;
"#;

/// Write the script into `dir` (once; an unchanged copy stays, a running
/// script may be reading it) and return what to start: the shell part, with
/// the Perl part next to it. The `.sh` is not decoration: cmd looks a name
/// without an extension up as `.com`/`.exe`/`.bat`/`.cmd` and finds nothing,
/// while one with any other extension it hands to Wine, which runs the host
/// program (both tried with Proton 11).
pub fn install(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let shell = dir.join("nll-fnr.sh");
    for (path, text) in [(&shell, SHELL), (&dir.join("nll-fnr.pl"), PERL)] {
        if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
            std::fs::write(path, text)?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(shell)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fnr(dir: &Path, args: &[&str]) -> std::process::Output {
        let tools = dir.join("tools");
        let script = install(&tools).unwrap();
        std::process::Command::new(script)
            .args(args)
            .output()
            .unwrap()
    }

    fn has_perl() -> bool {
        std::process::Command::new("perl")
            .arg("-v")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn the_calls_etis_scripts_make_replace_what_they_name() {
        if !has_perl() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let system = tmp.path().join("game/System");
        std::fs::create_dir_all(&system).unwrap();
        let ini = system.join("Demo.ini");
        std::fs::write(
            &ini,
            "[Engine]\r\nLanguage=int\r\nname=Player\r\nOther=1\r\n",
        )
        .unwrap();
        std::fs::write(system.join("Keep.txt"), "Language=int\r\n").unwrap();
        // As cmd hands it over: a Wine path, the mask, the regex.
        let dir = format!("Z:{}", system.to_string_lossy().replace('/', "\\"));
        let common = [
            "--cl",
            "--silent",
            "--dir",
            &dir,
            "--fileMask",
            "Demo.ini",
            "--useRegEx",
            "--useEscapeChars",
        ];
        let run = |find: &str, replace: &str| {
            let mut args = common.to_vec();
            args.extend(["--find", find, "--replace", replace]);
            let out = fnr(tmp.path(), &args);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run("Language=.*", "Language=det");
        run("Name=.*", "Name=Jürgen K");
        assert_eq!(
            std::fs::read(&ini).unwrap(),
            // Ignoring case as fnr does; `.*` takes the `\r` as .NET's does;
            // an ASCII file is ANSI, so is the name written into it.
            b"[Engine]\r\nLanguage=det\nName=J\xFCrgen K\nOther=1\r\n"
        );
        assert_eq!(
            std::fs::read_to_string(system.join("Keep.txt")).unwrap(),
            "Language=int\r\n"
        );
    }

    #[test]
    fn a_file_keeps_its_encoding_and_a_name_its_umlaut() {
        if !has_perl() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("game");
        std::fs::create_dir_all(&dir).unwrap();
        // ANSI with an umlaut already in it, plain ASCII, UTF-16 with a BOM.
        std::fs::write(dir.join("ansi.ini"), b"Map=Gr\xFCn\r\nName=x\r\n").unwrap();
        std::fs::write(dir.join("ascii.ini"), b"Name=x\r\n").unwrap();
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Name=x\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(dir.join("wide.ini"), &utf16).unwrap();
        let wine_dir = format!("Z:{}", dir.to_string_lossy().replace('/', "\\"));
        let out = fnr(
            tmp.path(),
            &[
                "--cl",
                "--silent",
                "--dir",
                &wine_dir,
                "--fileMask",
                "*.ini",
                "--useRegEx",
                "--find",
                "Name=[^\\r\\n]*",
                "--replace",
                "Name=Jürgen ($0) $$5",
            ],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            std::fs::read(dir.join("ansi.ini")).unwrap(),
            b"Map=Gr\xFCn\r\nName=J\xFCrgen (Name=x) $5\r\n"
        );
        assert_eq!(
            std::fs::read(dir.join("ascii.ini")).unwrap(),
            b"Name=J\xFCrgen (Name=x) $5\r\n"
        );
        let wide = std::fs::read(dir.join("wide.ini")).unwrap();
        let expected: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "Name=Jürgen (Name=x) $5\r\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(wide, expected);
    }

    #[test]
    fn names_with_umlauts_and_without_an_extension_are_found() {
        if !has_perl() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Spiele für jürgen");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["Einstellungen ä.ini", "config"] {
            std::fs::write(dir.join(name), "Name=x\n").unwrap();
        }
        let wine_dir = format!("Z:{}", dir.to_string_lossy().replace('/', "\\"));
        // No --fileMask: fnr's `*.*`, which takes `config` as well.
        let out = fnr(
            tmp.path(),
            &[
                "--cl",
                "--silent",
                "--dir",
                &wine_dir,
                "--useRegEx",
                "--find",
                "Name=.*",
                "--replace",
                "Name=y",
            ],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for name in ["Einstellungen ä.ini", "config"] {
            assert_eq!(
                std::fs::read(dir.join(name)).unwrap(),
                b"Name=y\n",
                "{name}"
            );
        }
    }

    #[test]
    fn subfolders_masks_and_plain_text_work_as_in_fnr() {
        if !has_perl() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("game");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/x.cfg"), "seta name \"x\"\nLocaleXX\n").unwrap();
        std::fs::write(root.join("a/y.xml"), "<LanguageTAG>en</LanguageTag>\n").unwrap();
        let dir = format!("Z:{}", root.to_string_lossy().replace('/', "\\"));
        let out = fnr(
            tmp.path(),
            &[
                "--cl",
                "--silent",
                "--dir",
                &dir,
                "--fileMask",
                "*.cfg, *.xml",
                "--includeSubDirectories",
                "--find",
                "LocaleXX",
                "--replace",
                "Locale.de",
            ],
        );
        assert!(out.status.success());
        let out = fnr(
            tmp.path(),
            &[
                "--cl",
                "--dir",
                &dir,
                "--fileMask",
                "*.xml",
                "--includeSubDirectories",
                "--useRegEx",
                "--find",
                "<LanguageTAG>.*</LanguageTag>",
                "--replace",
                "<LanguageTag>de</LanguageTag>",
            ],
        );
        assert!(out.status.success());
        assert_eq!(
            std::fs::read_to_string(root.join("a/b/x.cfg")).unwrap(),
            "seta name \"x\"\nLocale.de\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a/y.xml")).unwrap(),
            "<LanguageTag>de</LanguageTag>\n"
        );
    }
}

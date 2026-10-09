#!/bin/sh
# Bundled cabextract with its libmspack (see tools/fetch-winetricks.mjs).
#
# Extraction into a folder goes through a scratch folder inside the target,
# and the files are moved into place only if cabextract succeeded. In a
# Proton prefix every builtin DLL in system32/syswow64 is a symlink into the
# Proton install; cabextract opens the destination and writes through the
# link — into Proton's own file (read-only there, so the verb fails:
# `directplay` stopped at dplaysvr.exe). `mv` replaces the link itself, as
# winetricks' own w_try_cp_dll does.
here=$(dirname "$0")
run() {
    LD_LIBRARY_PATH="$here/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$here/cabextract.bin" "$@"
}

# The target folder in every spelling getopt accepts: -d DIR, -dDIR,
# -qd DIR, --directory DIR, --directory=DIR. Listing and testing write
# nothing.
dest=""
want=""
for arg in "$@"; do
    if [ -n "$want" ]; then
        dest=$arg
        want=""
        continue
    fi
    case "$arg" in
        -l | --list | -t | --test | -p | --pipe) dest=""; break ;;
        --directory=*) dest=${arg#--directory=} ;;
        --directory) want=1 ;;
        --*) ;;
        -*d*)
            rest=${arg#*d}
            if [ -n "$rest" ]; then dest=$rest; else want=1; fi
            ;;
    esac
done
if [ -z "$dest" ] || [ ! -d "$dest" ]; then
    run "$@"
    exit $?
fi
dest=$(cd "$dest" && pwd) || exit 1

# A scratch folder left by a run that was killed (only one winetricks run
# works on a prefix at a time).
rm -rf "$dest"/.nll-cabextract.*
scratch=$(mktemp -d "$dest/.nll-cabextract.XXXXXX") || exit 1
trap 'rm -rf "$scratch"' EXIT
trap 'exit 1' HUP INT TERM

# The same arguments with the folder swapped; everything else stays.
want=""
first=1
for arg in "$@"; do
    if [ $first = 1 ]; then
        set --
        first=0
    fi
    if [ -n "$want" ]; then
        set -- "$@" "$scratch"
        want=""
        continue
    fi
    case "$arg" in
        --directory=*) set -- "$@" "--directory=$scratch" ;;
        --directory) set -- "$@" "$arg"; want=1 ;;
        --*) set -- "$@" "$arg" ;;
        -*d)
            set -- "$@" "$arg"
            want=1
            ;;
        -*d*) set -- "$@" "${arg%%d*}d" "$scratch" ;;
        *) set -- "$@" "$arg" ;;
    esac
done
run "$@" || exit $?

cd "$scratch" || exit 1
find . ! -type d -exec sh -c '
    dest=$1
    shift
    for file; do
        mkdir -p "$dest/${file%/*}" && mv -f "$file" "$dest/$file" || exit 1
    done
' sh "$dest" {} + || exit 1
# Empty folders from the archive.
find . -type d -exec sh -c '
    dest=$1
    shift
    for dir; do mkdir -p "$dest/$dir" || exit 1; done
' sh "$dest" {} + || exit 1
exit 0

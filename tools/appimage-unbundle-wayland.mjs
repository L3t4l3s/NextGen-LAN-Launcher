#!/usr/bin/env node
// Take the Wayland libraries back out of a built AppImage.
//
// A built image was measured: `libEGL`, `libGL`, `libgbm` and `libdrm` are
// correctly left out and come from the machine, but all four Wayland
// libraries are packed — `libwayland-client.so.0` (which the AppImage exclude
// list, pkg2appimage/excludelist, names explicitly; Tauri ships its own
// linuxdeploy build, and that one is the only entry of the whole list it gets
// wrong) together with `libwayland-server.so.0`, `libwayland-egl.so.1` and
// `libwayland-cursor.so.0`, which the list does not mention and
// `libwebkit2gtk-4.1` and `libgdk-3` pull in.
//
// So the image supplies the entire Wayland set while Mesa comes from the
// machine, and `LD_LIBRARY_PATH` puts the image first. Mesa's `libEGL_mesa`
// links `libwayland-client` and `libwayland-server` and is built against the
// machine's versions; where the packed ones are older it cannot be loaded at
// all, and then *every* `eglGetDisplay` fails with EGL_BAD_PARAMETER —
// whatever `EGL_PLATFORM` says, and whether or not `LIBGL_ALWAYS_SOFTWARE` is
// set. That is exactly what a Steam Deck reports: a Wayland session, an X11
// window, `EGL_PLATFORM=x11`, software GL, and still no display.
//
// All four go, so that the Wayland set and the Mesa that uses it come from
// the same machine. Tauri's AppImage bundler is a prebuilt binary with no hook
// between linuxdeploy and the finished image, so the image is unpacked, the
// files are removed, and it is packed again.
//
// Usage: node tools/appimage-unbundle-wayland.mjs <file.AppImage> [...]
//        node tools/appimage-unbundle-wayland.mjs <directory>

import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, basename } from "node:path";

/** The Wayland set: it belongs to the machine, next to the machine's Mesa. */
const UNBUNDLE = [
  /^libwayland-client\.so/,
  /^libwayland-server\.so/,
  /^libwayland-egl\.so/,
  /^libwayland-cursor\.so/,
];

/** Below this many removals something has changed in the bundler and the
 * result would ship with the mismatch this script exists to prevent. */
const EXPECTED = 4;

const APPIMAGETOOL =
  "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage";

function run(cmd, args, cwd, env) {
  return execFileSync(cmd, args, {
    cwd,
    env: env ?? process.env,
    stdio: ["ignore", "pipe", "pipe"],
  }).toString();
}

/** appimagetool, downloaded once into the system temp folder. */
function appimagetool() {
  const local = join(tmpdir(), "appimagetool-x86_64.AppImage");
  if (!existsSync(local)) {
    console.log("fetching appimagetool");
    // Into a part file first: a transfer that dies half-way would otherwise
    // leave something at the final name that every later run takes for a
    // finished download and tries to execute.
    const part = `${local}.part`;
    rmSync(part, { force: true });
    run("curl", ["-sSfL", "-o", part, APPIMAGETOOL]);
    renameSync(part, local);
  }
  // Outside the branch: a copy from an earlier run has to be executable too.
  chmodSync(local, 0o755);
  return local;
}

function targets(argv) {
  const found = [];
  for (const arg of argv) {
    const path = resolve(arg);
    if (!existsSync(path)) throw new Error(`no such path: ${path}`);
    if (statSync(path).isDirectory()) {
      for (const name of readdirSync(path)) {
        if (name.endsWith(".AppImage")) found.push(join(path, name));
      }
    } else {
      found.push(path);
    }
  }
  return found;
}

function unbundle(image) {
  const work = mkdtempSync(join(tmpdir(), "nll-appimage-"));
  try {
    chmodSync(image, 0o755);
    // `--appimage-extract` needs no FUSE, which a CI runner may not have.
    run(image, ["--appimage-extract"], work);
    const root = join(work, "squashfs-root");
    const libs = join(root, "usr", "lib");
    const removed = [];
    for (const name of existsSync(libs) ? readdirSync(libs) : []) {
      if (UNBUNDLE.some((re) => re.test(name))) {
        rmSync(join(libs, name), { recursive: true, force: true });
        removed.push(name);
      }
    }
    // Silence would mean the next AppImage quietly goes out with the
    // mismatch again, so a bundler that stops packing these has to be noticed.
    if (removed.length < EXPECTED) {
      throw new Error(
        `${basename(image)}: expected ${EXPECTED} Wayland libraries to remove, found ` +
          `${removed.length} (${removed.join(", ") || "none"}). The bundler's library ` +
          `set has changed — check whether the mismatch is still there before ` +
          `lowering this.`,
      );
    }
    const rebuilt = join(work, basename(image));
    // ARCH is what appimagetool asks for when it cannot guess it.
    run(appimagetool(), ["--appimage-extract-and-run", root, rebuilt], work, {
      ...process.env,
      ARCH: "x86_64",
    });
    // Copy rather than rename: the work folder is in the system temp
    // directory, which is a mount of its own often enough (a tmpfs `/tmp`, a
    // `TMPDIR` elsewhere) for a rename across it to fail with EXDEV — the
    // same reason `tools/fetch-resilio.mjs` copies.
    copyFileSync(rebuilt, image);
    chmodSync(image, 0o755);
    console.log(`${basename(image)}: removed ${removed.join(", ")}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error("usage: appimage-unbundle-wayland.mjs <file.AppImage|directory> [...]");
  process.exit(2);
}
const images = targets(args);
if (images.length === 0) {
  console.error("no .AppImage found in the given paths");
  process.exit(1);
}
for (const image of images) unbundle(image);

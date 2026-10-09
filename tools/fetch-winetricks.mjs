// Puts the pinned winetricks script — and on Linux the cabextract it cannot
// do without — into a folder the installers bundle. Used by release.yml and
// the full CI matrix, next to fetch-resilio.mjs.
//
//   node tools/fetch-winetricks.mjs <windows|osx|linux-x64|linux-arm64> <dest-dir>
//
// winetricks is one shell script, pinned by version and SHA-256 in
// winetricks.lock.json; a mismatch is fatal. Windows needs neither.
//
// On Linux the runner's own cabextract (apt package `cabextract`) is copied
// with the libmspack it links against, behind a small wrapper that points
// the loader at that library. SteamOS ships no cabextract, and winetricks
// gives up without it for every installer packed as .cab — DirectX, the
// Visual C++ runtimes. The runner's glibc is older than any distribution the
// AppImage targets, so the binary runs on the newer ones.
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import path from "node:path";

const [platform, dest] = process.argv.slice(2);
if (!platform || !dest) {
  console.error("usage: fetch-winetricks.mjs <platform> <dest-dir>");
  process.exit(2);
}
if (platform === "windows") {
  console.log("windows: nothing to fetch (games start through game_start.cmd)");
  process.exit(0);
}
const lock = JSON.parse(readFileSync(new URL("../winetricks.lock.json", import.meta.url), "utf8"));
mkdirSync(dest, { recursive: true });

const response = await fetch(lock.url);
if (!response.ok) {
  console.error(`download of ${lock.url} failed: HTTP ${response.status}`);
  process.exit(1);
}
const script = Buffer.from(await response.arrayBuffer());
const sha256 = createHash("sha256").update(script).digest("hex");
if (sha256 !== lock.sha256) {
  console.error(`winetricks ${lock.version}: sha256 ${sha256} does not match the pinned ${lock.sha256}`);
  process.exit(1);
}
const scriptPath = path.join(dest, "winetricks");
writeFileSync(scriptPath, script);
chmodSync(scriptPath, 0o755);
console.log(`winetricks ${lock.version} -> ${scriptPath}`);

if (platform.startsWith("linux")) {
  let cabextract = "";
  try {
    cabextract = execSync("command -v cabextract", { shell: "/bin/sh" }).toString().trim();
  } catch {
    // `command -v` fails when there is none; the message below says what to do.
  }
  if (!cabextract) {
    console.error("cabextract is not installed on this runner (apt-get install cabextract)");
    process.exit(1);
  }
  // The one library it needs beyond libc, wherever the runner keeps it.
  const ldd = execSync(`ldd ${JSON.stringify(cabextract)}`).toString();
  const mspack = ldd.match(/libmspack\.so\.0 => (\S+)/)?.[1];
  if (!mspack) {
    console.error(`cabextract does not link libmspack.so.0 as expected:\n${ldd}`);
    process.exit(1);
  }
  const bin = path.join(dest, "bin");
  mkdirSync(path.join(bin, "lib"), { recursive: true });
  copyFileSync(cabextract, path.join(bin, "cabextract.bin"));
  chmodSync(path.join(bin, "cabextract.bin"), 0o755);
  copyFileSync(mspack, path.join(bin, "lib", "libmspack.so.0"));
  const wrapper = path.join(bin, "cabextract");
  // The wrapper also keeps cabextract from writing through the symlinks of
  // a Proton prefix (see the script).
  copyFileSync(new URL("cabextract-wrapper.sh", import.meta.url), wrapper);
  chmodSync(wrapper, 0o755);
  const version = execSync(`${JSON.stringify(wrapper)} --version`).toString().trim();
  console.log(`${version} (+ ${path.basename(mspack)}) -> ${bin}`);
}

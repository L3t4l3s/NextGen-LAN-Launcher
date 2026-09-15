// Downloads the official Resilio Sync binary for a platform into a folder and
// verifies it against resilio.lock.json.
//
//   node tools/fetch-resilio.mjs <windows|osx|linux-x64|linux-arm64> <dest-dir> [--allow-unpinned]
//
// Release builds must be reproducible: without a pinned SHA-256 the script
// refuses to continue and a hash mismatch is fatal. `--allow-unpinned` exists
// only for the resilio-lock workflow that (re)computes the hashes; there a
// mismatch is reported as a warning so a new Resilio build can be re-pinned.
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import path from "node:path";

const args = process.argv.slice(2);
const allowUnpinned = args.includes("--allow-unpinned");
const [platform, dest] = args.filter((a) => !a.startsWith("--"));
if (!platform || !dest) {
  console.error("usage: fetch-resilio.mjs <platform> <dest-dir> [--allow-unpinned]");
  process.exit(2);
}
const lock = JSON.parse(readFileSync(new URL("../resilio.lock.json", import.meta.url), "utf8"));
const artifact = lock.artifacts[platform];
if (!artifact) {
  console.error(`unknown platform ${platform}; known: ${Object.keys(lock.artifacts).join(", ")}`);
  process.exit(2);
}
if (!artifact.sha256 && !allowUnpinned) {
  console.error(
    `resilio.lock.json has no sha256 for ${platform}. Run the "Resilio lock" workflow ` +
      `(.github/workflows/resilio-lock.yml), copy its output into resilio.lock.json and commit, ` +
      `or pass --allow-unpinned (never for release builds).`,
  );
  process.exit(1);
}

mkdirSync(dest, { recursive: true });
console.log(`downloading ${artifact.url}`);
const res = await fetch(artifact.url);
if (!res.ok) throw new Error(`HTTP ${res.status} for ${artifact.url}`);
const buf = Buffer.from(await res.arrayBuffer());
const sha = createHash("sha256").update(buf).digest("hex");
console.log(`sha256 ${sha} (${buf.length} bytes)`);
if (artifact.sha256 && artifact.sha256 !== sha) {
  if (!allowUnpinned) {
    console.error(`checksum mismatch: expected ${artifact.sha256}, got ${sha}`);
    process.exit(1);
  }
  console.warn(`warning: pinned sha256 ${artifact.sha256} differs from download; Resilio published a new build`);
}
const file = path.join(dest, path.basename(new URL(artifact.url).pathname));
writeFileSync(file, buf);

if (platform.startsWith("linux")) {
  execSync(`tar -xzf "${file}" -C "${dest}" rslsync`, { stdio: "inherit" });
  execSync(`chmod +x "${path.join(dest, "rslsync")}"`);
} else if (platform === "osx") {
  const mount = execSync(`hdiutil attach -nobrowse -readonly "${file}" | tail -1 | awk '{print $NF}'`).toString().trim();
  execSync(`cp -R "${mount}/Resilio Sync.app" "${dest}/"`, { stdio: "inherit" });
  execSync(`hdiutil detach "${mount}"`);
} else {
  // Windows: keep the installer. When no system-wide Resilio is found the
  // launcher runs it silently (`/S /D=<data>/resilio`, see
  // transport::resilio::install_bundled_windows). Untested on real hardware.
}
// Machine-readable summary for the lock workflow.
console.log(`::lock:: ${JSON.stringify({ platform, url: artifact.url, sha256: sha, bytes: buf.length, file })}`);
console.log(`done -> ${dest}`);

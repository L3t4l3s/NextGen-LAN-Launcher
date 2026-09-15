// Downloads the official Resilio Sync binary for a platform into a folder,
// verifying the SHA-256 from resilio.lock.json when one is pinned.
//   node tools/fetch-resilio.mjs <windows|osx|linux-x64|linux-arm64> <dest-dir>
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import path from "node:path";

const [platform, dest] = process.argv.slice(2);
if (!platform || !dest) {
  console.error("usage: fetch-resilio.mjs <platform> <dest-dir>");
  process.exit(2);
}
const lock = JSON.parse(readFileSync(new URL("../resilio.lock.json", import.meta.url), "utf8"));
const artifact = lock.artifacts[platform];
if (!artifact) {
  console.error(`unknown platform ${platform}`);
  process.exit(2);
}
mkdirSync(dest, { recursive: true });
console.log(`downloading ${artifact.url}`);
const res = await fetch(artifact.url);
if (!res.ok) throw new Error(`HTTP ${res.status}`);
const buf = Buffer.from(await res.arrayBuffer());
const sha = createHash("sha256").update(buf).digest("hex");
console.log(`sha256 ${sha}`);
if (artifact.sha256 && artifact.sha256 !== sha) {
  console.error(`checksum mismatch: expected ${artifact.sha256}`);
  process.exit(1);
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
console.log(`done -> ${dest}`);

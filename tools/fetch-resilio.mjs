// Downloads the official Resilio Sync binary for a platform into a folder and
// verifies it against resilio.lock.json. Used by release.yml and the full CI
// matrix so every installer ships the pinned build.
//
//   node tools/fetch-resilio.mjs <windows|osx|linux-x64|linux-arm64> <dest-dir> [--allow-unpinned] [--version=<x.y.z.build>]
//
// `--version` (lock workflow only) downloads a fixed build instead of the
// current `stable` release: Resilio serves old builds under the same path
// layout, e.g. https://download-cdn.resilio.com/2.8.1.1390/linux/x64/0/…
//
// Release builds must be reproducible: without a pinned SHA-256 the script
// refuses to continue and a hash mismatch is fatal. `--allow-unpinned` exists
// only for the resilio-lock workflow that (re)computes the hashes; there a
// mismatch is reported as a warning so a new Resilio build can be re-pinned.
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import { execSync } from "node:child_process";
import path from "node:path";

const args = process.argv.slice(2);
const allowUnpinned = args.includes("--allow-unpinned");
const version = args.find((a) => a.startsWith("--version="))?.slice("--version=".length) ?? "";
const [platform, dest] = args.filter((a) => !a.startsWith("--"));
if (!platform || !dest) {
  console.error("usage: fetch-resilio.mjs <platform> <dest-dir> [--allow-unpinned] [--version=<x.y.z.build>]");
  process.exit(2);
}
if (version && !allowUnpinned) {
  console.error("--version only makes sense together with --allow-unpinned; release builds use the pinned url from resilio.lock.json");
  process.exit(2);
}
if (version && !/^[0-9]+(\.[0-9]+){1,3}$/.test(version)) {
  console.error(`--version must look like 2.8.1.1390, got ${JSON.stringify(version)}`);
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
// Resilio's CDN answers 404 to unknown user agents and has moved paths between
// major versions, so send a browser-like UA and try candidates in order while
// no URL is pinned.
const headers = { "user-agent": "Mozilla/5.0 (X11; Linux x86_64) NextGen-LAN-Launcher-release/1.0", accept: "*/*" };
let urls = artifact.url ? [artifact.url] : (artifact.candidates ?? []);
if (allowUnpinned) {
  // Re-locking: try the pinned url and every candidate with the release segment
  // (`stable` or an old build number) swapped for the requested build. URLs
  // without such a segment are dropped so a request for an old build can never
  // silently fall back to the current release.
  const segment = /\/(stable|[0-9]+(?:\.[0-9]+){1,3})\//;
  const target = version || "stable";
  urls = [...new Set([artifact.url, ...(artifact.candidates ?? [])].filter((u) => u && segment.test(u)).map((u) => u.replace(segment, `/${target}/`)))];
}
if (urls.length === 0) {
  console.error(`no url or candidates for ${platform} in resilio.lock.json`);
  process.exit(2);
}
let buf = null;
let chosen = null;
for (const url of urls) {
  console.log(`downloading ${url}`);
  const res = await fetch(url, { headers, redirect: "follow" });
  console.log(`  -> HTTP ${res.status} ${res.headers.get("content-type") ?? ""} ${res.headers.get("content-length") ?? ""}`);
  if (res.ok) {
    buf = Buffer.from(await res.arrayBuffer());
    chosen = url;
    break;
  }
}
if (!buf) {
  console.error(`no candidate URL for ${platform} answered successfully`);
  process.exit(1);
}
artifact.url = chosen;
const sha = createHash("sha256").update(buf).digest("hex");
console.log(`sha256 ${sha} (${buf.length} bytes)`);
if (artifact.sha256 && artifact.sha256 !== sha) {
  if (!allowUnpinned) {
    console.error(`checksum mismatch: expected ${artifact.sha256}, got ${sha}`);
    process.exit(1);
  }
  console.warn(
    `warning: pinned sha256 ${artifact.sha256} differs from download; ` +
      (version ? `a different build (${version}) was requested` : "Resilio published a new build"),
  );
}
// The download lands in a temp dir; `dest` is shipped as a whole by
// bundle.resources, so only the runnable binary, named as `install` in
// resilio.lock.json (the field transport::resilio::bundled_install_name reads
// too), may ever appear there.
if (!artifact.install) {
  console.error(`resilio.lock.json: artifacts.${platform}.install is missing`);
  process.exit(1);
}
const installed = path.join(dest, artifact.install);
const tmp = mkdtempSync(path.join(os.tmpdir(), "resilio-fetch-"));
const download = path.join(tmp, path.basename(new URL(artifact.url).pathname));
writeFileSync(download, buf);
try {
  // Leftovers from earlier layouts (the archive next to the binary) or an
  // older pin would ship with the bundle too; only README.md may stay.
  for (const entry of readdirSync(dest)) {
    if (entry === "README.md") continue;
    console.log(`removing stale ${path.join(dest, entry)}`);
    rmSync(path.join(dest, entry), { recursive: true, force: true });
  }
  if (platform.startsWith("linux")) {
    const flags = download.endsWith(".gz") ? "-xzf" : "-xf";
    execSync(`tar ${flags} "${download}" -C "${tmp}" rslsync`, { stdio: "inherit" });
    copyFileSync(path.join(tmp, "rslsync"), installed);
    chmodSync(installed, 0o755);
  } else if (platform === "osx") {
    // The volume is called "Resilio Sync" (with a space), so take the whole
    // mount-point column instead of the last whitespace-separated token.
    const attach = execSync(`hdiutil attach -nobrowse -readonly "${download}"`).toString();
    const mount = attach.split("\n").map((l) => l.match(/(\/Volumes\/.*)$/)?.[1]?.trim()).find(Boolean);
    if (!mount) throw new Error(`could not determine DMG mount point:\n${attach}`);
    try {
      execSync(`cp -R "${mount}/Resilio Sync.app" "${installed}"`, { stdio: "inherit" });
    } finally {
      execSync(`hdiutil detach "${mount}"`);
    }
  } else {
    // Windows: the download is the program itself, not an installer (Resilio
    // documents `/noinstall` for running it in place).
    if (path.extname(download).toLowerCase() !== ".exe") {
      throw new Error(`windows artifact must be the .exe program, got ${path.basename(download)} (an .msi cannot run in place)`);
    }
    copyFileSync(download, installed);
  }
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
const file = installed;
// Machine-readable summary for the lock workflow.
console.log(`::lock:: ${JSON.stringify({ platform, url: artifact.url, sha256: sha, bytes: buf.length, file })}`);
console.log(`done -> ${dest}`);

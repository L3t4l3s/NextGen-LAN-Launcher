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
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
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
let file = path.join(dest, path.basename(new URL(artifact.url).pathname));
writeFileSync(file, buf);

if (platform.startsWith("linux")) {
  const flags = file.endsWith(".gz") ? "-xzf" : "-xf";
  execSync(`tar ${flags} "${file}" -C "${dest}" rslsync`, { stdio: "inherit" });
  execSync(`chmod +x "${path.join(dest, "rslsync")}"`);
} else if (platform === "osx") {
  // The volume is called "Resilio Sync" (with a space), so take the whole
  // mount-point column instead of the last whitespace-separated token.
  const attach = execSync(`hdiutil attach -nobrowse -readonly "${file}"`).toString();
  const mount = attach.split("\n").map((l) => l.match(/(\/Volumes\/.*)$/)?.[1]?.trim()).find(Boolean);
  if (!mount) throw new Error(`could not determine DMG mount point:\n${attach}`);
  execSync(`cp -R "${mount}/Resilio Sync.app" "${dest}/"`, { stdio: "inherit" });
  execSync(`hdiutil detach "${mount}"`);
} else {
  // Windows: the download is the program itself, not an installer (Resilio
  // documents `/noinstall` for running it in place). Give it the name the
  // launcher probes first so the bundled copy wins over system installs.
  // `install` in resilio.lock.json is the name the launcher probes first
  // (transport::resilio::bundled_install_name reads the same field).
  if (path.extname(file).toLowerCase() !== ".exe") {
    console.error(`windows artifact must be the .exe program, got ${path.basename(file)} (an .msi cannot run in place)`);
    process.exit(1);
  }
  if (!artifact.install) {
    console.error("resilio.lock.json: artifacts.windows.install is missing");
    process.exit(1);
  }
  const target = path.join(dest, artifact.install);
  if (target !== file) {
    renameSync(file, target);
    file = target;
  }
}
// Machine-readable summary for the lock workflow.
console.log(`::lock:: ${JSON.stringify({ platform, url: artifact.url, sha256: sha, bytes: buf.length, file })}`);
console.log(`done -> ${dest}`);

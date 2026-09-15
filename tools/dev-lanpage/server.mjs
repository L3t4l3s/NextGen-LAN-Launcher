// Minimal stand-in for an ETI LANPage host, for local development:
//   node tools/dev-lanpage/server.mjs [port]
// Serves launcher.ini, launcher.css, theme.json and a stats.php-compatible
// endpoint that prints what the launcher reports. Point the launcher's
// "LANPage-Adresse" setting at 127.0.0.1:<port>.
import http from "node:http";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const port = Number(process.argv[2] ?? 8080);
const ini = readFileSync(path.join(here, "launcher.ini"), "utf8");
const css = readFileSync(path.join(here, "launcher.css"), "utf8");
const theme = readFileSync(path.join(here, "..", "..", "themes", "beispiel-lan.json"), "utf8");

http
  .createServer((req, res) => {
    const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
    if (url.pathname === "/launcher.ini") return res.writeHead(200, { "content-type": "text/plain" }).end(ini);
    if (url.pathname === "/launcher.css") return res.writeHead(200, { "content-type": "text/css" }).end(css);
    if (url.pathname === "/theme.json") return res.writeHead(200, { "content-type": "application/json" }).end(theme);
    if (url.pathname === "/stats.php") {
      const params = Object.fromEntries(url.searchParams.entries());
      console.log(new Date().toISOString(), "stats", params);
      return res.writeHead(200, { "content-type": "text/plain" }).end(params.macaddr1 ? "ok" : "error");
    }
    res.writeHead(404).end("not found");
  })
  .listen(port, () => console.log(`dev LANPage on http://127.0.0.1:${port}`));

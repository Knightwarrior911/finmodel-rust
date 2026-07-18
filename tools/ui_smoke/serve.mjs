// Minimal static file server for headless UI verification (dev-only, not shipped).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";

const root = process.argv[2] || ".";
const port = Number(process.argv[3] || 8917);
const types = {
  ".html": "text/html",
  ".mjs": "text/javascript",
  ".js": "text/javascript",
  ".css": "text/css",
  ".woff2": "font/woff2",
  ".json": "application/json",
  ".svg": "image/svg+xml",
};

createServer(async (req, res) => {
  try {
    let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (p === "/") p = "/index.html";
    const abs = normalize(join(root, p));
    const buf = await readFile(abs);
    res.writeHead(200, { "content-type": types[extname(abs)] || "application/octet-stream" });
    res.end(buf);
  } catch (e) {
    res.writeHead(404);
    res.end("not found");
  }
}).listen(port, () => console.log(`serving ${root} on http://localhost:${port}`));
